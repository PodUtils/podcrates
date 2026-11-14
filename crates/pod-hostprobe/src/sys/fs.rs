use super::*;

/// `chroot(2)` on a path, used only as `chroot(".")` after an `fchdir` onto
/// a descriptor already held. `chroot` follows a symlink,
/// so a path checked and then passed is a path that can be swapped in between.
pub fn chroot(path: &CBuf) -> Sysres {
    unsafe { sys(SYS_CHROOT, [path.ptr(), 0, 0, 0, 0, 0]) }
}

/// `mount(2)` on a target already held, and `umount2(2)` on one.
/// A tmpfs rung mounts with kernel defaults
/// (flags 0, no data: no invented size) and unmounts before removing the
/// staging directory, so a failed mount never leaves a directory behind
/// that a later run could mistake for a staged tree.
pub fn mount(source: &CBuf, target: &CBuf, fstype: &CBuf, flags: u64) -> Sysres {
    unsafe { sys(SYS_MOUNT, [source.ptr(), target.ptr(), fstype.ptr(), flags, 0, 0]) }
}

/// `mount(2)` with the data of the file system (`size=` of a tmpfs, the
/// layers of an overlay).
pub fn mount_data(source: &CBuf, target: &CBuf, fstype: &CBuf, flags: u64, data: &CBuf) -> Sysres {
    unsafe { sys(SYS_MOUNT, [source.ptr(), target.ptr(), fstype.ptr(), flags, data.ptr(), 0]) }
}

pub fn umount(target: &CBuf) -> Sysres {
    unsafe { sys(SYS_UMOUNT2, [target.ptr(), 0, 0, 0, 0, 0]) }
}

/// `MNT_DETACH` of `umount2(2)`.
pub const MNT_DETACH: u64 = 2;

/// `umount2(target, MNT_DETACH)`: the mount leaves the tree at once, also
/// while a process holds a descriptor or a directory in it (where a plain
/// unmount gives `EBUSY`); the kernel frees it when the last one goes.
pub fn umount_detach(target: &CBuf) -> Sysres {
    unsafe { sys(SYS_UMOUNT2, [target.ptr(), MNT_DETACH, 0, 0, 0, 0]) }
}

/// `unshare(2)` with one flag word. The probes call the raw number
/// beside their rows (`probes.rs`: the `unshare(CLONE_NEWNS)` row), and
/// this is the half of the same call for an entry of a payload, so the two cannot drift
/// into different numbers.
pub fn unshare(flags: u64) -> Sysres {
    unsafe { sys(SYS_UNSHARE, [flags, 0, 0, 0, 0, 0]) }
}

/// `setns(2)` on a namespace descriptor or a pidfd (Linux 5.8 and later),
/// with the type of the namespace to join.
pub fn setns(fd: i64, nstype: u64) -> Sysres {
    unsafe { sys(SYS_SETNS, [fd as u64, nstype, 0, 0, 0, 0]) }
}

pub fn chdir(path: &CBuf) -> Sysres {
    unsafe { sys(SYS_CHDIR, [path.ptr(), 0, 0, 0, 0, 0]) }
}

pub fn fchdir(fd: i64) -> Sysres {
    unsafe { sys(SYS_FCHDIR, [fd as u64, 0, 0, 0, 0, 0]) }
}

/// `open(2)` by name, `openat(2)` in fact, see [`stat`] for why every
/// path-taking utility in this module routes through the `*at` form.
pub fn open(path: &CBuf, flags: u64, mode: u64) -> Sysres {
    openat(AT_FDCWD as i64, path, flags, mode)
}

pub fn read(fd: i64, buf: &mut [u8]) -> Sysres {
    unsafe { sys(SYS_READ, [fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0]) }
}

pub fn write(fd: i64, buf: &[u8]) -> Sysres {
    unsafe { sys(SYS_WRITE, [fd as u64, buf.as_ptr() as u64, buf.len() as u64, 0, 0, 0]) }
}

/// `getrandom(2)`. Use it for random bytes, not `/dev/urandom`.
///
/// A read of `/dev/urandom` with `std::fs::read` has no EOF and allocates
/// until the OOM killer stops the process. Inside a chroot the file can be a
/// shim, so it is not an entropy source at all. The kernel may return short; the caller loops.
pub fn getrandom(buf: &mut [u8]) -> Sysres {
    unsafe { sys(SYS_GETRANDOM, [buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0, 0]) }
}

pub fn unlink(path: &CBuf) -> Sysres {
    unlinkat(AT_FDCWD as i64, path, 0)
}

