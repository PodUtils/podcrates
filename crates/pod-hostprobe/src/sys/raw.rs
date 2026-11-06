use super::*;

/// ⛔ Refuse a symlink anywhere in the walk, including a trailing one. This is
/// what makes the check immune to a symlink the same layer created a moment
/// ago, which is the defect `TODO/extract.md` T-0304 exists for.
pub const RESOLVE_NO_SYMLINKS: u64 = 0x04;
/// ⛔ Refuse anything resolving outside the directory the descriptor names,
/// `..` and an absolute path included. The kernel enforces it; a lexical check
/// alone can be defeated by a component created between the check and the use.
pub const RESOLVE_BENEATH: u64 = 0x08;

/// `openat2`'s "treat the starting descriptor as `/`".
///
/// ⭐ Symlinks ARE followed and an absolute target is rebased onto that
/// descriptor, so nothing resolves outside it and `..` at the top stays at the
/// top. [`TODO/complete.md`](../../../TODO/complete.md) T-0405 turns on the
/// difference between this and [`RESOLVE_BENEATH`] `| ` [`RESOLVE_NO_SYMLINKS`]:
/// during extraction a symlink is the attack, and afterwards a rootfs's own
/// internal links are how its files are reached.
pub const RESOLVE_IN_ROOT: u64 = 0x10;

pub const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
pub const AT_REMOVEDIR: u64 = 0x200;
pub const AT_EMPTY_PATH: u64 = 0x1000;

pub const S_IFDIR: u32 = 0o040000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFLNK: u32 = 0o120000;

/// `flock(2)` operations. ⚠ `LOCK_NB` is not optional anywhere in this tree:
/// `RULES.md` section 8 forbids an unbounded wait, and a blocking `flock` on a
/// lock another process holds through an exec is exactly one.
pub const LOCK_SH: u64 = 1;
pub const LOCK_EX: u64 = 2;
pub const LOCK_NB: u64 = 4;
pub const LOCK_UN: u64 = 8;

/// `fcntl(2)`'s descriptor-flag commands, and the only flag that lives there.
///
/// ⭐ `FD_CLOEXEC` is a property of the DESCRIPTOR, not of the open file
/// description, which is why it can be cleared on one fd immediately before an
/// exec without affecting any other fd onto the same file.
/// [`TODO/image.md`](../../../TODO/image.md) T-0211 is that distinction: an fd
/// opened inheritable is inherited by every fork, and an fd made inheritable
/// one call before `execve` is inherited by exactly the payload.
pub const F_GETFD: u64 = 1;
pub const F_SETFD: u64 = 2;
pub const FD_CLOEXEC: u64 = 1;

/// `AT_FDCWD` is `-100`, and every syscall taking it wants it sign-extended.
pub const AT_FDCWD: u64 = -100i64 as u64;

pub const PR_SET_PDEATHSIG: u64 = 1;
pub const PR_SET_NO_NEW_PRIVS: u64 = 38;

pub const SECCOMP_SET_MODE_FILTER: u64 = 1;
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u64 = 1 << 3;

pub const FSCONFIG_CMD_CREATE: u64 = 6;
pub const OPEN_TREE_CLONE: u64 = 1;
pub const MOVE_MOUNT_F_EMPTY_PATH: u64 = 0x0000_0004;

// ---------------------------------------------------------------- the trap
//
// ⛔ The instruction and the register convention are per architecture, x86_64
// puts the number in rax and the arguments in rdi rsi rdx r10 r8 r9, aarch64
// puts them in x8 and x0..x5, and each has its own clobber set, and getting
// one wrong is not a build failure, it is a syscall with the arguments in the
// wrong places. `syscalls::raw` carries the correct sequence for every
// architecture it supports, which is why podvm no longer writes one.
//
// ⚠ The raw return value is still read the same way: `split` below turns the
// kernel's `-4095..=-1` window into an `Errno`. Nothing passes through libc's
// `errno` thread-local, which is `TODO/probe.md` T-0101's requirement and is
// the reason `syscalls::raw` is taken rather than its `Result`-returning API.

