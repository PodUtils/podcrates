//! Raw Linux syscalls, and the only module in this crate that is allowed
//! `unsafe`.
//!
//! `TOOL.md` section 3.3 requires `unsafe` be confined to one module per
//! subsystem with the safe wrapper adjacent. Everything below the `syscall*`
//! functions is safe and returns `Result<i64, Errno>`.
//!
//! ⛔ No libc wrapper is used, and that is the point rather than an economy.
//! `TODO/deps.md` T-0901 rules the probe set is hand-declared and measured
//! before a crate is considered. Beyond that, a libc wrapper is the wrong
//! instrument here: glibc's `setuid(3)` runs a multi-threaded id-change dance
//! and musl's runs another, so the errno a wrapper returns is the library's
//! answer and not the kernel's. `TODO/probe.md` T-0101 requires the kernel's.
//!
//! ⚠ The raw return value carries the errno directly: a value in
//! `-4095..=-1` is `-errno`. Nothing passes through `errno`'s thread-local,
//! so there is no window in which another call overwrites it.

//! ---------------------------------------------------------------------------
//!
//! ⭐ **Every syscall number and every kernel struct layout below comes from a
//! crate, per architecture, and none of them is written here.**
//! [`TODO/deps.md`](../../../TODO/deps.md) T-0911 is the ruling and the
//! measurement. Until 2026-09-09 this module declared x86_64's numbers by hand
//! and refused to compile anywhere else, which made podvm a single-
//! architecture runtime for no reason but the table.
//!
//! - [`syscalls`] carries the kernel's own table for fourteen architectures and
//!   the `syscall` instruction for each. `nr!(openat, __NR_openat)` is that table,
//!   so an architecture podvm has never been built on gets its numbers from
//!   the kernel's source rather than from a session's transcription.
//! - [`linux_raw_sys`] carries `struct stat` and `struct statfs64` generated
//!   from the kernel headers. ⚠ Their **shape differs by architecture**,
//!   `stat` is 144 bytes on x86_64 and 128 on aarch64, which is precisely the
//!   class of thing a hand-written `#[repr(C)]` gets wrong silently, by handing
//!   the kernel a buffer shorter than what it writes.
//!
//! ⛔ **The transcription risk did not go away, it moved to one place and is
//! asserted.** A name that does not exist for an architecture is a compile
//! error, never a wrong number: `Sysno` has no variant for a syscall the kernel
//! does not define there. The `#[cfg]` list below names the architectures whose
//! kernels use `asm-generic/unistd.h`, which omits `open`, `stat`, `mkdir`,
//! `unlink`, `chown`, `lchown`, `readlink`, `mknod` and `dup2` in favour of the
//! `*at` forms, get that list wrong in either direction and the build fails
//! rather than measuring something nobody named.

/// A kernel error number. Never a boolean: `TODO/probe.md` T-0101 rules that
/// `EINVAL` from `setuid` or `chown` means an unmapped id and points at a
/// mapping, while `EPERM` points at a policy, and they are different remedies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Errno(pub i32);

impl Errno {
    /// The symbolic name, or `E<n>` where this table does not carry one.
    ///
    /// ⚠ `E<n>` is a statement that the number was measured and the name is
    /// not known here, not a guess at which errno it was.
    pub fn name(self) -> String {
        let n = match self.0 {
            1 => "EPERM",
            2 => "ENOENT",
            3 => "ESRCH",
            4 => "EINTR",
            5 => "EIO",
            9 => "EBADF",
            11 => "EAGAIN",
            12 => "ENOMEM",
            13 => "EACCES",
            14 => "EFAULT",
            16 => "EBUSY",
            17 => "EEXIST",
            18 => "EXDEV",
            19 => "ENODEV",
            20 => "ENOTDIR",
            21 => "EISDIR",
            22 => "EINVAL",
            24 => "EMFILE",
            26 => "ETXTBSY",
            28 => "ENOSPC",
            30 => "EROFS",
            36 => "ENAMETOOLONG",
            38 => "ENOSYS",
            39 => "ENOTEMPTY",
            40 => "ELOOP",
            95 => "EOPNOTSUPP",
            _ => return format!("E{}", self.0),
        };
        n.to_string()
    }
}