pub fn mkdir(path: &CBuf, mode: u64) -> Sysres {
    mkdirat(AT_FDCWD as i64, path, mode)
}

// ------------------------------------------------------- the `*at` wrappers
//
// Each resolves against a directory DESCRIPTOR. An extraction holds one open
// on the destination for the whole of a layer, so no path that it writes is
// resolved again from the root through components that another entry can
// replace in between.

pub fn openat(dirfd: i64, path: &CBuf, flags: u64, mode: u64) -> Sysres {
    unsafe { sys(SYS_OPENAT, [dirfd as u64, path.ptr(), flags, mode, 0, 0]) }
}

/// `openat2(2)`, whose `resolve` mask is enforced by the kernel rather than by
/// the caller.
///
/// Returns `ENOSYS` before Linux 5.6, and `E2BIG` where the kernel is older
/// than this `OpenHow`. Both mean "this kernel cannot answer", not "refused":
/// the caller falls back to the `O_NOFOLLOW` walk rather than treating either
/// as a denial.
pub fn openat2(dirfd: i64, path: &CBuf, how: &OpenHow) -> Sysres {
    unsafe {
        sys(
            SYS_OPENAT2,
            [dirfd as u64, path.ptr(), how as *const OpenHow as u64, std::mem::size_of::<OpenHow>() as u64, 0, 0],
        )
    }
}

pub fn mkdirat(dirfd: i64, path: &CBuf, mode: u64) -> Sysres {
    unsafe { sys(SYS_MKDIRAT, [dirfd as u64, path.ptr(), mode, 0, 0, 0]) }
}

pub fn unlinkat(dirfd: i64, path: &CBuf, flags: u64) -> Sysres {
    unsafe { sys(SYS_UNLINKAT, [dirfd as u64, path.ptr(), flags, 0, 0, 0]) }
}

pub fn renameat(old_dirfd: i64, old_path: &CBuf, new_dirfd: i64, new_path: &CBuf) -> Sysres {
    unsafe { sys(SYS_RENAMEAT, [old_dirfd as u64, old_path.ptr(), new_dirfd as u64, new_path.ptr(), 0, 0]) }
}

/// `mknodat(2)`. The completion of a rootfs CALLS THIS FIRST and makes a shim
/// only where it fails: on a machine that permits it, the payload gets a real
/// `/dev/null`. A shim on a machine that does not need one is a degradation
/// for no cause.
///
/// `dev` is the encoded device number: `makedev(major, minor)`.
pub fn mknodat(dirfd: i64, path: &CBuf, mode: u64, dev: u64) -> Sysres {
    unsafe { sys(SYS_MKNODAT, [dirfd as u64, path.ptr(), mode, dev, 0, 0]) }
}

/// The kernel's `makedev`. The wide encoding, which is what `mknodat`
/// takes on each architecture of the crate.
pub fn makedev(major: u64, minor: u64) -> u64 {
    ((major & 0xfff) << 8) | (minor & 0xff) | ((minor & !0xffu64) << 12) | ((major & !0xfffu64) << 32)
}

/// `fgetxattr(2)`: the value of the attribute `name` of the file of `fd`
/// into `buf`, and its length. A value longer than `buf` is `ERANGE`.
pub fn fgetxattr(fd: i64, name: &CBuf, buf: &mut [u8]) -> Sysres {
    unsafe { sys(SYS_FGETXATTR, [fd as u64, name.ptr(), buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0]) }
}

/// `fsetxattr(2)`: set the attribute `name` of the file of `fd` to
/// `value`. `flags` 0 makes or replaces it.
pub fn fsetxattr(fd: i64, name: &CBuf, value: &[u8], flags: u64) -> Sysres {
    unsafe { sys(SYS_FSETXATTR, [fd as u64, name.ptr(), value.as_ptr() as u64, value.len() as u64, flags, 0]) }
}

pub fn symlinkat(target: &CBuf, dirfd: i64, path: &CBuf) -> Sysres {
    unsafe { sys(SYS_SYMLINKAT, [target.ptr(), dirfd as u64, path.ptr(), 0, 0, 0]) }
}

pub fn linkat(olddirfd: i64, old: &CBuf, newdirfd: i64, new: &CBuf, flags: u64) -> Sysres {
    unsafe { sys(SYS_LINKAT, [olddirfd as u64, old.ptr(), newdirfd as u64, new.ptr(), flags, 0]) }
}

