use super::*;

// ---------------------------------------------------------------- census

pub(super) fn p_unshare_newns() -> Outcome {
    Outcome::from(unsafe { sys::sys(sys::SYS_UNSHARE, [sys::CLONE_NEWNS, 0, 0, 0, 0, 0]) })
}

pub(super) fn p_unshare_newuser() -> Outcome {
    // The kernel refuses `unshare(CLONE_NEWUSER)` to a multithreaded caller
    // with EINVAL whatever the policy, so an EINVAL from a threaded prober is
    // the prober's own answer and not the machine's. This child was newly
    // execed and has one thread, so the invariant holds. It is checked
    // rather than assumed, because the day somebody adds a thread pool the
    // check is what says so.
    match threads_of_self() {
        Some(n) if n > 1 => {
            return Outcome::skip(
                None,
                format!(
                    "this prober has {n} threads, and the kernel refuses \
                         unshare(CLONE_NEWUSER) to any multithreaded caller with \
                         EINVAL whatever the policy; the machine was not asked"
                ),
            );
        }
        _ => {}
    }
    Outcome::from(unsafe { sys::sys(sys::SYS_UNSHARE, [sys::CLONE_NEWUSER, 0, 0, 0, 0, 0]) })
}

pub(super) fn p_unshare_newpid() -> Outcome {
    Outcome::from(unsafe { sys::sys(sys::SYS_UNSHARE, [sys::CLONE_NEWPID, 0, 0, 0, 0, 0]) })
}

fn threads_of_self() -> Option<usize> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|l| l.strip_prefix("Threads:")).and_then(|v| v.trim().parse().ok())
}

pub(super) fn p_sethostname() -> Outcome {
    // Runs inside clone(CLONE_NEWUTS), so a success renames that namespace's
    // host and nothing else. Without the namespace the row is a `skip`, which
    // the child harness produces from the clone's own errno.
    const NAME: &str = "pod-hostprobe";
    let name = c(NAME);
    Outcome::from(unsafe { sys::sys(sys::SYS_SETHOSTNAME, [name.ptr(), NAME.len() as u64, 0, 0, 0, 0]) })
}

pub(super) fn p_mount_tmpfs() -> Outcome {
    // The target is NOT checked before the call, and that is the point. A
    // seccomp filter runs before the syscall body, so a filtered mount(2)
    // answers EPERM whether or not /mnt exists; a `stat` first would turn that
    // measurement into a skip and this machine's policy would go unmeasured.
    // The precondition is separated afterwards instead, from the errno: only
    // ENOENT is a statement about the target rather than about the mount.
    let (src, tgt, fs, data) = (c("none"), c("/mnt"), c("tmpfs"), sys::cempty());
    let r = unsafe { sys::sys(sys::SYS_MOUNT, [src.ptr(), tgt.ptr(), fs.ptr(), 0, data.ptr(), 0]) };
    match r {
        Err(sys::ENOENT) => Outcome::skip(
            Some(sys::ENOENT),
            "mount(2) executed and answered ENOENT, which is about the target \
             /mnt and not about the mount policy: /mnt does not exist here. The \
             policy itself is in the `mount(2) bogus target` row.",
        ),
        Err(e) => Outcome::denied(e),
        Ok(_) => {
            // A probe that mounts, unmounts. Where it cannot, the row says so.
            match unsafe { sys::sys(sys::SYS_UMOUNT2, [tgt.ptr(), 0, 0, 0, 0, 0]) } {
                Ok(_) => Outcome::ok(),
                Err(e) => Outcome::ok_with(format!(
                    "note: the tmpfs mounted at /mnt could not be removed: umount2 \
                     answered {} ({}). It is still mounted.",
                    e.name(),
                    e.0
                )),
            }
        }
    }
}

pub(super) fn p_mount_slave() -> Outcome {
    // Only ever reached inside clone(CLONE_NEWNS): see the module header. This
    // is bubblewrap's `MS_SLAVE` remount, the one whose failure it reports as
    // "Failed to make / slave".
    let (src, tgt, fs, data) = (sys::cempty(), c("/"), sys::cempty(), sys::cempty());
    Outcome::from(unsafe {
        sys::sys(sys::SYS_MOUNT, [src.ptr(), tgt.ptr(), fs.ptr(), sys::MS_SLAVE | sys::MS_REC, data.ptr(), 0])
    })
}

pub(super) fn p_pivot_root() -> Outcome {
    let p = c("/tmp");
    Outcome::from(unsafe { sys::sys(sys::SYS_PIVOT_ROOT, [p.ptr(), p.ptr(), 0, 0, 0, 0]) })
}

