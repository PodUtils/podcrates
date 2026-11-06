use super::*;

// ------------------------------------------------------------ attribution

pub(super) fn a_mount_bogus() -> Outcome {
    let (src, tgt, fs) = (c("none"), c(BOGUS), c("tmpfs"));
    Outcome::from(unsafe { sys::sys(sys::SYS_MOUNT, [src.ptr(), tgt.ptr(), fs.ptr(), 0, 0, 0]) })
}

pub(super) fn a_umount_bogus() -> Outcome {
    let tgt = c(BOGUS);
    Outcome::from(unsafe { sys::sys(sys::SYS_UMOUNT2, [tgt.ptr(), 0, 0, 0, 0, 0]) })
}

pub(super) fn a_pivot_bogus() -> Outcome {
    let p = c(BOGUS);
    Outcome::from(unsafe { sys::sys(sys::SYS_PIVOT_ROOT, [p.ptr(), p.ptr(), 0, 0, 0, 0]) })
}

pub(super) fn a_process_vm_readv() -> Outcome {
    let mut buf = [0u8; 1];
    let local = [buf.as_mut_ptr() as u64, 1u64];
    let remote = [0x1000u64, 1u64];
    Outcome::from(unsafe {
        sys::sys(
            sys::SYS_PROCESS_VM_READV,
            [
                999_999,
                local.as_ptr() as u64,
                1,
                remote.as_ptr() as u64,
                1,
                0,
            ],
        )
    })
}

/// The supervise tier's argument channel, T-0606.
///
/// ⛔ Not the discriminator above. A bogus pid separates a filter from a
/// later policy; it never copies a byte. This aims the remote end at one
/// byte of our own memory and copies it: the returned count is the
/// integrity check, so an `Ok` answers the channel carries arguments and a
/// denial answers it is filtered or absent. `references/multikernel__sandlock`
/// reads the child's arguments through exactly this call and has no
/// `/proc/pid/mem` fallback anywhere in its tree.
/// ⚠ The count is read through `Outcome::from` beside every other body: for
/// a valid single-byte iov the kernel answers 1 or an errno, and a short
/// count has no errno to carry, so there is no honest third verdict for it.
pub(super) fn s_read_channel() -> Outcome {
    static MARK: u8 = 0x5a;
    let mut buf = [0u8; 1];
    let local = [buf.as_mut_ptr() as u64, 1u64];
    let remote = [&MARK as *const u8 as u64, 1u64];
    Outcome::from(unsafe {
        sys::sys(
            sys::SYS_PROCESS_VM_READV,
            [
                sys::getpid() as u64,
                local.as_ptr() as u64,
                1,
                remote.as_ptr() as u64,
                1,
                0,
            ],
        )
    })
}

pub(super) fn a_pidfd_getfd() -> Outcome {
    let m1 = -1i64 as u64;
    Outcome::from(unsafe { sys::sys(sys::SYS_PIDFD_GETFD, [m1, m1, 0, 0, 0, 0]) })
}

/// TODO/supervise.md T-0606. Supervision's first leg: a pidfd for the
/// caller's own pid, which answers wherever the number exists. ENOSYS is
/// "this kernel cannot answer" (pidfd_open needs 5.3), not a refusal.
pub(super) fn s_pidfd_self() -> Outcome {
    match sys::pidfd_open(sys::getpid()) {
        Ok(fd) => {
            let _ = sys::close(fd);
            Outcome::ok()
        }
        Err(sys::ENOSYS) => Outcome::skip(
            Some(sys::ENOSYS),
            "pidfd_open(2) is not present on this kernel, so supervision through pidfds cannot be measured here",
        ),
        Err(e) => Outcome::denied(e),
    }
}