pub fn fchmod(fd: i64, mode: u64) -> Sysres {
    unsafe { sys(SYS_FCHMOD, [fd as u64, mode, 0, 0, 0, 0]) }
}

/// `AT_SYMLINK_NOFOLLOW` is accepted by the libc wrapper and **rejected by
/// the kernel** with `EINVAL` on Linux: there is no `lchmod`. A caller that
/// wants to avoid following a symlink opens it `O_PATH|O_NOFOLLOW` and uses
/// `fchmod` on the descriptor, or does not chmod a symlink at all, which is
/// what extraction does because a symlink's own mode is not used.
pub fn fchmodat(dirfd: i64, path: &CBuf, mode: u64, flags: u64) -> Sysres {
    unsafe { sys(SYS_FCHMODAT, [dirfd as u64, path.ptr(), mode, flags, 0, 0]) }
}

/// `X_OK`, the access mode the memfd probe asks about. The only mode that
/// the crate needs: the launch path probes only for executability.
pub const X_OK: u64 = 1;

/// `faccessat(2)`: whether the real ids may access `path` in mode `mode`.
///
/// The kernel's answer, not the mode bits': for root the bits may read
/// `0o644` while the check still succeeds, and for a non-owner they may read
/// `0o755` while it fails. Probe both: either alone gives a wrong answer
/// in one of those two cases.
pub fn faccessat(dirfd: i64, path: &CBuf, mode: u64, flags: u64) -> Sysres {
    unsafe { sys(SYS_FACCESSAT, [dirfd as u64, path.ptr(), mode, flags, 0, 0]) }
}

pub fn fchownat(dirfd: i64, path: &CBuf, uid: u32, gid: u32, flags: u64) -> Sysres {
    unsafe { sys(SYS_FCHOWNAT, [dirfd as u64, path.ptr(), uid as u64, gid as u64, flags, 0]) }
}

/// `newfstatat(2)` into the same `Stat` [`stat`] fills.
///
/// Pass `AT_SYMLINK_NOFOLLOW` to ask about a symlink rather than about what
/// it points at. Extraction always does: the question it asks is "what is
/// already at this name", and a symlink that answers for its target is the
/// defect that a layer can use to write outside the rootfs.
///
/// Where the kernel has no such call (`ENOSYS`: loongarch64 before Linux
/// 6.11), this calls `statx(2)` and remembers the answer.
pub fn fstatat(dirfd: i64, path: &CBuf, flags: u64) -> Result<Stat, Errno> {
    use core::sync::atomic::Ordering;
    if !statx::NO_FSTATAT.load(Ordering::Relaxed) {
        match fstatat_direct(dirfd, path, flags) {
            Err(ENOSYS) => statx::NO_FSTATAT.store(true, Ordering::Relaxed),
            other => return other,
        }
    }
    statx_at(dirfd, path, flags)
}

/// The stat call of this architecture, with no fallback.
pub(super) fn fstatat_direct(dirfd: i64, path: &CBuf, flags: u64) -> Result<Stat, Errno> {
    let mut raw = unsafe { core::mem::zeroed::<KernelStat>() };
    unsafe { sys(SYS_FSTATAT, [dirfd as u64, path.ptr(), &mut raw as *mut KernelStat as u64, flags, 0, 0])? };
    Ok(Stat::from_kernel(&raw))
}

/// `DT_*`, the file type `getdents64` reports without a `stat`.
/// `DT_UNKNOWN` is legal on any filesystem and is not "not a directory": a
/// caller that needs certainty stats.
pub const DT_UNKNOWN: u8 = 0;
pub const DT_DIR: u8 = 4;
pub const DT_LNK: u8 = 10;

/// One directory entry, as `getdents64(2)` reports it.
pub struct Dirent {
    pub name: Vec<u8>,
    pub d_type: u8,
}

/// Read a whole directory through a descriptor.
///
/// Through the descriptor, never by path. The tree being listed is one this
/// process is writing to, and a path is re-resolved by the kernel on every
/// call.
///
/// `.` and `..` are dropped here. Every caller wants the contents, and a
/// caller that removed what this returned without dropping them would try to
/// remove the directory it is standing in.
pub fn getdents64(fd: i64) -> Result<Vec<Dirent>, Errno> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        let n =
            unsafe { sys(SYS_GETDENTS64, [fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0])? } as usize;
        if n == 0 {
            break;
        }
        let mut off = 0usize;
        while off + 19 <= n {
            let reclen = u16::from_ne_bytes([buf[off + 16], buf[off + 17]]) as usize;
            if reclen == 0 || off + reclen > n {
                break;
            }
            let d_type = buf[off + 18];
            let name_bytes = &buf[off + 19..off + reclen];
            let end = name_bytes.iter().position(|b| *b == 0).unwrap_or(0);
            let name = name_bytes[..end].to_vec();
            if name != b"." && name != b".." {
                out.push(Dirent { name, d_type });
            }
            off += reclen;
        }
    }
    Ok(out)
}