/// # Safety
/// The caller states that this syscall with these arguments is sound: any
/// pointer argument is valid for the kernel's access, and the effect on this
/// process is one the caller intends.
#[cfg(not(any(
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "s390x",
    target_arch = "powerpc",
    target_arch = "powerpc64",
)))]
pub unsafe fn syscall6(nr: i64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    unsafe {
        syscalls::raw::syscall6(
            nr as usize,
            a0 as usize,
            a1 as usize,
            a2 as usize,
            a3 as usize,
            a4 as usize,
            a5 as usize,
        ) as i64
    }
}

// ------------------------------------------- podvm's own trap, T-0912
//
// ⭐ **Two architectures, and both are RUN rather than only compiled.**
// `experiments/260-multiarch.sh` builds `podvm` for
// `powerpc64le-unknown-linux-musl` and `s390x-unknown-linux-musl` and executes
// it under `qemu-ppc64le-static` and `qemu-s390x-static`, so the register
// conventions below are measured and not asserted. ⛔ That distinction is the
// whole reason these two are here and `mips` is not: getting a convention wrong
// is not a build failure, it is a syscall with the arguments in the wrong
// places, and asm nobody has executed is a claim.

/// powerpc64, both endiannesses.
///
/// ⛔ **The error convention is NOT x86_64's and this is where that is
/// handled.** powerpc does not return `-errno` in `r3`: it returns the positive
/// errno there and sets **CR0.SO** to say the call failed. `split` upstream
/// reads the kernel's `-4095..=-1` window, so the value is negated here when SO
/// is set, and every caller then sees one convention.
///
/// r0 carries the number, r3 through r8 the six arguments, `sc` is the trap,
/// and r9 through r12, cr0, ctr and xer are clobbered.
#[cfg(target_arch = "powerpc64")]
pub unsafe fn syscall6(nr: i64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    let ret: u64;
    unsafe {
        core::arch::asm!(
            "sc",
            // ⚠ `bns` is "branch if not summary overflow", on cr0. A local
            // numeric label, because a named one would collide when this is
            // inlined twice into one function.
            "bns 2f",
            "neg 3, 3",
            "2:",
            inlateout("r0") nr as u64 => _,
            inlateout("r3") a0 => ret,
            in("r4") a1,
            in("r5") a2,
            in("r6") a3,
            in("r7") a4,
            in("r8") a5,
            lateout("r9") _,
            lateout("r10") _,
            lateout("r11") _,
            lateout("r12") _,
            options(nostack)
        );
    }
    ret as i64
}

/// s390x.
///
/// ⚠ r1 carries the number and r2 through r7 the six arguments; `svc 0` is the
/// trap and the result comes back in r2 as `-errno`, which is the convention
/// `split` already reads.
#[cfg(target_arch = "s390x")]
pub unsafe fn syscall6(nr: i64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    let ret: u64;
    unsafe {
        core::arch::asm!(
            "svc 0",
            in("r1") nr as u64,
            inlateout("r2") a0 => ret,
            in("r3") a1,
            in("r4") a2,
            in("r5") a3,
            in("r6") a4,
            in("r7") a5,
            options(nostack)
        );
    }
    ret as i64
}

/// ⛔ **mips and mips64 are still gated, and this says so at compile time
/// rather than shipping a convention nobody has run.**
///
/// T-0912's Approach names three families; two of them are here and executed
/// under an emulator by `experiments/260-multiarch.sh`. mips is not, and the
/// reason is not effort: `qemu-mips*-static` is installed on this host, but
/// o32 passes arguments five and six **on the caller's stack**, which
/// `options(nostack)` forbids and which a wrong frame layout gets wrong
/// silently. ⚠ What would clear it is the same thing that cleared these two: a
/// clause in `260-multiarch.sh` that builds for a mips target and RUNS the
/// binary under the emulator.
#[cfg(any(target_arch = "mips", target_arch = "mips64", target_arch = "powerpc"))]
compile_error!(
    "podvm has no measured syscall trap for this architecture. TODO/deps.md \
     T-0912: powerpc64 and s390x carry podvm's own, executed under an emulator; \
     mips, mips64 and 32-bit powerpc do not, and shipping a register convention \
     nobody has run is what that entry refuses."
);

