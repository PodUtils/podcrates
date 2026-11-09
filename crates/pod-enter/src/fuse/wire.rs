use super::*;

#[cfg(unix)]
pub(super) fn euid_sync() -> u32 {
    euid()
}

#[cfg(not(unix))]
fn euid_sync() -> u32 {
    0
}

#[cfg(unix)]
pub(super) fn egid_sync() -> u32 {
    egid()
}

#[cfg(not(unix))]
fn egid_sync() -> u32 {
    0
}

/// An `io::Error` as the errno the reply carries: the name where the
/// kernel names it, `EIO` past that.
pub(super) fn map_io(e: std::io::Error) -> sys::Errno {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound => errno(2),
        K::PermissionDenied => errno(13),
        K::AlreadyExists => errno(17),
        K::InvalidInput => errno(22),
        _ => errno(5),
    }
}

pub(super) fn align8(n: usize) -> usize {
    (n + 7) & !7
}

#[cfg(unix)]
fn attr_bytes(meta: &std::fs::Metadata, out: &mut [u8; 88]) {
    use std::os::unix::fs::MetadataExt;
    put_u64(out, 0, meta.ino());
    put_u64(out, 8, meta.size());
    put_u64(out, 16, meta.blocks());
    put_u64(out, 24, meta.atime() as u64);
    put_u64(out, 32, meta.mtime() as u64);
    put_u64(out, 40, meta.ctime() as u64);
    put_u32(out, 48, meta.atime_nsec() as u32);
    put_u32(out, 52, meta.mtime_nsec() as u32);
    put_u32(out, 56, meta.ctime_nsec() as u32);
    put_u32(out, 60, meta.mode());
    put_u32(out, 64, meta.nlink() as u32);
    put_u32(out, 68, meta.uid());
    put_u32(out, 72, meta.gid());
    put_u32(out, 76, meta.rdev() as u32);
    put_u32(out, 80, meta.blksize() as u32);
    put_u32(out, 84, 0);
}

#[cfg(not(unix))]
fn attr_bytes(meta: &std::fs::Metadata, out: &mut [u8; 88]) {
    put_u64(out, 8, meta.len());
    if meta.is_dir() {
        put_u32(out, 60, 0o040755);
    } else {
        put_u32(out, 60, 0o100644);
    }
    put_u32(out, 64, 1);
}

/// The LOOKUP/regular reply: entry timeouts of zero (no caching: every
/// answer is fresh, which is the coherent half of serving a tree the
/// store never mutates mid-run) and the file's own attributes.
pub(super) fn entry_out(id: u64, meta: &std::fs::Metadata) -> Result<Vec<u8>, sys::Errno> {
    let mut out = vec![0u8; 128];
    put_u64(&mut out, 0, id);
    put_u64(&mut out, 8, 0);
    put_u64(&mut out, 16, 0);
    put_u64(&mut out, 24, 0);
    put_u32(&mut out, 32, 0);
    put_u32(&mut out, 36, 0);
    let mut attr = [0u8; 88];
    attr_bytes(meta, &mut attr);
    out[40..128].copy_from_slice(&attr);
    Ok(out)
}

/// The GETATTR reply: the same attributes under their own header.
pub(super) fn attr_out(meta: &std::fs::Metadata) -> Result<Vec<u8>, sys::Errno> {
    let mut out = vec![0u8; 104];
    put_u64(&mut out, 0, 0);
    put_u32(&mut out, 8, 0);
    put_u32(&mut out, 12, 0);
    let mut attr = [0u8; 88];
    attr_bytes(meta, &mut attr);
    out[16..104].copy_from_slice(&attr);
    Ok(out)
}

/// The INIT reply: the version offered, nothing capable, a 4 KiB largest
/// write (writes are refused anyway) and nanosecond timestamps.
fn init_out() -> Vec<u8> {
    let mut out = vec![0u8; 64];
    put_u32(&mut out, 0, ABI_MAJOR);
    put_u32(&mut out, 4, ABI_MINOR);
    put_u32(&mut out, 8, 0);
    put_u32(&mut out, 12, 0);
    put_u32(&mut out, 20, 4096);
    put_u32(&mut out, 24, 1);
    out
}

/// The STATFS reply: zeros with a 4 KiB block and a 255-name limit.
/// A usage figure would be invented (the backing is the store's own
/// filesystem, and its blocks are not this mount's), so this answers
/// the shape and no number.
fn statfs_out() -> Vec<u8> {
    let mut out = vec![0u8; 96];
    put_u32(&mut out, 32, 4096);
    put_u32(&mut out, 36, 255);
    put_u32(&mut out, 40, 4096);
    out
}