pub const EPERM: Errno = Errno(1);
pub const ENOENT: Errno = Errno(2);
pub const ESRCH: Errno = Errno(3);
pub const EINTR: Errno = Errno(4);
pub const EBADF: Errno = Errno(9);
pub const EACCES: Errno = Errno(13);
pub const EEXIST: Errno = Errno(17);
/// `EXDEV`, cross-device link: the number the (d,i) guard reports under,
/// because a chroot that landed off its descriptor is on the wrong device
/// or the wrong file. TODO/enter.md T-1339.
pub const EXDEV: Errno = Errno(18);
pub const EINVAL: Errno = Errno(22);
pub const ENOSYS: Errno = Errno(38);
pub const ENAMETOOLONG: Errno = Errno(36);
/// ⚠ On Linux `EAGAIN` and `EWOULDBLOCK` are the same number, and `flock` with
/// `LOCK_NB` returns it for "somebody else holds this". It is named here for
/// the reader, because "try again later" and "in use" are different sentences.
pub const EWOULDBLOCK: Errno = Errno(11);

pub type Sysres = Result<i64, Errno>;

// ---------------------------------------------------------------- syscall nrs
//
// ⛔ Not one number is written here. `Sysno::<name> as i64` is the kernel's own
// table for the architecture being built, carried by the `syscalls` crate.

// ---------------------------------------------------- where a number comes from
//
// ⭐ **T-0912. TWO SOURCES, ONE PER LINE, AND THE ARCHITECTURE PICKS.**
//
// `syscalls` carries the kernel's table for fourteen architectures, and until
// 2026-09-09 podvm took every number from it. ⛔ It also carries
// `#![feature(asm_experimental_arch)]` for `mips`, `mips64`, `s390x`,
// `powerpc` and `powerpc64`, which is a **crate-level attribute**: the whole
// crate refuses to compile for those targets on stable, whatever podvm does.
// That is why `powerpc64le-unknown-linux-musl` was the one architecture this
// workspace could not build for.
//
// ⭐ **The gate is the crate's and not the compiler's, and that was measured
// rather than assumed.** `experiments/260-multiarch.sh` clause 3 compiles a
// `powerpc64` `sc` sequence against the real rustc:
//
//     ⚠ powerpc64 inline asm COMPILES on rustc 1.98.1
//
// So on exactly the five architectures the crate gates, podvm takes the
// numbers from `linux-raw-sys` -- which it already depends on for the kernel
// structs -- and carries its own trap. ⛔ Not a fork of `syscalls` and not a
// patch to it in this tree: the numbers are already available from a crate
// podvm takes, and one more source of them is one more place to disagree.
//
// ⚠ **Both spellings are on the same line**, `Sysno::openat` and
// `__NR_openat`, so a reader can check the pair and a rename cannot silently
// take a different number on one architecture.

/// ⛔ The five the `syscalls` crate gates behind nightly. Written out at each
/// use rather than aliased, because Rust has no `cfg` alias without a
/// `build.rs`, and a `build.rs` to save three repetitions is a worse trade.
#[cfg(not(any(
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "s390x",
    target_arch = "powerpc",
    target_arch = "powerpc64",
)))]
use syscalls::Sysno;

#[cfg(not(any(
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "s390x",
    target_arch = "powerpc",
    target_arch = "powerpc64",
)))]
macro_rules! nr {
    ($sysno:ident, $raw:ident) => {
        crate::sys::Sysno::$sysno as i64
    };
}

#[cfg(any(
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "s390x",
    target_arch = "powerpc",
    target_arch = "powerpc64",
))]
macro_rules! nr {
    ($sysno:ident, $raw:ident) => {
        linux_raw_sys::general::$raw as i64
    };
}