/// Split the raw return into a value and an errno.
///
/// ⛔ The `-4095..=-1` window is the kernel's own convention. A return outside
/// it is a value, including a large negative one from a syscall that returns a
/// signed quantity.
pub fn split(ret: i64) -> Sysres {
    if (-4095..0).contains(&ret) {
        Err(Errno(-ret as i32))
    } else {
        Ok(ret)
    }
}

/// # Safety
/// As `syscall6`.
pub unsafe fn sys(nr: i64, args: [u64; 6]) -> Sysres {
    split(unsafe { syscall6(nr, args[0], args[1], args[2], args[3], args[4], args[5]) })
}

// -------------------------------------------------- fds that a fork must shed
//
// ⛔ `O_CLOEXEC` is close-on-EXEC and there is no close-on-FORK. A `fork`
// duplicates every descriptor, and `flock(2)` is held on the OPEN FILE
// DESCRIPTION the duplicates share, so the lock lives until the last of them is
// closed. A child that never execs, and this tree makes one per `clone`
// probe, therefore holds an image lock that `O_CLOEXEC` cannot take from it.
// [`TODO/image.md`](../../../TODO/image.md) T-0211.
//
// ⭐ The list is exact rather than a blanket close of everything above stderr:
// podvm knows which descriptors these are, and closing a range would also shut
// fds a caller handed podvm deliberately.

/// How many such fds may be registered at once. ⚠ Fixed, because the child
/// drains this between `clone` and `execve`, where allocation is not permitted.
/// podvm holds one image lock per container; sixteen is far past what any
/// single process does, and a seventeenth is refused by name rather than
/// dropped silently.
pub const FORK_CLOSE_SLOTS: usize = 16;

static FORK_CLOSE: [core::sync::atomic::AtomicI64; FORK_CLOSE_SLOTS] =
    [const { core::sync::atomic::AtomicI64::new(-1) }; FORK_CLOSE_SLOTS];

/// Register `fd` to be closed in every child [`clone_fork`] makes.
///
/// Returns false where every slot is taken, and the caller reports that rather
/// than proceeding: an unregistered lock fd is the defect, not a detail.
pub fn close_in_children(fd: i64) -> bool {
    use core::sync::atomic::Ordering;
    for slot in &FORK_CLOSE {
        if slot
            .compare_exchange(-1, fd, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return true;
        }
    }
    false
}

/// Stop shedding `fd`, either because it is being closed or because it is about
/// to be handed to a payload on purpose.
pub fn stop_closing_in_children(fd: i64) {
    use core::sync::atomic::Ordering;
    for slot in &FORK_CLOSE {
        let _ = slot.compare_exchange(fd, -1, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// Close every registered fd. ⛔ Called in the child, by [`clone_fork`] and by
/// [`shed_after_fork`], and nowhere else.
///
/// ⚠ Async-signal-safe: an atomic load and a `close(2)` per slot, no allocation
/// and no lock.
pub(super) fn shed_registered_fds() {
    use core::sync::atomic::Ordering;
    for slot in &FORK_CLOSE {
        let fd = slot.load(Ordering::Acquire);
        if fd >= 0 {
            let _ = close(fd);
        }
    }
}

/// ⛔ **The second drain, and `std::process::Command` is why it exists.**
/// [`clone_fork`] is podvm's own fork and it sheds before it returns in the
/// child. libstd forks inside `Command::spawn`, never passes through
/// [`clone_fork`], and the shed list therefore cannot reach that child.
///
/// ⚠ **`O_CLOEXEC` does not cover the gap, because it acts at the `execve` and
/// not at the `fork`.** Between the two the child holds a duplicate of every
/// open file description the parent had, and `flock(2)` is held on the
/// description. So a lock ANY OTHER THREAD releases inside that window stays
/// held by the child's copy until the child execs, and `in_use` reads an image
/// as in use with no holder anywhere.
/// [`TODO/image.md`](../../../TODO/image.md) T-0215.
///
/// ⭐ The hook runs in the child after the `fork` and before the `execve`, so
/// it shortens the window to the instructions between those two points rather
/// than removing it. ⚠ It cannot be removed: the duplication happens in the
/// kernel at the `fork`, and there is no close-on-fork.
///
/// # Safety
/// `pre_exec` runs between `fork` and `execve` in a process that may hold locks
/// another thread owns, so the hook has to be async-signal-safe.
/// [`shed_registered_fds`] is atomic loads and `close(2)`, which is.
pub fn shed_after_fork(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            shed_registered_fds();
            Ok(())
        })
    }
}

