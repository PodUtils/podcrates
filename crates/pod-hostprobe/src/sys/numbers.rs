//! The errno numbers and the open flags of the architecture of the build.
//! Both differ by architecture: mips gives `ENOSYS` 89 and `O_CREAT` 0x100,
//! sparc gives `ELOOP` 62 and `O_CREAT` 0x200, most of the others `ENOSYS`
//! 38 and `O_CREAT` 0x40.
//! Each value comes from the headers of the kernel for the architecture,
//! through `linux_raw_sys`; none is written here.

use linux_raw_sys::errno as e;
use linux_raw_sys::general as g;

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
        let Ok(n) = u32::try_from(self.0) else {
            return format!("E{}", self.0);
        };
        let n = match n {
            e::EPERM => "EPERM",
            e::ENOENT => "ENOENT",
            e::ESRCH => "ESRCH",
            e::EINTR => "EINTR",
            e::EIO => "EIO",
            e::E2BIG => "E2BIG",
            e::ENOEXEC => "ENOEXEC",
            e::EBADF => "EBADF",
            e::ECHILD => "ECHILD",
            e::EAGAIN => "EAGAIN",
            e::ENOMEM => "ENOMEM",
            e::EACCES => "EACCES",
            e::EFAULT => "EFAULT",
            e::EBUSY => "EBUSY",
            e::ENOTTY => "ENOTTY",
            e::EEXIST => "EEXIST",
            e::EXDEV => "EXDEV",
            e::ENODEV => "ENODEV",
            e::ENOTDIR => "ENOTDIR",
            e::EISDIR => "EISDIR",
            e::EINVAL => "EINVAL",
            e::EMFILE => "EMFILE",
            e::ETXTBSY => "ETXTBSY",
            e::ENOSPC => "ENOSPC",
            e::EROFS => "EROFS",
            e::ENAMETOOLONG => "ENAMETOOLONG",
            e::ENOSYS => "ENOSYS",
            e::ENOTEMPTY => "ENOTEMPTY",
            e::ELOOP => "ELOOP",
            // The answer of an `execve` whose ELF interpreter has a bad
            // format.
            e::ELIBBAD => "ELIBBAD",
            e::EOPNOTSUPP => "EOPNOTSUPP",
            // The answers of a socket call: a family that the host does not
            // have, and a file that a Landlock rule cannot name.
            e::EAFNOSUPPORT => "EAFNOSUPPORT",
            e::EBADFD => "EBADFD",
            // The answers of a connect, and of an open of `/dev/tty` with
            // no controlling terminal.
            e::ENXIO => "ENXIO",
            e::ENETUNREACH => "ENETUNREACH",
            e::EHOSTUNREACH => "EHOSTUNREACH",
            e::ECONNREFUSED => "ECONNREFUSED",
            e::EINPROGRESS => "EINPROGRESS",
            e::EISCONN => "EISCONN",
            e::ETIMEDOUT => "ETIMEDOUT",
            e::EPROTONOSUPPORT => "EPROTONOSUPPORT",
            e::ESOCKTNOSUPPORT => "ESOCKTNOSUPPORT",
            _ => return format!("E{}", self.0),
        };
        n.to_string()
    }

    /// Was the file of an `execve` there and refused, and not absent?
    ///
    /// The shell and docker exit 126 for these, and 127 for each other
    /// errno. `execve` gives `ENOENT` for a path that is not there, and
    /// also for a dynamic loader that the binary names and that is not
    /// there. These mean that the file was found and could not run:
    ///
    /// * `EPERM`: a mount with `nosuid`, or a security module.
    /// * `E2BIG`: the argv and the environment are too large for the
    ///   kernel. The command is there; each path refuses the same strings.
    /// * `ENOEXEC`: a format that the kernel does not know.
    /// * `EACCES`: no execute permission, or a file that is not regular.
    /// * `EISDIR`: the ELF interpreter is a directory.
    /// * `EINVAL`: the ELF names more than one interpreter.
    /// * `ETXTBSY`: the file is open for a write.
    /// * `ELIBBAD`: the ELF interpreter has a bad format.
    ///
    /// `ELOOP`, `ENAMETOOLONG` and `ENOTDIR` stay with "not found": each is
    /// an answer about the resolution of the path, not about the file at
    /// its end. `ENOMEM`, `EIO`, `EMFILE`, `ENFILE` and `EFAULT` stay too:
    /// each is a failure of the host, not an answer about the file.
    pub fn exec_refused(self) -> bool {
        let Ok(n) = u32::try_from(self.0) else { return false };
        matches!(n, e::EPERM | e::E2BIG | e::ENOEXEC | e::EACCES | e::EISDIR | e::EINVAL | e::ETXTBSY | e::ELIBBAD)
    }
}

/// The errno `n` of the headers of the kernel.
const fn errno(n: u32) -> Errno {
    Errno(n as i32)
}