/// `readlinkat(2)`. Returns the target as bytes, because a link target in an
/// image is not guaranteed to be UTF-8.
pub fn readlinkat(dirfd: i64, path: &CBuf) -> Result<Vec<u8>, Errno> {
    let mut buf = vec![0u8; 4096];
    let n = unsafe { sys(SYS_READLINKAT, [dirfd as u64, path.ptr(), buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0])? }
        as usize;
    // readlink(2) truncates silently and does not NUL-terminate. A target
    // that exactly filled the buffer may have been longer, so it is refused
    // rather than returned short.
    if n >= buf.len() {
        return Err(ENAMETOOLONG);
    }
    buf.truncate(n);
    Ok(buf)
}

/// Never returns. Used only in a probe child.
pub fn exit_group(code: i32) -> ! {
    loop {
        unsafe {
            syscall6(SYS_EXIT_GROUP, code as u64, 0, 0, 0, 0, 0);
        }
    }
}

pub fn pipe2(fds: &mut [i32; 2], flags: u64) -> Sysres {
    unsafe { sys(SYS_PIPE2, [fds.as_mut_ptr() as u64, flags, 0, 0, 0, 0]) }
}

/// `dup2(2)` by name, `dup3(2)` in fact.
///
/// **They differ in one case and it is handled by the caller, not here.**
/// `dup2(fd, fd)` is a no-op returning `fd`; `dup3(fd, fd, 0)` is `EINVAL`.
/// The one caller in this crate, `child.rs`'s verdict channel, already guards
/// `w != 1` before calling, because `dup2(1, 1)` followed by an unconditional
/// `close` was a real defect there. The guard is therefore load-bearing twice,
/// and the `debug_assert` says so where a future caller would otherwise
/// discover it as an `EINVAL` on one architecture only.
pub fn dup2(old: i64, new: i64) -> Sysres {
    debug_assert_ne!(old, new, "dup3 answers EINVAL where dup2 is a no-op");
    unsafe { sys(SYS_DUP3, [old as u64, new as u64, 0, 0, 0, 0]) }
}

/// `clone(flags, stack=0, ...)`, which is a fork when no `CLONE_VM` is set.
///
/// Never pass `CLONE_VM`. The child below this call runs on the parent's
/// stack under copy-on-write; sharing the address space instead would have two
/// processes on one stack.
///
/// The child sheds every fd registered with [`close_in_children`] before
/// this returns to it. That is here rather than in each caller's `Ok(0)` arm on
/// purpose: an image lock leaking into an unrelated child is
/// a defect, and a guard belongs where a caller who knows nothing about it
/// still passes through.
///
/// # Safety
/// The caller must do nothing in the child but async-signal-safe work on
/// buffers that already exist: no allocation, no `std` IO, no locks.
pub unsafe fn clone_fork(flags: u64) -> Sysres {
    let r = unsafe { sys(SYS_CLONE, [flags, 0, 0, 0, 0, 0]) };
    if matches!(r, Ok(0)) {
        shed_registered_fds();
    }
    r
}

/// # Safety
/// `argv` and `envp` must be NUL-terminated arrays of valid C string pointers.
pub unsafe fn execve(path: &CBuf, argv: *const *const u8, envp: *const *const u8) -> Sysres {
    unsafe { sys(SYS_EXECVE, [path.ptr(), argv as u64, envp as u64, 0, 0, 0]) }
}

/// Execute the file `dirfd` points at, without resolving a path.
///
/// `path` is the empty string and `flags` carries [`AT_EMPTY_PATH`]: the
/// kernel execs the descriptor itself, which is the memfd rung's whole
/// mechanism. Like [`execve`], it returns only on failure.
///
/// # Safety
/// `argv` and `envp` must be NUL-terminated arrays of valid C string pointers.
pub unsafe fn execveat(dirfd: i64, path: &CBuf, argv: *const *const u8, envp: *const *const u8, flags: u64) -> Sysres {
    unsafe { sys(SYS_EXECVEAT, [dirfd as u64, path.ptr(), argv as u64, envp as u64, flags, 0]) }
}