pub(super) fn p_ptrace() -> Outcome {
    // PTRACE_TRACEME is request 0.
    Outcome::from(unsafe { sys::sys(sys::SYS_PTRACE, [0, 0, 0, 0, 0, 0]) })
}

pub(super) fn p_mknod_real() -> Outcome {
    mknod_at("/tmp/nodprobe", sys::DEV_1_3)
}

pub(super) fn p_mknod_whiteout() -> Outcome {
    // makedev(0,0) is WHITEOUT_DEV and `vfs_mknod` exempts it from the
    // capability check, which is why it tests nothing on its own and why
    // the census needs the pair.
    mknod_at("/tmp/wprobe", 0)
}

fn mknod_at(path: &str, dev: u64) -> Outcome {
    if exists(path) {
        return already_there(path);
    }
    let p = c(path);
    // `sys::MKNOD` and not a bare number: on an architecture whose kernel has
    // no `mknod(2)` this is `mknodat(2)`, with the directory descriptor first,
    // and the row says which was issued. A probe measures a NAMED syscall, so calling one and printing another would be the exact
    // dishonesty this probe set exists to avoid.
    let e = sys::MKNOD;
    let args = if e.via_at {
        [sys::AT_FDCWD, p.ptr(), sys::S_IFCHR | 0o600, dev, 0, 0]
    } else {
        [p.ptr(), sys::S_IFCHR | 0o600, dev, 0, 0, 0]
    };
    let r = unsafe { sys::sys(e.nr, args) };
    if r.is_ok() {
        unscratch(path);
    }
    Outcome::from(r)
}

pub(super) fn p_setuid_1000() -> Outcome {
    Outcome::from(unsafe { sys::sys(sys::SYS_SETUID, [1000, 0, 0, 0, 0, 0]) })
}

pub(super) fn p_setuid_0() -> Outcome {
    Outcome::from(unsafe { sys::sys(sys::SYS_SETUID, [0, 0, 0, 0, 0, 0]) })
}

pub(super) fn p_setgid_0() -> Outcome {
    Outcome::from(unsafe { sys::sys(sys::SYS_SETGID, [0, 0, 0, 0, 0, 0]) })
}

pub(super) fn p_setgroups() -> Outcome {
    Outcome::from(unsafe { sys::sys(sys::SYS_SETGROUPS, [0, 0, 0, 0, 0, 0]) })
}

const CHOWN_TARGET: &str = "/tmp/chown-probe-target";

/// The entry point is part of the measurement, so it is passed in rather
/// than assumed. Where the kernel has no `chown(2)`, every architecture taking
/// its table from `asm-generic/unistd.h`, [`sys::CHOWN`] is `fchownat(2)` and
/// the row prints that name. The **verdict** is about the operation (may this
/// process give a file an id it does not map?), and the entry point is the
/// evidence under it.
fn chown_probe(e: sys::Entry, uid: u64, gid: u64) -> Outcome {
    if let Err(o) = make_scratch(CHOWN_TARGET) {
        return o;
    }
    let p = c(CHOWN_TARGET);
    let at_flags = if e.name.contains("AT_SYMLINK_NOFOLLOW") { sys::AT_SYMLINK_NOFOLLOW } else { 0 };
    let args = if e.via_at { [sys::AT_FDCWD, p.ptr(), uid, gid, at_flags, 0] } else { [p.ptr(), uid, gid, 0, 0, 0] };
    let r = unsafe { sys::sys(e.nr, args) };
    unscratch(CHOWN_TARGET);
    Outcome::from(r)
}

pub(super) fn p_chown_0_0() -> Outcome {
    chown_probe(sys::CHOWN, 0, 0)
}

pub(super) fn p_chown_0_42() -> Outcome {
    chown_probe(sys::CHOWN, 0, 42)
}

pub(super) fn p_lchown_0_42() -> Outcome {
    chown_probe(sys::LCHOWN, 0, 42)
}

pub(super) fn p_chown_1000_0() -> Outcome {
    chown_probe(sys::CHOWN, 1000, 0)
}

pub(super) fn p_chroot() -> Outcome {
    let p = c("/tmp");
    Outcome::from(unsafe { sys::sys(sys::SYS_CHROOT, [p.ptr(), 0, 0, 0, 0, 0]) })
}

