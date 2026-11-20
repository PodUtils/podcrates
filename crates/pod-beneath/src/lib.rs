//! The open of a path under a base directory with no symbolic link and
//! nothing above the base.
//!
//! The rule: split the host path at the base, open the base, and open the
//! rest under the base with `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`.
//! The kernel then refuses each link and each `..` above the base in one
//! call, so a link that another process makes after a check moves no open
//! out of the base. The result is a descriptor, or a [`Refusal`] that names
//! the case.
//!
//! The crate is `no_std` and does no allocation: the caller gives the bytes
//! of each path and a buffer for the NUL-ended copy. The caller also gives
//! the system calls ([`Calls`]), so that an `LD_PRELOAD` interposer or a
//! handler of Syscall User Dispatch uses its own trap. The feature `libc`
//! gives [`Libc`], through `syscall(2)` of the C library.
//!
//! The crate is Linux only. On another target it is empty.
#![no_std]
#![cfg(target_os = "linux")]
#![forbid(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "libc")]
mod libc;
mod refusal;
mod walk;

#[cfg(feature = "libc")]
pub use libc::Libc;
pub use refusal::Refusal;
pub use walk::{EAGAIN_TRIES, last_name, open_beneath, open_parent_beneath, open_under_base, split};

use core::ffi::{CStr, c_int};
use linux_raw_sys::general as g;

/// `O_PATH`: a descriptor of the place, with no read and no write.
pub const O_PATH: u64 = g::O_PATH as u64;
/// `O_DIRECTORY`: the last name must be a directory.
pub const O_DIRECTORY: u64 = g::O_DIRECTORY as u64;
/// `O_NOFOLLOW`: a link at the last name is `ELOOP`.
pub const O_NOFOLLOW: u64 = g::O_NOFOLLOW as u64;
/// `O_CLOEXEC`: each open of this crate adds it.
pub const O_CLOEXEC: u64 = g::O_CLOEXEC as u64;
/// `O_CREAT`: make the file of the last name.
pub const O_CREAT: u64 = g::O_CREAT as u64;
/// `O_EXCL`: with `O_CREAT`, the file must not exist.
pub const O_EXCL: u64 = g::O_EXCL as u64;
/// `O_WRONLY`.
pub const O_WRONLY: u64 = g::O_WRONLY as u64;
/// `O_RDWR`.
pub const O_RDWR: u64 = g::O_RDWR as u64;
/// `RESOLVE_BENEATH`: no step above the directory of the call.
pub const RESOLVE_BENEATH: u64 = g::RESOLVE_BENEATH as u64;
/// `RESOLVE_NO_SYMLINKS`: no symbolic link on the way, also at the last name.
pub const RESOLVE_NO_SYMLINKS: u64 = g::RESOLVE_NO_SYMLINKS as u64;
/// The `resolve` of each walk of this crate.
pub const BENEATH: u64 = RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS;
/// `AT_FDCWD`: the directory of the process.
pub const AT_FDCWD: c_int = g::AT_FDCWD;
/// The number of `openat2(2)` on this architecture.
pub const SYS_OPENAT2: u32 = g::__NR_openat2;

/// `struct open_how` of `openat2(2)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenHow {
    /// The open flags.
    pub flags: u64,
    /// The mode of a file that `O_CREAT` makes.
    pub mode: u64,
    /// The `RESOLVE_*` flags.
    pub resolve: u64,
}

/// The system calls of a walk. The caller gives them, so that each
/// consumer keeps its own trap (a raw `syscall` instruction, a call that a
/// seccomp filter or Syscall User Dispatch lets through).
pub trait Calls {
    /// `openat2(dirfd, path, how, size_of::<OpenHow>())`: the descriptor,
    /// or the errno (a positive number). One call: the walk tries `EAGAIN`
    /// again itself.
    fn openat2(&self, dirfd: c_int, path: &CStr, how: &OpenHow) -> Result<c_int, c_int>;

    /// `close(fd)` of a descriptor that the walk opened and does not give.
    fn close(&self, fd: c_int);
}

impl<C: Calls + ?Sized> Calls for &C {
    fn openat2(&self, dirfd: c_int, path: &CStr, how: &OpenHow) -> Result<c_int, c_int> {
        (**self).openat2(dirfd, path, how)
    }

    fn close(&self, fd: c_int) {
        (**self).close(fd)
    }
}
