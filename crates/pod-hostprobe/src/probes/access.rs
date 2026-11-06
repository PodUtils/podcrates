use super::*;

pub const PTMX: &str = "/dev/ptmx";

pub(super) fn p_stat_ptmx() -> Outcome {
    match sys::stat(&c(PTMX)) {
        Ok(st) => Outcome::ok_with(format!(
            "{} {}:{} mode {:o}",
            st.kind(),
            st.rdev_major(),
            st.rdev_minor(),
            st.st_mode & 0o7777
        )),
        // ENOENT here is the answer, not a missing precondition: the
        // question this row exists for is whether the file is there.
        Err(e) => Outcome::denied(e),
    }
}

pub(super) fn p_open_ptmx() -> Outcome {
    // O_NOCTTY. Opening a terminal without it can make it the prober's
    // controlling terminal, which mutates the process doing the measuring.
    // The child is disposable either way; the flag is what makes the row a
    // measurement rather than a side effect.
    match sys::open(&c(PTMX), sys::O_RDWR | sys::O_NOCTTY, 0) {
        Ok(fd) => {
            // A successful open of a working ptmx allocates a pty pair. Closing
            // it releases the slave, so the probe leaves no pty behind.
            let _ = sys::close(fd);
            Outcome::ok()
        }
        Err(e) => Outcome::denied(e),
    }
}

/// Whether a FUSE rung may promise a mount.
///
/// One home for the question the ladder asks: only the OPEN row counts, with
/// an `Ok` verdict and nothing else, which is `ptmx_usable`'s own rule. The
/// `mknod` this runtime refuses cannot create the node where it is absent,
/// so absence is the answer rather than a missing precondition.
pub const FUSE: &str = "/dev/fuse";

pub(super) fn p_open_fuse() -> Outcome {
    open_probe(FUSE, sys::O_RDWR | sys::O_CLOEXEC)
}

/// The FUSE rung's probe input, beside `ptmx_usable`: true only where the
/// open row ran and answered `Ok`. A `Skip` never ran, so it promises
/// nothing; a `Denied` row is the refusal itself.
pub fn fuse_usable(findings: &crate::Findings) -> bool {
    findings
        .rows
        .iter()
        .any(|(n, out)| n.starts_with("open(/dev/fuse") && matches!(out.verdict, crate::verdict::Verdict::Ok))
}

// An instance of the confined target class answers
// EACCES to readdir("/") while opening entries by name still works, and
// denies creat at the top level: a launcher died on the first, running
// `find /` to collect bind sources. Three legs, each its own verdict, in
// the outer environment before any chroot.
pub(super) fn p_list_root() -> Outcome {
    match sys::open(&c("/"), sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC, 0) {
        Err(e) => Outcome::denied(e),
        Ok(fd) => {
            let res = sys::getdents64(fd);
            let _ = sys::close(fd);
            match res {
                Ok(entries) => Outcome::ok_with(format!("{} entries", entries.len())),
                Err(e) => Outcome::denied(e),
            }
        }
    }
}

pub(super) fn p_open_by_name() -> Outcome {
    // A fixed FHS name, documented: the leg asks whether opening by name
    // works, not whether this name exists. ENOENT where it does not is the
    // kernel's answer, not a harness failure.
    match sys::open(&c("/bin"), sys::O_RDONLY | sys::O_CLOEXEC, 0) {
        Ok(fd) => {
            let _ = sys::close(fd);
            Outcome::ok()
        }
        Err(e) => Outcome::denied(e),
    }
}

pub(super) fn p_creat_toplevel() -> Outcome {
    let path = format!("/pod-hostprobe-creat-{}", std::process::id());
    let cpath = match sys::CBuf::new(&path) {
        Some(b) => b,
        None => {
            return Outcome::skip(None, "the creat path is unrepresentable".to_string());
        }
    };
    // O_EXCL: refusing to clobber is part of the question, and the probe
    // leaves nothing behind either way.
    match sys::open(&cpath, sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL | sys::O_CLOEXEC, 0o600) {
        Err(e) => Outcome::denied(e),
        Ok(fd) => {
            let _ = sys::close(fd);
            match sys::unlink(&cpath) {
                Ok(_) => Outcome::ok(),
                Err(e) => Outcome::skip(Some(e), format!("created {path} but could not unlink it: left in place")),
            }
        }
    }
}

/// Whether the chroot rung may be entered.
///
/// One home for the question that a run, a create and a start all ask before
/// a fixup changes the rootfs: only the chroot row with an `Ok` verdict
/// promises entry. A `Skip` never ran, so it promises nothing; a `Denied`
/// row is the refusal itself.
pub fn chroot_usable(findings: &crate::Findings) -> bool {
    findings.rows.iter().any(|(n, out)| n.starts_with("chroot(") && matches!(out.verdict, crate::verdict::Verdict::Ok))
}

/// Whether `-t` may promise a pty.
///
/// One home for the question that a run and an exec both ask: only the OPEN
/// row counts, with an `Ok` verdict and nothing else. A `Skip` never ran, so
/// it promises nothing; a `Denied` row is the refusal itself.
/// Existence is not function, so the stat row does not answer.
pub fn ptmx_usable(findings: &crate::Findings) -> bool {
    findings
        .rows
        .iter()
        .any(|(n, out)| n.starts_with("open(/dev/ptmx") && matches!(out.verdict, crate::verdict::Verdict::Ok))
}

pub(super) fn p_tcp_listen() -> Outcome {
    // socket+bind+listen on 127.0.0.1 port 0: the kernel picks the port, so
    // no fixture can collide, and nothing is ever accepted on it. Closing
    // releases the port, so the probe leaves no listener behind.
    // sockaddr_in, 16 bytes: family, port, address, 8 zero bytes.
    let fd = match sys::socket(sys::AF_INET, sys::SOCK_STREAM | sys::SOCK_CLOEXEC, 0) {
        Ok(fd) => fd,
        Err(e) => return Outcome::denied(e),
    };
    let mut addr = [0u8; 16];
    // The family in native order: the crate builds for big-endian
    // architectures too, where a hard-coded
    // little-endian 2 reads as family 512. Port 0 and the zero bytes are
    // order-free; the address is big-endian by definition.
    let fam = 2u16.to_ne_bytes();
    addr[0] = fam[0];
    addr[1] = fam[1];
    addr[4] = 127;
    addr[7] = 1;
    let r = match sys::bind(fd, addr.as_ptr() as u64, 16) {
        Ok(_) => sys::listen(fd, 1).map(|_| ()),
        Err(e) => Err(e),
    };
    let _ = sys::close(fd);
    match r {
        Ok(()) => Outcome::ok_with("loopback TCP bind+listen permitted"),
        Err(e) => Outcome::denied(e),
    }
}