fn memfd(name: &str) -> Result<i64, Errno> {
    // No MFD_CLOEXEC: the descriptor has to survive a fork and exec for
    // /proc/self/fd/N to name it in the payload.
    let n = c(name);
    unsafe { sys::sys(sys::SYS_MEMFD_CREATE, [n.ptr(), 0, 0, 0, 0, 0]) }
}

pub(super) fn p_memfd() -> Outcome {
    match memfd("probe") {
        Ok(fd) => {
            let _ = sys::close(fd);
            Outcome::ok()
        }
        Err(e) => Outcome::denied(e),
    }
}

pub(super) fn p_memfd_exec() -> Outcome {
    if let Err(o) = need_sh() {
        return o;
    }
    let fd = match memfd("probe") {
        Ok(fd) => fd,
        Err(e) => return precondition("memfd_create", e),
    };
    let script = b"#!/bin/sh\nexit 0\n";
    if let Err(e) = sys::write(fd, script) {
        let _ = sys::close(fd);
        return precondition("write to the memfd", e);
    }
    let out = run_payload(&format!("/proc/self/fd/{fd}"));
    let _ = sys::close(fd);
    out
}

pub(super) fn p_setsid() -> Outcome {
    Outcome::from(unsafe { sys::sys(sys::SYS_SETSID, [0, 0, 0, 0, 0, 0]) })
}

pub(super) fn p_pdeathsig() -> Outcome {
    Outcome::from(unsafe {
        sys::sys(sys::SYS_PRCTL, [sys::PR_SET_PDEATHSIG, 15 /* SIGTERM */, 0, 0, 0, 0])
    })
}

const EXEC_TARGET: &str = "/tmp/execprobe";

pub(super) fn p_exec_file() -> Outcome {
    if let Err(o) = need_sh() {
        return o;
    }
    if exists(EXEC_TARGET) {
        return already_there(EXEC_TARGET);
    }
    let fd = match create_exclusive(EXEC_TARGET, 0o755) {
        Ok(fd) => fd,
        Err(sys::EEXIST) => return already_there(EXEC_TARGET),
        Err(e) => return precondition(&format!("creating {EXEC_TARGET}"), e),
    };
    let r = sys::write(fd, b"#!/bin/sh\nexit 0\n");
    let _ = sys::close(fd);
    if let Err(e) = r {
        unscratch(EXEC_TARGET);
        return precondition(&format!("writing {EXEC_TARGET}"), e);
    }
    let out = run_payload(EXEC_TARGET);
    unscratch(EXEC_TARGET);
    out
}

pub(super) fn p_getrandom() -> Outcome {
    let mut buf = [0u8; 16];
    Outcome::from(unsafe { sys::sys(sys::SYS_GETRANDOM, [buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0, 0]) })
}

/// The one probe whose failure to create a file IS the measurement, which
/// is why it does not go through `make_scratch`. An `EEXIST` is still a skip:
/// the file that was in the way was somebody else's, and nothing about the
/// write policy was learned.
pub(super) fn p_write_etc() -> Outcome {
    const P: &str = "/etc/probe";
    match create_exclusive(P, 0o600) {
        Ok(fd) => {
            let _ = sys::close(fd);
            unscratch(P);
            Outcome::ok()
        }
        Err(sys::EEXIST) => already_there(P),
        Err(e) => Outcome::denied(e),
    }
}

/// The root-squash observation: a directory owned by an id that is not mapped
/// in this user namespace is unwritable even with `CAP_DAC_OVERRIDE`.
///
/// The fixture cannot be built from inside the confinement, because `chown` to
/// an unmapped id is exactly what is denied. A setup outside the confinement
/// stages it; the Go instrument looks for its own copy under `/tmp`.
pub(super) fn p_write_squash() -> Outcome {
    const CANDIDATES: [&str; 2] = ["/workspace/.fixtures/squash-probe", "/tmp/squash-probe"];
    let Some(dir) = CANDIDATES.iter().find(|d| exists(d)) else {
        return Outcome::skip(
            Some(sys::ENOENT),
            format!(
                "no fixture directory owned by an unmapped id: neither {} nor {} \
                 exists. Create one as another uid outside the confinement.",
                CANDIDATES[0], CANDIDATES[1]
            ),
        );
    };
    let path = format!("{dir}/pod-hostprobe-write");
    match create_exclusive(&path, 0o600) {
        Ok(fd) => {
            let _ = sys::close(fd);
            unscratch(&path);
            Outcome::ok_with(format!("wrote into {dir}"))
        }
        Err(sys::EEXIST) => already_there(&path),
        Err(e) => Outcome::denied(e),
    }
}
