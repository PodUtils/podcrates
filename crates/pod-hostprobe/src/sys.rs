//! Raw Linux syscalls. The `unsafe` of the crate is here and in the probe
//! bodies, each beside its safe wrapper. Each function below the `syscall*`
//! functions is safe and returns `Result<i64, Errno>`.
//!
//! No libc wrapper is used. glibc's `setuid(3)` runs a multi-threaded id
//! change and musl's runs another, so the errno of a wrapper is the answer
//! of the library and not of the kernel. A probe needs the errno of the
//! kernel.
//!
//! The raw return value carries the errno directly: a value in
//! `-4095..=-1` is `-errno`. Nothing goes through the thread-local `errno`,
//! so no other call can overwrite it.
//!
//! ---------------------------------------------------------------------------
//!
//! **Each syscall number and each layout of a kernel struct comes from a
//! crate, for each architecture. None of them is written here.**
//!
//! - [`syscalls`] carries the table of the kernel for fourteen architectures
//!   and the `syscall` instruction for each. `nr!(openat, __NR_openat)` is
//!   that table, so each architecture gets its numbers from the source of
//!   the kernel.
//! - [`linux_raw_sys`] carries `struct stat` and `struct statfs64` generated
//!   from the kernel headers. Their **shape differs by architecture**,
//!   `stat` is 144 bytes on x86_64 and 128 on aarch64, which is precisely the
//!   class of thing a hand-written `#[repr(C)]` gets wrong silently, by handing
//!   the kernel a buffer shorter than what it writes.
//!
//! **The risk of a wrong number is in one place, and the compiler checks
//! it.** A name that does not exist for an architecture is a compile
//! error, never a wrong number: `Sysno` has no variant for a syscall the kernel
//! does not define there. The `#[cfg]` list below names the architectures whose
//! kernels use `asm-generic/unistd.h`, which omits `open`, `stat`, `mkdir`,
//! `unlink`, `chown`, `lchown`, `readlink`, `mknod` and `dup2` in favour of the
//! `*at` forms, get that list wrong in either direction and the build fails
//! rather than measuring something nobody named.

/// A kernel error number. Never a boolean: `EINVAL` from `setuid` or `chown` means an unmapped id and points at a
/// mapping, while `EPERM` points at a policy, and they are different remedies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Errno(pub i32);