pub const SYS_READ: i64 = nr!(read, __NR_read);
pub const SYS_WRITE: i64 = nr!(write, __NR_write);
pub const SYS_CLOSE: i64 = nr!(close, __NR_close);
pub const SYS_GETPID: i64 = nr!(getpid, __NR_getpid);
pub const SYS_CLONE: i64 = nr!(clone, __NR_clone);
pub const SYS_EXECVE: i64 = nr!(execve, __NR_execve);
/// ⭐ The fd-exec number, and why it is `execveat` and not `fexecve`.
/// Neither number crate names `fexecve` (`syscalls` 0.8.1 and `linux-raw-sys`
/// 0.12.1 were read and neither spells it), so the `nr!` pattern cannot take
/// it. `execveat` with an empty path and `AT_EMPTY_PATH` is the same kernel
/// operation -- it is how libc implements `fexecve` -- and both crates name
/// it on every architecture, so the ladder execs a memfd through this.
pub const SYS_EXECVEAT: i64 = nr!(execveat, __NR_execveat);
pub const SYS_WAIT4: i64 = nr!(wait4, __NR_wait4);
pub const SYS_KILL: i64 = nr!(kill, __NR_kill);
pub const SYS_PTRACE: i64 = nr!(ptrace, __NR_ptrace);
pub const SYS_GETUID: i64 = nr!(getuid, __NR_getuid);
pub const SYS_GETGID: i64 = nr!(getgid, __NR_getgid);
pub const SYS_SETUID: i64 = nr!(setuid, __NR_setuid);
pub const SYS_SETGID: i64 = nr!(setgid, __NR_setgid);
pub const SYS_SETSID: i64 = nr!(setsid, __NR_setsid);
pub const SYS_GETGROUPS: i64 = nr!(getgroups, __NR_getgroups);
pub const SYS_SETGROUPS: i64 = nr!(setgroups, __NR_setgroups);
pub const SYS_STATFS: i64 = nr!(statfs, __NR_statfs);
pub const SYS_FLOCK: i64 = nr!(flock, __NR_flock);
pub const SYS_FCNTL: i64 = nr!(fcntl, __NR_fcntl);
pub const SYS_PIVOT_ROOT: i64 = nr!(pivot_root, __NR_pivot_root);
pub const SYS_PRCTL: i64 = nr!(prctl, __NR_prctl);
pub const SYS_CHROOT: i64 = nr!(chroot, __NR_chroot);
pub const SYS_MOUNT: i64 = nr!(mount, __NR_mount);
pub const SYS_UMOUNT2: i64 = nr!(umount2, __NR_umount2);
pub const SYS_SETHOSTNAME: i64 = nr!(sethostname, __NR_sethostname);
pub const SYS_EXIT_GROUP: i64 = nr!(exit_group, __NR_exit_group);
pub const SYS_UNSHARE: i64 = nr!(unshare, __NR_unshare);
pub const SYS_OPENAT: i64 = nr!(openat, __NR_openat);
pub const SYS_PIPE2: i64 = nr!(pipe2, __NR_pipe2);
pub const SYS_PROCESS_VM_READV: i64 = nr!(process_vm_readv, __NR_process_vm_readv);
pub const SYS_KCMP: i64 = nr!(kcmp, __NR_kcmp);
pub const SYS_SECCOMP: i64 = nr!(seccomp, __NR_seccomp);
pub const SYS_GETRANDOM: i64 = nr!(getrandom, __NR_getrandom);
pub const SYS_MEMFD_CREATE: i64 = nr!(memfd_create, __NR_memfd_create);
pub const SYS_OPEN_TREE: i64 = nr!(open_tree, __NR_open_tree);
pub const SYS_MOVE_MOUNT: i64 = nr!(move_mount, __NR_move_mount);
pub const SYS_FSOPEN: i64 = nr!(fsopen, __NR_fsopen);
pub const SYS_FSCONFIG: i64 = nr!(fsconfig, __NR_fsconfig);
pub const SYS_FSMOUNT: i64 = nr!(fsmount, __NR_fsmount);
pub const SYS_PIDFD_GETFD: i64 = nr!(pidfd_getfd, __NR_pidfd_getfd);
// ⭐ M4's supervisor. TODO/supervise.md T-0601 and T-0602: a pidfd per direct
// child, `waitid` for its status, and `ppoll` to wait on the CONDITION rather
// than on a guessed duration.
// ⚠ `ppoll` and not `poll`: `poll` does not exist on aarch64 or riscv64, and
// this workspace compiles for six architectures (TODO/deps.md T-0911).
pub const SYS_PIDFD_OPEN: i64 = nr!(pidfd_open, __NR_pidfd_open);
pub const SYS_WAITID: i64 = nr!(waitid, __NR_waitid);
pub const SYS_PPOLL: i64 = nr!(ppoll, __NR_ppoll);
// ⭐ The shutdown-forwarding pair. TODO/supervise.md T-1335: a waiter that
// forwards its own SIGINT/SIGTERM to the payload blocks them first and reads
// them from a signalfd, so the signal never kills the waiter between the
// decision and the forward. `rt_sigprocmask` and not `sigprocmask`: the raw
// call takes the set size, and the libc wrapper's is not the kernel's.
pub const SYS_RT_SIGPROCMASK: i64 = nr!(rt_sigprocmask, __NR_rt_sigprocmask);
pub const SYS_SIGNALFD4: i64 = nr!(signalfd4, __NR_signalfd4);
pub const SYS_LANDLOCK_CREATE_RULESET: i64 =
    nr!(landlock_create_ruleset, __NR_landlock_create_ruleset);