// ---------------------------------------------------------------- wrappers
//
// Each takes what the kernel takes and returns the kernel's answer. Nothing
// here interprets a verdict; that is `probes.rs`'s job.

/// A NUL-terminated byte buffer whose pointer is stable for as long as it is
/// held. Built before any fork, so nothing after a fork allocates.
pub struct CBuf(Vec<u8>);

impl CBuf {
    /// Returns `None` when `s` contains an interior NUL, because a path the
    /// kernel would silently truncate is not the path the caller named.
    pub fn new(s: &str) -> Option<CBuf> {
        if s.as_bytes().contains(&0) {
            return None;
        }
        let mut v = Vec::with_capacity(s.len() + 1);
        v.extend_from_slice(s.as_bytes());
        v.push(0);
        Some(CBuf(v))
    }

    pub fn ptr(&self) -> u64 {
        self.0.as_ptr() as u64
    }
}

/// The empty C string, for syscalls taking a path they must not resolve.
pub fn cempty() -> CBuf {
    CBuf(vec![0])
}

pub fn close(fd: i64) -> Sysres {
    unsafe { sys(SYS_CLOSE, [fd as u64, 0, 0, 0, 0, 0]) }
}

/// `fcntl(2)`, for the commands whose argument is an integer.
///
/// ⚠ Restricted to `F_GETFD`, `F_SETFD` and `F_DUPFD_CLOEXEC` by its own
/// callers rather than by its signature: the commands taking a pointer argument
/// (`F_GETLK`, `F_SETLK`, `F_GETOWN_EX`) need a struct this takes no room for,
/// and calling one through here would hand the kernel an integer where it reads
/// an address.
pub fn fcntl(fd: i64, cmd: u64, arg: u64) -> Sysres {
    unsafe { sys(SYS_FCNTL, [fd as u64, cmd, arg, 0, 0, 0]) }
}

/// `F_DUPFD_CLOEXEC`, which is `F_LINUX_SPECIFIC_BASE + 6`.
pub const F_DUPFD_CLOEXEC: u64 = 1030;

/// Seal commands for a memfd, from `linux/fcntl.h`. Arch-independent: `F_ADD_SEALS`
/// is `F_LINUX_SPECIFIC_BASE + 9` and the seals are single bits. The ladder seals
/// a written memfd with all four where the kernel accepts them; a kernel without
/// sealing answers `EINVAL` and the payload runs unsealed rather than not at all.
pub const F_ADD_SEALS: u64 = 1033;
pub const F_SEAL_SEAL: u64 = 0x0001;
pub const F_SEAL_SHRINK: u64 = 0x0002;
pub const F_SEAL_GROW: u64 = 0x0004;
pub const F_SEAL_WRITE: u64 = 0x0008;

/// A second descriptor for the same open file description, at an unused number.
///
/// ⛔ **Why this and not [`dup2`].** A caller that wants the child's stdout to
/// be its stderr cannot pass `(1, 2)` to a `dup2`-then-`close` loop: the close
/// would take the child's stderr with it. It dups first, hands over the
/// duplicate, and the loop closes the duplicate instead.
///
/// ⚠ `O_CLOEXEC` on the duplicate is deliberate and is not inherited by the
/// `dup2` that installs it: a duplicate the caller forgot would otherwise reach
/// the payload, and a payload holding an extra descriptor onto podvm's own
/// stderr is a difference from docker nobody asked for.
pub fn dup_cloexec(fd: i64) -> Sysres {
    // ⚠ From 3 upwards: below that are the standard descriptors, and a
    // duplicate landing on one of them is the bug this function exists to
    // avoid.
    fcntl(fd, F_DUPFD_CLOEXEC, 3)
}
