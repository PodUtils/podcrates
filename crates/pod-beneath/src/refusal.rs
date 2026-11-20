//! The refusal of a walk: one case for each cause, with its errno and one
//! line of text, so that each consumer words the same cases.

use core::ffi::c_int;
use core::fmt;
use linux_raw_sys::errno as e;

/// Why a path does not open under its base or its directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The host path is not at or under the base (a lexical test). For most
    /// callers this is not an error: the base does not apply to the path.
    NotUnderBase,
    /// The base is not an absolute path.
    BaseNotAbsolute,
    /// The base does not open as a directory: the errno.
    Base(c_int),
    /// A name on the way, or the last name, is a symbolic link (`ELOOP`).
    Link,
    /// A `..` goes above the directory of the walk (`EXDEV`). An absolute
    /// path under a descriptor gives the same refusal, as the kernel does.
    DotDot,
    /// A name on the way, or the last name where the caller asks for a
    /// directory, is not a directory (`ENOTDIR`).
    NotDirectory,
    /// The kernel has no `openat2(2)` (`ENOSYS`, Linux 5.5 and older, or a
    /// filter that hides the call). No walk with a weaker rule replaces it.
    NoOpenat2,
    /// The path and its NUL do not fit the buffer of the caller.
    TooLong,
    /// The path holds a NUL byte.
    Nul,
    /// The path does not open for another cause: the errno (`ENOENT`,
    /// `EACCES`, `EAGAIN` after [`crate::EAGAIN_TRIES`] tries).
    Open(c_int),
}

impl Refusal {
    /// The refusal of a failed open of the base.
    pub(crate) fn of_base(errno: c_int) -> Refusal {
        if errno == e::ENOSYS as c_int { Refusal::NoOpenat2 } else { Refusal::Base(errno) }
    }

    /// The refusal of a failed open under a directory.
    pub(crate) fn of_walk(errno: c_int) -> Refusal {
        match errno as u32 {
            e::ELOOP => Refusal::Link,
            e::EXDEV => Refusal::DotDot,
            e::ENOTDIR => Refusal::NotDirectory,
            e::ENOSYS => Refusal::NoOpenat2,
            _ => Refusal::Open(errno),
        }
    }

    /// The errno that a call which takes this refusal gives its caller.
    pub fn errno(self) -> c_int {
        let n = match self {
            Refusal::NotUnderBase | Refusal::DotDot => e::EXDEV,
            Refusal::BaseNotAbsolute | Refusal::Nul => e::EINVAL,
            Refusal::Link => e::ELOOP,
            Refusal::NotDirectory => e::ENOTDIR,
            Refusal::NoOpenat2 => e::ENOSYS,
            Refusal::TooLong => e::ENAMETOOLONG,
            Refusal::Base(n) | Refusal::Open(n) => return n,
        };
        n as c_int
    }

    /// One line that names the cause, with no errno and no path.
    pub fn text(self) -> &'static str {
        match self {
            Refusal::NotUnderBase => "the path is not under the base",
            Refusal::BaseNotAbsolute => "the base is not an absolute path",
            Refusal::Base(_) => "the base does not open as a directory",
            Refusal::Link => "the path holds a symbolic link",
            Refusal::DotDot => "a `..` of the path goes above its directory",
            Refusal::NotDirectory => "a name of the path is not a directory",
            Refusal::NoOpenat2 => "the kernel has no openat2(2), and no other call keeps the path under its base",
            Refusal::TooLong => "the path is longer than the buffer",
            Refusal::Nul => "the path holds a NUL byte",
            Refusal::Open(_) => "the path does not open",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Base(n) | Refusal::Open(n) => write!(f, "{} (errno {n})", self.text()),
            _ => f.write_str(self.text()),
        }
    }
}

impl core::error::Error for Refusal {}