pub const SYS_DUP3: i64 = nr!(dup3, __NR_dup3);
// ⭐ M3's entry sequence. TODO/enter.md T-0504: the rootfs is held as a
// directory DESCRIPTOR and entered with `fchdir` then `chroot(".")`, so nothing
// between checking the path and changing the root can swap it.
pub const SYS_CHDIR: i64 = nr!(chdir, __NR_chdir);
pub const SYS_FCHDIR: i64 = nr!(fchdir, __NR_fchdir);

// ⭐ The `*at` family, taken by `crates/podvm-extract` (TODO/extract.md
// T-0304). Extraction resolves every entry against a directory FILE
// DESCRIPTOR rather than against a path, because a path is re-resolved by the
// kernel on every call and the tree is being written to between calls.
//
// ⛔ Since 2026-09-09 they are also what every path-taking wrapper in this
// module uses, on EVERY architecture and not only on the ones that have
// nothing else. `open(path)` is `openat(AT_FDCWD, path)` by definition, so
// routing through the `*at` form costs nothing and removes nine per-
// architecture cases. The three call sites where the entry point IS the
// measurement keep their own identity below.
pub const SYS_FCHMOD: i64 = nr!(fchmod, __NR_fchmod);
pub const SYS_GETDENTS64: i64 = nr!(getdents64, __NR_getdents64);
pub const SYS_MKDIRAT: i64 = nr!(mkdirat, __NR_mkdirat);
pub const SYS_FCHOWNAT: i64 = nr!(fchownat, __NR_fchownat);
pub const SYS_UNLINKAT: i64 = nr!(unlinkat, __NR_unlinkat);
#[cfg(not(target_arch = "riscv64"))]
pub const SYS_RENAMEAT: i64 = nr!(renameat, __NR_renameat);
/// `riscv64` has no `renameat`: asm-generic removed it, and `renameat2` with
/// flags 0 is the same operation (renameat2(2)). `syscalls` 0.8.1 has no such
/// variant in its riscv64 table, so this is the one number taken by its newer
/// spelling; the wrapper below already passes zero in the flags slot.
/// Measured 2026-09-23: six archs link and riscv64 fails to compile until
/// this line lands (TODO/packaging.md T-1314).
#[cfg(target_arch = "riscv64")]
pub const SYS_RENAMEAT: i64 = nr!(renameat2, __NR_renameat2);
pub const SYS_LINKAT: i64 = nr!(linkat, __NR_linkat);
pub const SYS_SYMLINKAT: i64 = nr!(symlinkat, __NR_symlinkat);
pub const SYS_READLINKAT: i64 = nr!(readlinkat, __NR_readlinkat);
pub const SYS_FCHMODAT: i64 = nr!(fchmodat, __NR_fchmodat);
pub const SYS_FACCESSAT: i64 = nr!(faccessat, __NR_faccessat);
pub const SYS_UTIMENSAT: i64 = nr!(utimensat, __NR_utimensat);
pub const SYS_MKNODAT: i64 = nr!(mknodat, __NR_mknodat);
/// ⚠ Linux 5.6. A kernel without it answers `ENOSYS`, which is why
/// `podvm-extract` carries an `O_NOFOLLOW` walk beside it rather than
/// requiring it.
pub const SYS_OPENAT2: i64 = nr!(openat2, __NR_openat2);

// -------------------------------------------- resource limits
//
// `prlimit64` and not `getrlimit`: the latter has no number on architectures
// whose kernel only carries the former, while `prlimit64` answers on every
// Linux architecture. `TODO/machine.md` T-1301 leg 3 reads `RLIMIT_FSIZE`
// because it bounds the memory image, and a guest over it dies with
// `SIGXFSZ`.
pub const SYS_PRLIMIT64: i64 = nr!(prlimit64, __NR_prlimit64);
/// `RLIMIT_FSIZE`, the maximum size of a file the process may create. 1 in
/// asm-generic `resource.h`, on every Linux architecture, measured against
/// the kernel's own headers. `RLIMIT_NOFILE` is 7: reading 7 here once
/// reported the descriptor ceiling as a file size.
pub const RLIMIT_FSIZE: u64 = 1;

