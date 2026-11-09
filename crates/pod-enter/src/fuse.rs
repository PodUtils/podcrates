//! TODO/packaging.md T-1003: the FUSE rung's staging and read-only server.
//!
//! memfd stages bytes into a fd, tmpfs stages the tree onto a mount, rundir
//! stages it into a directory: the FUSE rung serves the extracted tree over
//! `/dev/fuse` without copying it, and the payload enters the mount. The
//! mount needs what the other rungs do not: the node must open, a `mount(2)`
//! carrying the session fd must attach, and a server must answer the
//! kernel's requests for as long as the payload runs.
//!
//! ⛔ Read-only, by reply rather than by mount flag. Every mutating opcode
//! answers `EROFS` and every unknown one `ENOSYS`: a payload that writes
//! gets the errno, never a silent drop and never a write to the store's
//! own tree.
//!
//! ⛔ The server is a forked child, never a thread: the T-0603 assertion
//! scans this crate, and a thread would make the clone below fork a
//! threaded process. The child mounts, serves, and dies with the run; the
//! parent bounds every wait on it, kills it on every path, and unmounts
//! and removes the mount directory behind it.
//!
//! ⚠ What this module does NOT prove. No reachable machine holds
//! `/dev/fuse` (absent on the lane, uncreatable: `mknod` is `EPERM` even
//! in a user namespace, measured 2026-09-30) or grants a mount
//! (`mount(2)` is `EPERM` even in a user namespace, measured the same
//! day). The live drive therefore proves the refusal arms and the mount
//! failure's cleanup; entry over a real session needs a host with the
//! node and a granted mount, and names it in the refusal below.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use podvm_probe::sys::{self, CBuf};

use crate::stage;

/// The node the rung is named for.
pub const FUSE_NODE: &str = "/dev/fuse";

/// How long the parent waits for the server's mounted word and its serving
/// word before killing it. A bound reached is a refusal, never a longer
/// wait: TODO/RULES.md section 8's own rule.
const READY_TIMEOUT_MS: i64 = 10_000;

/// The request buffer. The kernel hands one request per read, and
/// `FUSE_MIN_READ_BUFFER` (8192, `/usr/include/linux/fuse.h:70`) is its
/// floor; reads are capped at 128 KiB beside the reply cap, so no request
/// sizes this buffer.
const IO_BUFFER: usize = 128 * 1024;

/// The largest single READ this server answers. Reads past it are served
/// whole up to this bound and refused past it: an unbounded reply buffer
/// is the whole-unbounded-response pattern `forbidden-patterns.md`
/// refuses, and no reader the payload uses asks past 128 KiB.
const MAX_READ: u64 = 128 * 1024;

/// Opcodes this server answers, read at `/usr/include/linux/fuse.h:517-566`
/// on the lane rather than remembered.
const OP_LOOKUP: u32 = 1;
const OP_FORGET: u32 = 2;
const OP_GETATTR: u32 = 3;
const OP_READLINK: u32 = 5;
const OP_OPEN: u32 = 14;
const OP_READ: u32 = 15;
const OP_STATFS: u32 = 17;
const OP_RELEASE: u32 = 18;
const OP_INIT: u32 = 26;
const OP_OPENDIR: u32 = 27;
const OP_READDIR: u32 = 28;
const OP_RELEASEDIR: u32 = 29;
const OP_ACCESS: u32 = 34;
const OP_INTERRUPT: u32 = 36;
const OP_DESTROY: u32 = 38;
const OP_BATCH_FORGET: u32 = 42;

/// Mutating opcodes, answered `EROFS` rather than implemented: setattr,
/// symlink, mknod, mkdir, unlink, rmdir, rename, link, write, create and
/// every xattr/flock/fallocate/tmpfile word. Same header, same lines.
const OP_MUTATING: [u32; 24] = [
    4, 6, 8, 9, 10, 11, 12, 13, 16, 20, 21, 22, 23, 24, 25, 31, 32, 33, 35, 37, 43, 45, 46, 47,
];

/// The ABI this server speaks, read at the same header (`:235-238` there
/// the kernel is 7.38). The kernel negotiates down on an older one; the
/// structs below are parsed by the negotiated version, so offering the
/// read one keeps the offer and the bytes consistent.
const ABI_MAJOR: u32 = 7;
const ABI_MINOR: u32 = 38;

/// `dirent.h`'s types for the READDIR reply: unknown, directory, regular,
/// symlink. Stable kernel ABI, like the opcodes above.
const DT_UNKNOWN: u32 = 0;
const DT_DIR: u32 = 4;
const DT_REG: u32 = 8;
const DT_LNK: u32 = 10;