impl Errno {
    /// The symbolic name, or `E<n>` where this table does not carry one.
    ///
    /// `E<n>` is a statement that the number was measured and the name is
    /// not known here, not a guess at which errno it was.
    pub fn name(self) -> String {
        let n = match self.0 {
            1 => "EPERM",
            2 => "ENOENT",
            3 => "ESRCH",
            4 => "EINTR",
            5 => "EIO",
            7 => "E2BIG",
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
/// `ECHILD`: `wait4` has no child to wait for, alive or ended.
pub const ECHILD: Errno = Errno(10);
pub const EINTR: Errno = Errno(4);
pub const EIO: Errno = Errno(5);
/// `E2BIG`: `openat2(2)` gives it for a `struct open_how` that the kernel
/// does not know.
pub const E2BIG: Errno = Errno(7);
pub const EBADF: Errno = Errno(9);
pub const EACCES: Errno = Errno(13);
pub const EEXIST: Errno = Errno(17);
/// `EXDEV`, cross-device link: the number the (d,i) guard reports under,
/// because a chroot that landed off its descriptor is on the wrong device
/// or the wrong file.
pub const EXDEV: Errno = Errno(18);
pub const ENOTDIR: Errno = Errno(20);
pub const EISDIR: Errno = Errno(21);
pub const EINVAL: Errno = Errno(22);
pub const ENOSYS: Errno = Errno(38);
pub const ENAMETOOLONG: Errno = Errno(36);
/// `ELOOP`: the refusal of `O_NOFOLLOW` and of `RESOLVE_NO_SYMLINKS`.
pub const ELOOP: Errno = Errno(40);
/// On Linux `EAGAIN` and `EWOULDBLOCK` are the same number, and `flock` with
/// `LOCK_NB` returns it for "somebody else holds this". It is named here for
/// the reader, because "try again later" and "in use" are different sentences.
pub const EWOULDBLOCK: Errno = Errno(11);

pub type Sysres = Result<i64, Errno>;

// ---------------------------------------------------------------- syscall nrs
//
// Not one number is written here. `Sysno::<name> as i64` is the kernel's own
// table for the architecture being built, carried by the `syscalls` crate.

// ---------------------------------------------------- where a number comes from
//
// **Two sources, one on each line, and the architecture selects.**
//
// `syscalls` carries the table of the kernel for fourteen architectures. It
// also carries `#![feature(asm_experimental_arch)]` for `mips`, `mips64`,
// `s390x`, `powerpc` and `powerpc64`. That is a **crate-level attribute**:
// the crate does not compile for those targets on stable Rust.
//
// The gate is of the crate, not of the compiler: stable rustc compiles the
// `powerpc64` `sc` sequence. So on the five architectures that the crate
// gates, this module takes the numbers from `linux-raw-sys` (a dependency
// for the kernel structs already) and has its own trap. It is not a fork of
// `syscalls`: one more source of the numbers is one more place to disagree.
//
// **Both spellings are on the same line**, `Sysno::openat` and
// `__NR_openat`, so a reader can check the pair and a rename cannot silently
// take a different number on one architecture.

/// The five the `syscalls` crate gates behind nightly. Written out at each
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
pub const SYS_GETPPID: i64 = nr!(getppid, __NR_getppid);
pub const SYS_CLONE: i64 = nr!(clone, __NR_clone);
pub const SYS_EXECVE: i64 = nr!(execve, __NR_execve);
/// The fd-exec number, and why it is `execveat` and not `fexecve`.
/// Neither number crate names `fexecve` (`syscalls` 0.8.1 and `linux-raw-sys`
/// 0.12.1 were read and neither spells it), so the `nr!` pattern cannot take
/// it. `execveat` with an empty path and `AT_EMPTY_PATH` is the same kernel
/// operation -- it is how libc implements `fexecve` -- and both crates name
/// it on every architecture, so the ladder execs a memfd through this.
pub const SYS_EXECVEAT: i64 = nr!(execveat, __NR_execveat);
pub const SYS_WAIT4: i64 = nr!(wait4, __NR_wait4);
pub const SYS_KILL: i64 = nr!(kill, __NR_kill);
pub const SYS_PTRACE: i64 = nr!(ptrace, __NR_ptrace);
pub const SYS_SETSID: i64 = nr!(setsid, __NR_setsid);
pub const SYS_GETRUSAGE: i64 = nr!(getrusage, __NR_getrusage);
pub const SYS_UMASK: i64 = nr!(umask, __NR_umask);

// The id calls. On x86 and arm the plain `getuid` family is the 16-bit
// form: an id above 65535 reads as 65534. The 32-bit kernel has the `*32`
// form of each. Every other target has only the 32-bit form under the
// plain name.
#[cfg(any(target_arch = "x86", target_arch = "arm"))]
mod id_calls {
    pub const SYS_GETUID: i64 = nr!(getuid32, __NR_getuid32);
    pub const SYS_GETGID: i64 = nr!(getgid32, __NR_getgid32);
    pub const SYS_SETUID: i64 = nr!(setuid32, __NR_setuid32);
    pub const SYS_SETGID: i64 = nr!(setgid32, __NR_setgid32);
    pub const SYS_GETGROUPS: i64 = nr!(getgroups32, __NR_getgroups32);
    pub const SYS_SETGROUPS: i64 = nr!(setgroups32, __NR_setgroups32);
}

#[cfg(not(any(target_arch = "x86", target_arch = "arm")))]
mod id_calls {
    pub const SYS_GETUID: i64 = nr!(getuid, __NR_getuid);
    pub const SYS_GETGID: i64 = nr!(getgid, __NR_getgid);
    pub const SYS_SETUID: i64 = nr!(setuid, __NR_setuid);
    pub const SYS_SETGID: i64 = nr!(setgid, __NR_setgid);
    pub const SYS_GETGROUPS: i64 = nr!(getgroups, __NR_getgroups);
    pub const SYS_SETGROUPS: i64 = nr!(setgroups, __NR_setgroups);
}

pub use id_calls::{SYS_GETGID, SYS_GETGROUPS, SYS_GETUID, SYS_SETGID, SYS_SETGROUPS, SYS_SETUID};
pub const SYS_STATFS: i64 = nr!(statfs, __NR_statfs);
/// The wide form of a 32-bit kernel: `statfs` there writes the narrow
/// struct into a buffer of the wide one.
#[cfg(target_pointer_width = "32")]
pub const SYS_STATFS64: i64 = nr!(statfs64, __NR_statfs64);
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
// A supervisor: a pidfd for each direct child, `waitid` for its status, and
// `ppoll` to wait for the CONDITION, not for a guessed duration.
// `ppoll` and not `poll`: `poll` does not exist on aarch64 or riscv64.
pub const SYS_PIDFD_OPEN: i64 = nr!(pidfd_open, __NR_pidfd_open);
pub const SYS_WAITID: i64 = nr!(waitid, __NR_waitid);
pub const SYS_PPOLL: i64 = nr!(ppoll, __NR_ppoll);
// The pair that forwards a shutdown: a waiter that forwards its own SIGINT/SIGTERM to the payload blocks them first and reads
// them from a signalfd, so the signal never kills the waiter between the
// decision and the forward. `rt_sigprocmask` and not `sigprocmask`: the raw
// call takes the set size, and the libc wrapper's is not the kernel's.
pub const SYS_RT_SIGPROCMASK: i64 = nr!(rt_sigprocmask, __NR_rt_sigprocmask);
pub const SYS_SIGNALFD4: i64 = nr!(signalfd4, __NR_signalfd4);
pub const SYS_LANDLOCK_CREATE_RULESET: i64 = nr!(landlock_create_ruleset, __NR_landlock_create_ruleset);
pub const SYS_LANDLOCK_ADD_RULE: i64 = nr!(landlock_add_rule, __NR_landlock_add_rule);
pub const SYS_LANDLOCK_RESTRICT_SELF: i64 = nr!(landlock_restrict_self, __NR_landlock_restrict_self);
pub const SYS_CONNECT: i64 = nr!(connect, __NR_connect);
// The calls of the deny list of a confined payload (pod-confine), beside
// the ones above that a probe asks too (`unshare`, `mount`, `umount2`,
// `pivot_root`, the new mount API, `ptrace`, `process_vm_readv`).
pub const SYS_SETNS: i64 = nr!(setns, __NR_setns);
pub const SYS_CLONE3: i64 = nr!(clone3, __NR_clone3);
pub const SYS_FSPICK: i64 = nr!(fspick, __NR_fspick);
pub const SYS_MOUNT_SETATTR: i64 = nr!(mount_setattr, __NR_mount_setattr);
pub const SYS_OPEN_TREE_ATTR: i64 = nr!(open_tree_attr, __NR_open_tree_attr);
pub const SYS_PROCESS_VM_WRITEV: i64 = nr!(process_vm_writev, __NR_process_vm_writev);
pub const SYS_KEYCTL: i64 = nr!(keyctl, __NR_keyctl);
pub const SYS_ADD_KEY: i64 = nr!(add_key, __NR_add_key);
pub const SYS_REQUEST_KEY: i64 = nr!(request_key, __NR_request_key);
pub const SYS_BPF: i64 = nr!(bpf, __NR_bpf);
pub const SYS_PERF_EVENT_OPEN: i64 = nr!(perf_event_open, __NR_perf_event_open);
pub const SYS_KEXEC_LOAD: i64 = nr!(kexec_load, __NR_kexec_load);
/// i386 has no `kexec_file_load`.
#[cfg(not(target_arch = "x86"))]
pub const SYS_KEXEC_FILE_LOAD: i64 = nr!(kexec_file_load, __NR_kexec_file_load);
pub const SYS_INIT_MODULE: i64 = nr!(init_module, __NR_init_module);
pub const SYS_FINIT_MODULE: i64 = nr!(finit_module, __NR_finit_module);
pub const SYS_DELETE_MODULE: i64 = nr!(delete_module, __NR_delete_module);
pub const SYS_OPEN_BY_HANDLE_AT: i64 = nr!(open_by_handle_at, __NR_open_by_handle_at);
pub const SYS_NAME_TO_HANDLE_AT: i64 = nr!(name_to_handle_at, __NR_name_to_handle_at);
/// The old `umount` with no flags: only these kernels keep it.
#[cfg(any(target_arch = "x86", target_arch = "s390x", target_arch = "powerpc64"))]
pub const SYS_UMOUNT: i64 = nr!(umount, __NR_umount);
/// `socketcall(2)`: only these kernels keep it. Its arguments are behind a
/// pointer, so a filter cannot read the family of its socket.
#[cfg(any(target_arch = "x86", target_arch = "s390x", target_arch = "powerpc64"))]
pub const SYS_SOCKETCALL: i64 = nr!(socketcall, __NR_socketcall);
// A ring of io_uring does work (a socket, an open) with no system call of
// its own, so a seccomp filter does not see that work.
pub const SYS_IO_URING_SETUP: i64 = nr!(io_uring_setup, __NR_io_uring_setup);
pub const SYS_IO_URING_ENTER: i64 = nr!(io_uring_enter, __NR_io_uring_enter);
pub const SYS_IO_URING_REGISTER: i64 = nr!(io_uring_register, __NR_io_uring_register);
pub const SYS_DUP3: i64 = nr!(dup3, __NR_dup3);
// The entry sequence: the rootfs is held as a directory DESCRIPTOR and entered with `fchdir` then `chroot(".")`, so nothing
// between checking the path and changing the root can swap it.
pub const SYS_CHDIR: i64 = nr!(chdir, __NR_chdir);
pub const SYS_FCHDIR: i64 = nr!(fchdir, __NR_fchdir);

// The `*at` family. An extraction resolves each entry against a directory
// FILE DESCRIPTOR and not against a path, because the kernel resolves a path
// again on each call and the tree changes between calls.
//
// Each wrapper in this module that takes a path uses them too, on EACH
// architecture and not only on the ones that have nothing else. `open(path)` is `openat(AT_FDCWD, path)` by definition, so
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
#[cfg(target_arch = "riscv64")]
pub const SYS_RENAMEAT: i64 = nr!(renameat2, __NR_renameat2);
pub const SYS_LINKAT: i64 = nr!(linkat, __NR_linkat);
pub const SYS_SYMLINKAT: i64 = nr!(symlinkat, __NR_symlinkat);
pub const SYS_READLINKAT: i64 = nr!(readlinkat, __NR_readlinkat);
pub const SYS_FCHMODAT: i64 = nr!(fchmodat, __NR_fchmodat);
pub const SYS_FACCESSAT: i64 = nr!(faccessat, __NR_faccessat);
pub const SYS_UTIMENSAT: i64 = nr!(utimensat, __NR_utimensat);
pub const SYS_MKNODAT: i64 = nr!(mknodat, __NR_mknodat);
/// Linux 5.6. A kernel without it answers `ENOSYS`, which is why
/// a caller keeps an `O_NOFOLLOW` walk beside it and does not require it.
pub const SYS_OPENAT2: i64 = nr!(openat2, __NR_openat2);
/// The extended attributes of a file: the opaque mark of an overlayfs
/// directory lives there.
pub const SYS_FGETXATTR: i64 = nr!(fgetxattr, __NR_fgetxattr);
pub const SYS_FSETXATTR: i64 = nr!(fsetxattr, __NR_fsetxattr);

// -------------------------------------------- resource limits
//
// `prlimit64` and not `getrlimit`: the latter has no number on architectures
// whose kernel only carries the former, while `prlimit64` answers on every
// Linux architecture. The machine legs read `RLIMIT_FSIZE` because it bounds the memory image, and a guest over it dies with
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
/// The two shutdown signals that a waiter forwards, as numbers:
/// 2 is SIGINT (Ctrl-C on a terminal) and 15 is SIGTERM.
pub const SIGINT: u64 = 2;
pub const SIGTERM: u64 = 15;
/// The signalfd/`sigprocmask` word: exactly those two signal numbers' bit
/// indices, named once so the mask blocked and the descriptor read can never
/// disagree about the set. Signal NUMBER n is bit index (n-1): bit 1 is
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

/// `MS_NOSUID`, `MS_NODEV` and `MS_NOEXEC`, from the headers of the kernel.
pub const MS_NOSUID: u64 = 0x2;
pub const MS_NODEV: u64 = 0x4;
pub const MS_NOEXEC: u64 = 0x8;
/// `MS_BIND`, from the headers of the kernel.
pub const MS_BIND: u64 = 0x1000;
pub const MS_REC: u64 = 0x4000;
pub const MS_SLAVE: u64 = 0x0008_0000;
/// `MS_PRIVATE`, from the headers of the kernel
/// (`/usr/include/sys/mount.h`: `MS_PRIVATE = 1 << 18`). The namespace
/// rung remounts `/` recursively private before it mounts a thing, so its
/// tmpfs cannot propagate to the host.
pub const MS_PRIVATE: u64 = 0x0004_0000;

/// The `f_type` of `statfs(2)` on an overlay mount and on a proc
/// (`include/uapi/linux/magic.h`).
pub const OVERLAYFS_SUPER_MAGIC: u64 = 0x794c_7630;
pub const PROC_SUPER_MAGIC: u64 = 0x9fa0;

pub const S_IFMT: u32 = 0o170000;
pub const S_IFCHR: u64 = 0o0020000;
pub const S_IFIFO: u64 = 0o0010000;
/// `makedev(1, 3)`, that is `/dev/null`. A real device number, unlike
/// `makedev(0, 0)`, which is `WHITEOUT_DEV` and is exempt from the capability
/// check. The census probes that pair.
pub const DEV_1_3: u64 = 0x103;

pub const O_RDONLY: u64 = 0;
pub const O_WRONLY: u64 = 1;
pub const O_RDWR: u64 = 2;
pub const O_CREAT: u64 = 0o100;
/// Every scratch file a probe makes is created with this. A probe that
/// overwrites whatever is in its way has destroyed data to measure a
/// permission, and the errno it then reports is about its own file.
pub const O_EXCL: u64 = 0o200;
/// Every probe that opens a terminal-shaped device passes this. Without it
/// an `open` of a tty can make it the prober's controlling terminal, which is
/// a mutation of the process doing the measuring.
pub const O_NOCTTY: u64 = 0o400;
/// The kernel's value differs by architecture: arm, aarch64 and powerpc
/// use 0o40000, the others 0o200000.
#[cfg(any(target_arch = "arm", target_arch = "aarch64", target_arch = "powerpc64", target_arch = "powerpc"))]
pub const O_DIRECTORY: u64 = 0o40000;
#[cfg(not(any(target_arch = "arm", target_arch = "aarch64", target_arch = "powerpc64", target_arch = "powerpc")))]
pub const O_DIRECTORY: u64 = 0o200000;
pub const O_CLOEXEC: u64 = 0o2000000;
pub const O_TRUNC: u64 = 0o1000;
/// The one flag that makes an `open` of a path component a statement about
/// that component rather than about wherever a symlink pointed. Every directory
/// descent of an extraction carries it.
#[cfg(any(target_arch = "arm", target_arch = "aarch64", target_arch = "powerpc64", target_arch = "powerpc"))]
pub const O_NOFOLLOW: u64 = 0o100000;
/// See [`O_DIRECTORY`]: the value follows the architecture.
#[cfg(not(any(target_arch = "arm", target_arch = "aarch64", target_arch = "powerpc64", target_arch = "powerpc")))]
pub const O_NOFOLLOW: u64 = 0o400000;
pub const O_PATH: u64 = 0o10000000;
/// An open that does not wait: a fifo with no writer, or a device that
/// waits for a carrier, gives the descriptor at once. The value is
/// 0o200 on mips and 0o4000 on the other targets.
#[cfg(any(target_arch = "mips", target_arch = "mips64"))]
pub const O_NONBLOCK: u64 = 0o200;
/// See [`O_NONBLOCK`]: the value follows the architecture.
#[cfg(not(any(target_arch = "mips", target_arch = "mips64")))]
pub const O_NONBLOCK: u64 = 0o4000;

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

mod pidfd;
pub use pidfd::*;

mod statx;
pub use statx::statx_at;

mod seccomp;
pub use seccomp::*;

mod landlock;
pub use landlock::*;

mod caps;
pub use caps::*;

#[cfg(test)]
mod tests;