/// TODO/supervise.md T-0606. Supervision's second leg: a trivial child
/// reaped through its pidfd, the composition the launcher runs (T-0601,
/// T-0602). The verdict comes from waitid answering, never from the
/// reap: WNOHANG returns at once, so nothing here waits, and a child
/// that outruns the cap is reaped by init with the mechanism proven.
pub(super) fn s_waitid_child() -> Outcome {
    let child = match unsafe { sys::clone_fork(sys::SIGCHLD) } {
        Err(e) => return Outcome::from(Err(e)),
        Ok(0) => sys::exit_group(0),
        Ok(pid) => pid,
    };
    let fd = match sys::pidfd_open(child) {
        Err(e) => return Outcome::from(Err(e)),
        Ok(fd) => fd,
    };
    for _ in 0..4096 {
        match sys::waitid_pidfd(fd, true) {
            Err(e) => {
                let _ = sys::close(fd);
                if e == sys::ENOSYS {
                    return Outcome::skip(
                        Some(sys::ENOSYS),
                        "waitid(P_PIDFD) is not present on this kernel, so supervision through pidfds cannot be measured here",
                    );
                }
                return Outcome::denied(e);
            }
            Ok(Some(_)) => {
                let _ = sys::close(fd);
                return Outcome::ok();
            }
            Ok(None) => {}
        }
    }
    let _ = sys::close(fd);
    Outcome::ok()
}

pub(super) fn a_kcmp() -> Outcome {
    let m1 = -1i64 as u64;
    let r = unsafe { sys::sys(sys::SYS_KCMP, [m1, m1, 0, 0, 0, 0]) };
    match r {
        // ⭐ TODO/probe.md T-0102's correction. `kcmp(2)` exists only where the
        // kernel was built with CONFIG_CHECKPOINT_RESTORE. ENOSYS is that
        // kernel saying the control is unavailable, which is not the same
        // statement as "the control answered". A control that cannot answer is
        // reported as unable to, so the discriminator it belongs to says it has
        // stopped discriminating rather than reporting a mechanism.
        Err(sys::ENOSYS) => Outcome::skip(
            Some(sys::ENOSYS),
            "kcmp(2) is not present on this kernel (CONFIG_CHECKPOINT_RESTORE is \
             unset), so this control cannot answer here",
        ),
        other => Outcome::from(other),
    }
}

pub(super) fn a_fsopen() -> Outcome {
    let fs = c("tmpfs");
    match unsafe { sys::sys(sys::SYS_FSOPEN, [fs.ptr(), 0, 0, 0, 0, 0]) } {
        Ok(fd) => {
            let _ = sys::close(fd);
            Outcome::ok()
        }
        Err(e) => Outcome::denied(e),
    }
}

pub(super) fn a_fsmount() -> Outcome {
    match detached_tmpfs() {
        Ok(mfd) => {
            let _ = sys::close(mfd);
            // ⭐ TODO/probe.md T-0103: fsmount runs the same may_mount() check
            // move_mount and unshare(CLONE_NEWNS) use, so its success proves no
            // mount denial on this machine is a capability problem. Saying so
            // in the row stops the next reader hunting for a capability.
            Outcome::ok_with("may_mount() passes, so no mount denial here is a capability problem")
        }
        Err((e, Step::Fsmount)) => Outcome::denied(e),
        Err((e, step)) => precondition(step.label(), e),
    }
}

pub(super) fn a_open_tree() -> Outcome {
    let p = c("/tmp");
    match unsafe {
        sys::sys(
            sys::SYS_OPEN_TREE,
            [sys::AT_FDCWD, p.ptr(), sys::OPEN_TREE_CLONE, 0, 0, 0],
        )
    } {
        Ok(fd) => {
            let _ = sys::close(fd);
            Outcome::ok()
        }
        Err(e) => Outcome::denied(e),
    }
}

pub(super) fn a_move_mount_bogus() -> Outcome {
    let mfd = match detached_tmpfs() {
        Ok(fd) => fd,
        Err((e, step)) => return precondition(step.label(), e),
    };
    let (empty, dest) = (sys::cempty(), c(BOGUS));
    let r = unsafe {
        sys::sys(
            sys::SYS_MOVE_MOUNT,
            [
                mfd as u64,
                empty.ptr(),
                sys::AT_FDCWD,
                dest.ptr(),
                sys::MOVE_MOUNT_F_EMPTY_PATH,
                0,
            ],
        )
    };
    let _ = sys::close(mfd);
    Outcome::from(r)
}

