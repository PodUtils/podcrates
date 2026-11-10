//! The failures. Each is a [`pod_core::Failure`] at `Stage::Host`:
//!
//! * an entry that resolves outside the destination: `Class::Policy`, with
//!   the cause [`Refused`] (the entry and the digest of the layer);
//! * a blob that does not agree with its digest: `Class::Protocol`, with the
//!   cause [`Mismatch`];
//! * a compression or a digest algorithm that the crate cannot read:
//!   `Class::Unsupported`;
//! * a refused system call: the class of its errno, with the
//!   [`std::io::Error`] of the errno as the cause.
//!
//! [`refused`] and [`mismatch`] give the typed cause from the chain.

use std::fmt;

use pod_core::{Class, Failure, Stage};
use pod_hostprobe::sys::Errno;

use crate::safety::Refusal;

pub type Result<T> = std::result::Result<T, Failure>;

/// An entry that the crate does not extract. The layer is refused whole.
///
/// The entry and the digest of the layer are both named: a refusal that names
/// neither does not tell in which of a hundred layers to look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub layer: String,
    pub entry: String,
    pub refusal: Refusal,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The crate refuses and does not repair: a changed path gives an image
        // that differs from its digest with no word about it, and the caller
        // cannot then tell a repaired layer from a clean one.
        write!(
            f,
            "refusing layer {}: the entry {:?} is not extractable because {}. The layer is refused whole rather \
             than with that entry skipped, because an image that differs from its digest with nothing saying so \
             cannot be told from a clean one",
            self.layer,
            self.entry,
            self.refusal.why()
        )
    }
}

impl std::error::Error for Refused {}

/// A blob whose bytes do not agree with the digest of the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub what: String,
    pub want: String,
    pub got: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: digest mismatch, expected {} and computed {}. The bytes were discarded",
            self.what, self.want, self.got
        )
    }
}

impl std::error::Error for Mismatch {}

/// The [`Refused`] cause of a failure, when it has one.
pub fn refused(f: &Failure) -> Option<&Refused> {
    cause(f)
}

/// The [`Mismatch`] cause of a failure, when it has one.
pub fn mismatch(f: &Failure) -> Option<&Mismatch> {
    cause(f)
}

fn cause<T: std::error::Error + 'static>(f: &Failure) -> Option<&T> {
    let mut next: Option<&(dyn std::error::Error + 'static)> = Some(f);
    while let Some(e) = next {
        if let Some(t) = e.downcast_ref::<T>() {
            return Some(t);
        }
        next = e.source();
    }
    None
}

pub(crate) fn refuse(layer: &str, entry: &str, refusal: Refusal) -> Failure {
    let r = Refused { layer: layer.to_string(), entry: entry.to_string(), refusal };
    Failure::caused(Stage::Host, Class::Policy, r.to_string(), r)
}

pub(crate) fn io(e: std::io::Error) -> Failure {
    Failure::io(Stage::Host, e)
}

/// A refused system call: the text names the errno, and the cause is the
/// [`std::io::Error`] of the errno, so that the class follows its kind.
pub(crate) fn sys(text: impl Into<String>, e: Errno) -> Failure {
    let cause = std::io::Error::from_raw_os_error(e.0);
    let class = Class::of_io(cause.kind());
    Failure::caused(Stage::Host, class, format!("{}: {}", text.into(), e.name()), cause)
}

/// A value that cannot reach the kernel (a NUL in a name).
pub(crate) fn usage(text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, Class::Usage, text)
}

pub(crate) fn unsupported(text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, Class::Unsupported, text)
}

/// A name for the kernel, or a failure that names it.
pub(crate) fn cbuf(s: &str) -> Result<pod_hostprobe::sys::CBuf> {
    pod_hostprobe::sys::CBuf::new(s).ok_or_else(|| usage(format!("{s:?} contains a NUL and cannot reach the kernel")))
}