/// A reply frame: the length, the negated errno (0 where it worked),
/// the request's unique, and the payload.
fn reply(unique: u64, error: i32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + payload.len());
    out.extend_from_slice(&(16 + payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&error.to_le_bytes());
    out.extend_from_slice(&unique.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn ok(unique: u64, payload: &[u8]) -> Vec<u8> {
    reply(unique, 0, payload)
}

fn err(unique: u64, e: sys::Errno) -> Vec<u8> {
    reply(unique, -(e.0), &[])
}

/// Dispatch one request: the reply, or `None` where the kernel waits
/// for none (`FORGET`, `BATCH_FORGET`, `INTERRUPT`). The second half is
/// whether the loop exits after writing (`DESTROY`).
pub(super) fn dispatch(fs: &mut Fs, req: &[u8]) -> (Option<Vec<u8>>, bool) {
    if req.len() < 40 {
        return (None, false);
    }
    let len = get_u32(req, 0) as usize;
    if len > req.len() || len < 40 {
        return (None, false);
    }
    let opcode = get_u32(req, 4);
    let unique = get_u64(req, 8);
    let nodeid = get_u64(req, 16);
    let body = &req[40..len];
    let answered = |r: Result<Vec<u8>, sys::Errno>| {
        Some(match r {
            Ok(payload) => ok(unique, &payload),
            Err(e) => err(unique, e),
        })
    };
    match opcode {
        OP_INIT => (Some(ok(unique, &init_out())), false),
        OP_LOOKUP => (answered(fs.lookup(nodeid, cstr(body))), false),
        OP_GETATTR => (answered(fs.getattr(nodeid)), false),
        OP_READLINK => (answered(fs.readlink(nodeid)), false),
        OP_OPEN => {
            let dir = false;
            (answered(fs.open(nodeid, dir)), false)
        }
        OP_OPENDIR => (answered(fs.open(nodeid, true)), false),
        OP_READ | OP_READDIR => {
            if body.len() < 40 {
                return (Some(err(unique, errno(22))), false);
            }
            let arg_offset = get_u64(body, 8);
            let arg_size = get_u32(body, 16) as u64;
            if opcode == OP_READ {
                (answered(fs.read(nodeid, arg_offset, arg_size)), false)
            } else {
                (answered(fs.readdir(nodeid, arg_offset, arg_size)), false)
            }
        }
        OP_RELEASE | OP_RELEASEDIR => (Some(ok(unique, &[])), false),
        OP_ACCESS => {
            let mask = body.first().copied().unwrap_or(0) as u32
                | ((body.get(1).copied().unwrap_or(0) as u32) << 8)
                | ((body.get(2).copied().unwrap_or(0) as u32) << 16)
                | ((body.get(3).copied().unwrap_or(0) as u32) << 24);
            match fs.access(nodeid, mask) {
                Ok(()) => (Some(ok(unique, &[])), false),
                Err(e) => (Some(err(unique, e)), false),
            }
        }
        OP_STATFS => (Some(ok(unique, &statfs_out())), false),
        OP_FORGET | OP_BATCH_FORGET => {
            forget_in(fs, opcode, nodeid, body);
            (None, false)
        }
        OP_INTERRUPT => (None, false),
        OP_DESTROY => (Some(ok(unique, &[])), true),
        _ if OP_MUTATING.contains(&opcode) => (Some(err(unique, errno(30))), false),
        _ => (Some(err(unique, errno(38))), false),
    }
}

/// Apply a forget body: `FORGET` names its node in the header and
/// carries the count in the body; `BATCH_FORGET` carries id/count pairs.
fn forget_in(fs: &mut Fs, opcode: u32, nodeid: u64, body: &[u8]) {
    if opcode == OP_FORGET {
        if body.len() >= 8 {
            fs.forget(nodeid, get_u64(body, 0));
        }
        return;
    }
    if body.len() < 8 {
        return;
    }
    let count = get_u32(body, 0) as usize;
    let mut at = 8usize;
    for _ in 0..count {
        if body.len() < at + 16 {
            return;
        }
        fs.forget(get_u64(body, at), get_u64(body, at + 8));
        at += 16;
    }
}

/// The NUL-terminated name a LOOKUP carries, or an empty span where none
/// is there (which the lookup refuses as `ENOENT`).
pub(super) fn cstr(body: &[u8]) -> &[u8] {
    match body.iter().position(|b| *b == 0) {
        Some(n) => &body[..n],
        None => &[],
    }
}

/// Serve on `fd` until `DESTROY`: the child's whole job after the mount.
///
/// `ready` carries the serving word once INIT is answered: the parent
/// stats the mount to trigger it, so the first INIT arrives while this
/// loop runs. Every reply is written whole; a write failure ends the
/// server with the payload's fate decided by the kernel (transport
/// errors on a dead mount read as `ENOTCONN`, never as payload bytes).
pub(super) fn serve_until_destroy(fd: i64, backing: &Path, ready: i64) -> i32 {
    let mut fs = Fs::new(backing);
    let mut buf = vec![0u8; IO_BUFFER];
    // A stream may coalesce or split frames (a socket does; the node
    // hands one request per read). `pending` holds what arrived past
    // the frame in hand, so no byte is parsed twice or dropped either
    // way. A length past the buffer is garbage, never an allocation.
    let mut pending: Vec<u8> = Vec::new();
    let mut told = false;
    loop {
        while pending.len() < 40 {
            match sys::read(fd, &mut buf) {
                Ok(0) => return 0,
                Ok(n) => pending.extend_from_slice(&buf[..n as usize]),
                Err(e) if e == sys::EINTR => continue,
                Err(_) => return 1,
            }
        }
        let len = get_u32(&pending, 0) as usize;
        if !(40..=IO_BUFFER).contains(&len) {
            return 1;
        }
        while pending.len() < len {
            match sys::read(fd, &mut buf) {
                Ok(0) => return 1,
                Ok(n) => pending.extend_from_slice(&buf[..n as usize]),
                Err(e) if e == sys::EINTR => continue,
                Err(_) => return 1,
            }
        }
        let frame: Vec<u8> = pending[..len].to_vec();
        pending.drain(..len);
        let (answer, done) = dispatch(&mut fs, &frame);
        if !told {
            // The first request the kernel sends on a fresh mount is
            // INIT: its answer is the serving word the parent waits
            // for. Anything else first is still answered; the word
            // follows the first reply regardless.
            let _ = write_all(ready, &0u32.to_le_bytes());
            told = true;
        }
        if let Some(bytes) = answer
            && write_all(fd, &bytes).is_err()
        {
            return 1;
        }
        if done {
            return 0;
        }
    }
}