pub(super) fn a_move_mount_real() -> Outcome {
    const DEST: &str = "/tmp/mm-probe";
    let mfd = match detached_tmpfs() {
        Ok(fd) => fd,
        Err((e, step)) => return precondition(step.label(), e),
    };
    let dest = c(DEST);
    if let Err(e) = sys::mkdir(&dest, 0o755)
        && e != sys::EEXIST
    {
        let _ = sys::close(mfd);
        return precondition(&format!("mkdir {DEST}"), e);
    }
    let empty = sys::cempty();
    let r = unsafe {
        sys::sys(
            sys::SYS_MOVE_MOUNT,
            [
                mfd as u64,
                empty.ptr(),
                sys::AT_FDCWD,
                dest.ptr(),
                sys::MOVE_MOUNT_F_EMPTY_PATH,
                0,
            ],
        )
    };
    let _ = sys::close(mfd);
    match r {
        Err(e) => Outcome::denied(e),
        Ok(_) => match unsafe { sys::sys(sys::SYS_UMOUNT2, [dest.ptr(), 0, 0, 0, 0, 0]) } {
            Ok(_) => Outcome::ok(),
            Err(e) => Outcome::ok_with(format!(
                "note: the tmpfs attached at {DEST} could not be removed: umount2 \
                 answered {} ({}). It is still mounted.",
                e.name(),
                e.0
            )),
        },
    }
}

pub(super) fn a_openat_detached() -> Outcome {
    let mfd = match detached_tmpfs() {
        Ok(fd) => fd,
        Err((e, step)) => return precondition(step.label(), e),
    };
    let dot = c(".");
    let r = unsafe {
        sys::sys(
            sys::SYS_OPENAT,
            [
                mfd as u64,
                dot.ptr(),
                sys::O_RDONLY | sys::O_DIRECTORY,
                0,
                0,
                0,
            ],
        )
    };
    if let Ok(fd) = r {
        let _ = sys::close(fd);
    }
    let _ = sys::close(mfd);
    Outcome::from(r)
}

/// `struct sock_filter` and `struct sock_fprog`, in the kernel's layout. The
/// six padding bytes are the alignment of the pointer that follows the `u16`,
/// and writing them out is what stops the pointer landing four bytes early.
#[repr(C)]
struct SockFilter {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}

#[repr(C)]
struct SockFprog {
    len: u16,
    pad: [u8; 6],
    filter: *const SockFilter,
}

pub(super) fn a_seccomp_listener() -> Outcome {
    // BPF_RET|BPF_K with SECCOMP_RET_ALLOW: a filter that permits everything
    // and exists only to carry the notification listener.
    let filter = [SockFilter {
        code: 0x06,
        jt: 0,
        jf: 0,
        k: 0x7fff_0000,
    }];
    let prog = SockFprog {
        len: 1,
        pad: [0; 6],
        filter: filter.as_ptr(),
    };
    if let Err(e) = unsafe { sys::sys(sys::SYS_PRCTL, [sys::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0, 0]) } {
        return precondition("prctl(PR_SET_NO_NEW_PRIVS)", e);
    }
    let r = unsafe {
        sys::sys(
            sys::SYS_SECCOMP,
            [
                sys::SECCOMP_SET_MODE_FILTER,
                sys::SECCOMP_FILTER_FLAG_NEW_LISTENER,
                &prog as *const SockFprog as u64,
                0,
                0,
                0,
            ],
        )
    };
    match r {
        Ok(fd) => {
            let _ = sys::close(fd);
            Outcome::ok()
        }
        Err(e) => Outcome::denied(e),
    }
}

pub(super) fn open_probe(path: &str, flags: u64) -> Outcome {
    let p = c(path);
    match sys::open(&p, flags, 0) {
        Ok(fd) => {
            let _ = sys::close(fd);
            Outcome::ok()
        }
        Err(e) => Outcome::denied(e),
    }
}

pub(super) fn a_mem_rdonly() -> Outcome {
    open_probe("/proc/self/mem", sys::O_RDONLY)
}

pub(super) fn a_mem_rdwr() -> Outcome {
    open_probe("/proc/self/mem", sys::O_RDWR)
}

pub(super) fn a_landlock() -> Outcome {
    // (NULL, 0, LANDLOCK_CREATE_RULESET_VERSION) returns the ABI rather than a
    // ruleset fd. This row reports whether an LSM of the class that explains
    // mechanism M is present at all, which is not a verdict about podvm.
    match unsafe { sys::sys(sys::SYS_LANDLOCK_CREATE_RULESET, [0, 0, 1, 0, 0, 0]) } {
        Ok(abi) => Outcome::ok_with(format!("ABI {abi}")),
        Err(e) => Outcome::denied(e),
    }
}
