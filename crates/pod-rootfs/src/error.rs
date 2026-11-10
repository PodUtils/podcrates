//! The failures. Each is a [`pod_core::Failure`] at `Stage::Host`:
//!
//! * a path that leaves the root, or a file over the read limit:
//!   `Class::Policy`;
//! * a setting that cannot be used (a malformed `name:address`, a name with
//!   a line end): `Class::Usage`;
//! * a refused system call: the class of its errno, with the
//!   [`std::io::Error`] of the errno as the cause.
//!
//! A fixup that fails gives one failed row in the report, with the text of
//! its failure; it does not stop the other fixups.

use pod_core::{Class, Failure, Stage};
use pod_hostprobe::sys::{CBuf, Errno};

pub type Result<T> = std::result::Result<T, Failure>;

/// A refused system call: the text names the errno, and the cause is the
/// [`std::io::Error`] of the errno, so that the class follows its kind.
pub(crate) fn sys(text: impl Into<String>, e: Errno) -> Failure {
    let cause = std::io::Error::from_raw_os_error(e.0);
    let class = Class::of_io(cause.kind());
    Failure::caused(Stage::Host, class, format!("{}: {}", text.into(), e.name()), cause)
}

/// A refused file operation of the standard library.
pub(crate) fn io(text: impl Into<String>, e: std::io::Error) -> Failure {
    let class = Class::of_io(e.kind());
    Failure::caused(Stage::Host, class, format!("{}: {e}", text.into()), e)
}

/// A setting or a value of the caller that cannot be used.
pub(crate) fn usage(text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, Class::Usage, text)
}

/// A path or a file that the crate refuses.
pub(crate) fn policy(text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, Class::Policy, text)
}

/// A name for the kernel, or a failure that names it.
pub(crate) fn cbuf(s: &str) -> Result<CBuf> {
    CBuf::new(s).ok_or_else(|| usage(format!("{s:?} contains a NUL and cannot reach the kernel")))
}