mod arch;
pub use arch::*;

// ---------------------------------------------------------------- constants
pub const CLONE_NEWNS: u64 = 0x0002_0000;
pub const CLONE_NEWUTS: u64 = 0x0400_0000;
pub const CLONE_NEWUSER: u64 = 0x1000_0000;
pub const CLONE_NEWPID: u64 = 0x2000_0000;
pub const SIGCHLD: u64 = 17;
/// The two shutdown signals TODO/supervise.md T-1335 forwards, as numbers:
/// 2 is SIGINT (Ctrl-C on a terminal) and 15 is SIGTERM.
pub const SIGINT: u64 = 2;
pub const SIGTERM: u64 = 15;
/// The signalfd/`sigprocmask` word: exactly those two signal numbers' bit
/// indices, named once so the mask blocked and the descriptor read can never
/// disagree about the set. ⛔ Signal NUMBER n is bit index (n-1): bit 1 is
/// SIGHUP, bit 2+1 would be SIGINT's neighbour. `(1 << (SIGINT - 1)) |
/// (1 << (SIGTERM - 1))` names SIGINT (bit 1, value 2) and SIGTERM (bit 14,
/// value 16384); `(1 << SIGINT) | (1 << SIGTERM)` names bits 2 and 15
/// (SIGQUIT and SIGSTKFLT), which would block nothing that arrives and
/// leave the waiter dying on the real signals.
pub const SHUTDOWN_SIGNALS: u64 = (1 << (SIGINT - 1)) | (1 << (SIGTERM - 1));
/// `rt_sigprocmask` "block these", "set to this", and `signalfd4` "close on exec".
pub const SIG_BLOCK: u64 = 0;
pub const SIG_SETMASK: u64 = 2;
pub const SFD_CLOEXEC: u64 = 0x080000;

pub const MS_REC: u64 = 0x4000;
pub const MS_SLAVE: u64 = 0x0008_0000;
/// `MS_PRIVATE`, read at the kernel's own headers on 2026-09-26
/// (`/usr/include/sys/mount.h`: `MS_PRIVATE = 1 << 18`). The namespace
/// rung remounts `/` recursively private before mounting anything, so
/// its tmpfs cannot propagate to the host (TODO/enter.md T-1339).
pub const MS_PRIVATE: u64 = 0x0004_0000;

pub const S_IFMT: u32 = 0o170000;
pub const S_IFCHR: u64 = 0o0020000;
/// `makedev(1, 3)`, that is `/dev/null`. A real device number, unlike
/// `makedev(0, 0)`, which is `WHITEOUT_DEV` and is exempt from the capability
/// check. `TODO/probe.md` T-0106 is that pair.
pub const DEV_1_3: u64 = 0x103;

pub const O_RDONLY: u64 = 0;
pub const O_WRONLY: u64 = 1;
pub const O_RDWR: u64 = 2;
pub const O_CREAT: u64 = 0o100;
/// ⛔ Every scratch file a probe makes is created with this. A probe that
/// overwrites whatever is in its way has destroyed data to measure a
/// permission, and the errno it then reports is about its own file.
pub const O_EXCL: u64 = 0o200;
/// ⛔ Every probe that opens a terminal-shaped device passes this. Without it
/// an `open` of a tty can make it the prober's controlling terminal, which is
/// a mutation of the process doing the measuring.
pub const O_NOCTTY: u64 = 0o400;
pub const O_DIRECTORY: u64 = 0o200000;
pub const O_CLOEXEC: u64 = 0o2000000;
pub const O_TRUNC: u64 = 0o1000;
/// ⛔ The one flag that makes an `open` of a path component a statement about
/// that component rather than about wherever a symlink pointed. Every directory
/// descent in `crates/podvm-extract` carries it.
pub const O_NOFOLLOW: u64 = 0o400000;
pub const O_PATH: u64 = 0o10000000;

/// `openat2(2)`'s `struct open_how`. Three `u64`s, in this order, and the
/// kernel is told the size so it can reject a struct it does not know.
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct OpenHow {
    pub flags: u64,
    pub mode: u64,
    pub resolve: u64,
}

mod raw;
pub use raw::*;

mod fs;
pub use fs::*;

mod proc;
pub use proc::*;

#[cfg(test)]
mod tests;