/// `sys/mount.h`'s pair for a userspace mount: neither the set-uid bit
/// nor device nodes pass through it. `MS_PRIVATE` beside them documents
/// its header in `sys.rs:469-475`; these ride the same ABI.
const MS_NOSUID: u64 = 0x2;
const MS_NODEV: u64 = 0x4;

fn errno(n: i32) -> sys::Errno {
    sys::Errno(n)
}

fn put_u32(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u64(out: &mut [u8], at: usize, v: u64) {
    out[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn get_u32(req: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([req[at], req[at + 1], req[at + 2], req[at + 3]])
}

fn get_u64(req: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        req[at],
        req[at + 1],
        req[at + 2],
        req[at + 3],
        req[at + 4],
        req[at + 5],
        req[at + 6],
        req[at + 7],
    ])
}

/// Whether the device node opens: the rung's admission gate.
///
/// `path` is the parameter so tests drive both arms without the node: an
/// absent path refuses naming it, a regular file opens. [`check_node`]
/// pins the real node.
pub fn check_node_at(path: &str) -> Result<(), stage::Refusal> {
    let node = CBuf::new(path).ok_or_else(|| stage::Refusal {
        path: PathBuf::from(path),
        why: "the FUSE node path contains a NUL byte".to_string(),
    })?;
    match sys::open(&node, sys::O_RDWR | sys::O_CLOEXEC, 0) {
        Ok(fd) => {
            let _ = sys::close(fd);
            Ok(())
        }
        Err(e) => Err(stage::Refusal {
            path: PathBuf::from(path),
            why: format!(
                "/dev/fuse did not open ({} ({})): the FUSE rung needs the \
                 node this machine does not hold",
                e.name(),
                e.0
            ),
        }),
    }
}

/// [`check_node_at`] over the real node.
pub fn check_node() -> Result<(), stage::Refusal> {
    check_node_at(FUSE_NODE)
}

/// The mount directory: a unique directory under the store's fuse/ base.
///
/// The server mounts over it and the payload enters it; [`release_mount`]
/// unmounts and removes it on every path. Private: tests reach it through
/// [`serve_tree`]'s refusal arms and the in-module tests below.
fn stage_mount_dir(store_root: &Path) -> Result<PathBuf, stage::Refusal> {
    let base = store_root.join("fuse");
    std::fs::create_dir_all(&base).map_err(|e| stage::Refusal {
        path: base.clone(),
        why: format!("the fuse base could not be prepared: {e}"),
    })?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = base.join(format!("mnt-{}-{nanos}", std::process::id()));
    if dir.exists() {
        return Err(stage::Refusal {
            path: dir,
            why: "the mount directory already exists: refusing rather than mounting over a stale tree"
                .to_string(),
        });
    }
    std::fs::create_dir_all(&dir).map_err(|e| stage::Refusal {
        path: dir.clone(),
        why: format!("the mount directory could not be created: {e}"),
    })?;
    Ok(dir)
}

/// The serving mount: the server's pid and the directory it serves over.
///
/// Created by [`serve_tree`], ended by [`release_mount`] on every path,
/// on success and on failure alike.
#[derive(Debug)]
pub struct ActiveMount {
    /// The forked server's pid, killed and reaped by [`release_mount`].
    pub pid: i64,
    /// The mount target: the payload's root while active.
    pub dir: PathBuf,
}

/// Serve the extracted tree at `backing` over a FUSE mount.
///
/// The node must open, the mount must attach, and the server must answer
/// INIT, each bounded: any failure removes the mount directory and
/// refuses naming the step, so a half-staged FUSE mount never survives
/// to meet the next run. The tree is served, never copied: reads go to
/// the store's own extraction, writes are refused with `EROFS`.
pub fn serve_tree(store_root: &Path, backing: &Path) -> Result<ActiveMount, stage::Refusal> {
    if !backing.is_dir() {
        return Err(stage::Refusal {
            path: backing.to_path_buf(),
            why: "not a directory podvm can serve".to_string(),
        });
    }
    check_node()?;
    let dir = stage_mount_dir(store_root)?;
    let node = CBuf::new(FUSE_NODE).expect("a literal carries no NUL");
    let fd = sys::open(&node, sys::O_RDWR | sys::O_CLOEXEC, 0).map_err(|e| {
        let _ = std::fs::remove_dir_all(&dir);
        stage::Refusal {
            path: PathBuf::from(FUSE_NODE),
            why: format!(
                "/dev/fuse did not open ({} ({})): the FUSE rung needs the \
                 node this machine does not hold",
                e.name(),
                e.0
            ),
        }
    })?;
    let mut pipe = [0i32; 2];
    if sys::pipe2(&mut pipe, sys::O_CLOEXEC).is_err() {
        let _ = sys::close(fd);
        let _ = std::fs::remove_dir_all(&dir);
        return Err(stage::Refusal {
            path: dir,
            why: "the readiness pipe for the FUSE server could not be made".to_string(),
        });
    }
    let (read_end, write_end) = (pipe[0] as i64, pipe[1] as i64);
    let backing_owned = backing.to_path_buf();
    let dir_owned = dir.clone();
    let pid = match unsafe { sys::clone_fork(sys::SIGCHLD) } {
        Ok(0) => child_main(fd, read_end, write_end, &backing_owned, &dir_owned),
        Ok(pid) => pid,
        Err(e) => {
            let _ = sys::close(write_end);
            let _ = sys::close(read_end);
            let _ = sys::close(fd);
            let _ = std::fs::remove_dir_all(&dir);
            return Err(stage::Refusal {
                path: dir,
                why: format!(
                    "clone(2) for the FUSE server failed: {} ({})",
                    e.name(),
                    e.0
                ),
            });
        }
    };
    let _ = sys::close(write_end);
    // ⛔ The parent's copy of the session fd closes once the server is
    // serving: the session lives on the server's copy, and a copy held
    // here would keep the session open past the server's death.
    let outcome = wait_for_server(pid, read_end, &dir, fd);
    let _ = sys::close(read_end);
    outcome
}

/// The parent half of [`serve_tree`]: two bounded words, then the rung.
///
/// The server writes a 4-byte errno after the mount (0 where it
/// attached) and a second one after answering INIT. Between them the
/// parent stats the mount to trigger the INIT. Any other outcome kills
/// and reaps the server, unmounts where mounted, removes the directory,
/// and refuses naming the step.
fn wait_for_server(
    pid: i64,
    read_end: i64,
    dir: &Path,
    fd: i64,
) -> Result<ActiveMount, stage::Refusal> {
    let kill_and_clean = |dir: &Path| {
        let _ = sys::kill(pid, 9);
        let _ = crate::Child { pid }.wait_bounded(5_000);
        if let Some(target) = CBuf::new(&dir.to_string_lossy()) {
            let _ = sys::umount(&target);
        }
        let _ = std::fs::remove_dir_all(dir);
    };
    let first = read_word(read_end, READY_TIMEOUT_MS);
    match first {
        // The mount attached. Stat the target to trigger INIT, then wait
        // for the serving word.
        Some(0) => {
            let _ = std::fs::metadata(dir);
            match read_word(read_end, READY_TIMEOUT_MS) {
                Some(0) => {
                    let _ = sys::close(fd);
                    Ok(ActiveMount {
                        pid,
                        dir: dir.to_path_buf(),
                    })
                }
                _ => {
                    kill_and_clean(dir);
                    Err(stage::Refusal {
                        path: dir.to_path_buf(),
                        why: "the FUSE server mounted but never answered INIT within 10 s: \
                              bound reached, server killed, mount released"
                            .to_string(),
                    })
                }
            }
        }
        // The mount call refused: the child's errno travels in the word.
        Some(n) => {
            let _ = crate::Child { pid }.wait_bounded(5_000);
            if let Some(target) = CBuf::new(&dir.to_string_lossy()) {
                let _ = sys::umount(&target);
            }
            let _ = std::fs::remove_dir_all(dir);
            let e = errno(n as i32);
            Err(stage::Refusal {
                path: dir.to_path_buf(),
                why: format!(
                    "the FUSE mount was refused ({} ({})): the FUSE rung needs a \
                     mount this runtime does not grant",
                    e.name(),
                    e.0
                ),
            })
        }
        // EOF or the bound: the server is gone or wedged.
        None => {
            kill_and_clean(dir);
            Err(stage::Refusal {
                path: dir.to_path_buf(),
                why: "the FUSE server did not mount within 10 s: bound reached, \
                      server killed, mount released"
                    .to_string(),
            })
        }
    }
}

/// One 4-byte little-endian word off the readiness pipe, or `None` on EOF
/// or the bound. A 4-byte write is atomic, so a ready pipe carries the
/// whole word: no framing, no second read.
fn read_word(fd: i64, timeout_ms: i64) -> Option<u32> {
    let mut fds = [sys::PollFd {
        fd: fd as i32,
        events: sys::POLLIN,
        revents: 0,
    }];
    match sys::ppoll(&mut fds, timeout_ms) {
        Ok(n) if n > 0 => {}
        _ => return None,
    }
    let mut buf = [0u8; 4];
    let mut got = 0usize;
    while got < 4 {
        match sys::read(fd, &mut buf[got..]) {
            Ok(0) => return None,
            Ok(n) => got += n as usize,
            Err(e) if e == sys::EINTR => continue,
            Err(_) => return None,
        }
    }
    Some(u32::from_le_bytes(buf))
}

/// End the serving mount: kill and reap the server, unmount, remove.
///
/// Best-effort every half, like [`stage::release_tmpfs`]: the kill is
/// what stops the serving, the unmount is what frees the target, and the
/// removal is what keeps the next run from meeting this one. A directory
/// that was never mounted removes like any other.
pub fn release_mount(mounted: &ActiveMount) {
    // ⚠ SIGKILL, not SIGTERM: the server masks nothing and traps
    // nothing, and a server past its payload must die rather than flush.
    let _ = sys::kill(mounted.pid, 9);
    let _ = crate::Child { pid: mounted.pid }.wait_bounded(5_000);
    if let Some(target) = CBuf::new(&mounted.dir.to_string_lossy()) {
        let _ = sys::umount(&target);
    }
    let _ = std::fs::remove_dir_all(&mounted.dir);
}

/// The server child: mount, report, serve, exit. Never returns.
fn child_main(fd: i64, read_end: i64, write_end: i64, backing: &Path, dir: &Path) -> ! {
    let _ = sys::close(read_end);
    let report = |n: u32| {
        let _ = write_all(write_end, &n.to_le_bytes());
    };
    match mount_fuse(fd, dir) {
        Ok(()) => report(0),
        Err(e) => {
            report(e.0 as u32);
            let _ = sys::close(write_end);
            let _ = sys::close(fd);
            sys::exit_group(1);
        }
    }
    let code = serve_until_destroy(fd, backing, write_end);
    let _ = sys::close(write_end);
    let _ = sys::close(fd);
    sys::exit_group(code);
}

/// `mount(2)` the session fd over `dir`.
///
/// `sys::mount` carries no data argument (the tmpfs rung needs none), so
/// this issues the raw number beside it, the way `memfd.rs` issues its
/// own: source and fstype `fuse`, flags `MS_NOSUID|MS_NODEV`, data
/// `fd=N,rootmode=OOO,user_id=U,group_id=G`. The option shape is the
/// kernel's, read in the corpus at
/// `references/apptainer__apptainer/tree/internal/pkg/runtime/engine/apptainer/container_linux.go:563`.
fn mount_fuse(fd: i64, dir: &Path) -> Result<(), sys::Errno> {
    let mode = std::fs::metadata(dir).map_or(0o755, |m| {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o7777
    }) | 0o040000;
    let data = format!(
        "fd={fd},rootmode={mode:o},user_id={},group_id={}",
        euid(),
        egid()
    );
    let (Some(source), Some(target), Some(fstype), Some(data)) = (
        CBuf::new("fuse"),
        CBuf::new(&dir.to_string_lossy()),
        CBuf::new("fuse"),
        CBuf::new(&data),
    ) else {
        return Err(errno(22));
    };
    // ⛔ The CBufs outlive the call: the kernel reads every pointer
    // before returning, so nothing here may drop first.
    match unsafe {
        sys::sys(
            sys::SYS_MOUNT,
            [
                source.ptr(),
                target.ptr(),
                fstype.ptr(),
                MS_NOSUID | MS_NODEV,
                data.ptr(),
                0,
            ],
        )
    } {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

/// The mounting ids. No wrapper names them in `sys.rs`, so these issue
/// the raw numbers beside `memfd.rs`'s own raw calls. Neither call can
/// fail; the fallback is root's pair, which the mount then judges.
fn euid() -> u32 {
    unsafe { sys::sys(sys::SYS_GETUID, [0, 0, 0, 0, 0, 0]).map(|n| n as u32) }.unwrap_or(0)
}

/// [`euid`] for the group.
fn egid() -> u32 {
    unsafe { sys::sys(sys::SYS_GETGID, [0, 0, 0, 0, 0, 0]).map(|n| n as u32) }.unwrap_or(0)
}

fn write_all(fd: i64, mut bytes: &[u8]) -> Result<(), sys::Errno> {
    while !bytes.is_empty() {
        match sys::write(fd, bytes) {
            Ok(0) => return Err(errno(5)),
            Ok(n) => bytes = &bytes[n as usize..],
            Err(e) if e == sys::EINTR => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

mod fs;
use fs::*;

mod wire;
use wire::*;

#[cfg(test)]
mod tests;