pub const EPERM: Errno = errno(e::EPERM);
pub const ENOENT: Errno = errno(e::ENOENT);
pub const ESRCH: Errno = errno(e::ESRCH);
/// `ECHILD`: `wait4` has no child to wait for, alive or ended.
pub const ECHILD: Errno = errno(e::ECHILD);
pub const EINTR: Errno = errno(e::EINTR);
pub const EIO: Errno = errno(e::EIO);
/// `E2BIG`: `openat2(2)` gives it for a `struct open_how` that the kernel
/// does not know.
pub const E2BIG: Errno = errno(e::E2BIG);
pub const EBADF: Errno = errno(e::EBADF);
/// `ENOTTY`: a device that does not know an ioctl.
pub const ENOTTY: Errno = errno(e::ENOTTY);
pub const EACCES: Errno = errno(e::EACCES);
pub const EEXIST: Errno = errno(e::EEXIST);
/// `EXDEV`, cross-device link: the number the (d,i) guard reports under,
/// because a chroot that landed off its descriptor is on the wrong device
/// or the wrong file.
pub const EXDEV: Errno = errno(e::EXDEV);
pub const ENOTDIR: Errno = errno(e::ENOTDIR);
/// `EROFS`: a write to a file system or a mount that is read-only.
pub const EROFS: Errno = errno(e::EROFS);
pub const EISDIR: Errno = errno(e::EISDIR);
pub const EINVAL: Errno = errno(e::EINVAL);
pub const ENOSYS: Errno = errno(e::ENOSYS);
pub const ENAMETOOLONG: Errno = errno(e::ENAMETOOLONG);
/// `ELOOP`: the refusal of `O_NOFOLLOW` and of `RESOLVE_NO_SYMLINKS`.
pub const ELOOP: Errno = errno(e::ELOOP);
/// On Linux `EAGAIN` and `EWOULDBLOCK` are the same number, and `flock` with
/// `LOCK_NB` returns it for "somebody else holds this". It is named here for
/// the reader, because "try again later" and "in use" are different sentences.
pub const EWOULDBLOCK: Errno = errno(e::EWOULDBLOCK);
/// `ENXIO`: an open of `/dev/tty` by a process with no controlling
/// terminal.
pub const ENXIO: Errno = errno(e::ENXIO);
/// `EAFNOSUPPORT`: a socket of a family that the kernel does not have (IPv6
/// turned off).
pub const EAFNOSUPPORT: Errno = errno(e::EAFNOSUPPORT);
/// `ENETUNREACH`: no route, also to the loopback where it is down.
pub const ENETUNREACH: Errno = errno(e::ENETUNREACH);
/// `ECONNREFUSED`: the peer answered with a reset; no listener on the port.
pub const ECONNREFUSED: Errno = errno(e::ECONNREFUSED);
/// `EINPROGRESS`: a connect of a socket that does not wait.
pub const EINPROGRESS: Errno = errno(e::EINPROGRESS);
/// `EISCONN`: a second connect of a socket that is connected.
pub const EISCONN: Errno = errno(e::EISCONN);

pub const O_RDONLY: u64 = g::O_RDONLY as u64;
pub const O_WRONLY: u64 = g::O_WRONLY as u64;
pub const O_RDWR: u64 = g::O_RDWR as u64;
pub const O_CREAT: u64 = g::O_CREAT as u64;
/// Every scratch file a probe makes is created with this. A probe that
/// overwrites whatever is in its way has destroyed data to measure a
/// permission, and the errno it then reports is about its own file.
pub const O_EXCL: u64 = g::O_EXCL as u64;
/// Every probe that opens a terminal-shaped device passes this. Without it
/// an `open` of a tty can make it the prober's controlling terminal, which is
/// a mutation of the process doing the measuring.
pub const O_NOCTTY: u64 = g::O_NOCTTY as u64;
/// arm, aarch64 and powerpc use 0o40000, the others 0o200000.
pub const O_DIRECTORY: u64 = g::O_DIRECTORY as u64;
pub const O_CLOEXEC: u64 = g::O_CLOEXEC as u64;
pub const O_TRUNC: u64 = g::O_TRUNC as u64;
/// The one flag that makes an `open` of a path component a statement about
/// that component rather than about wherever a symlink pointed. Every directory
/// descent of an extraction carries it.
pub const O_NOFOLLOW: u64 = g::O_NOFOLLOW as u64;
pub const O_PATH: u64 = g::O_PATH as u64;
/// An open that does not wait: a fifo with no writer, or a device that
/// waits for a carrier, gives the descriptor at once.
pub const O_NONBLOCK: u64 = g::O_NONBLOCK as u64;

// The values of mips and sparc differ from those of the host of the tests.
// A build of this module for each of them proves that the values follow the
// target.
#[cfg(any(target_arch = "mips", target_arch = "mips64"))]
const _: () = assert!(
    O_CREAT == 0x100
        && O_EXCL == 0x400
        && O_NOCTTY == 0x800
        && O_TRUNC == 0x200
        && O_NONBLOCK == 0x80
        && ENAMETOOLONG.0 == 78
        && ENOSYS.0 == 89
        && ELOOP.0 == 90
);
#[cfg(any(target_arch = "sparc", target_arch = "sparc64"))]
const _: () = assert!(
    O_CREAT == 0x200
        && O_EXCL == 0x800
        && O_NOCTTY == 0x8000
        && O_TRUNC == 0x400
        && O_NONBLOCK == 0x4000
        && ELOOP.0 == 62
        && ENAMETOOLONG.0 == 63
        && ENOSYS.0 == 90
);
