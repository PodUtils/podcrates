//! The failures of the crate. Each failure is a [`pod_core::Failure`] with
//! its stage and its class. A caller that must tell two refusals apart
//! reads the typed cause, a [`Refusal`], from the chain ([`refusal`]).
//!
//! The stages: [`Stage::Registry`] for an answer, a document or a digest of
//! a registry; the stage of `pod-proxy` (`Dns`, `Proxy`, `Connect`,
//! `Network`, `Tls`) for a request that got no answer; [`Stage::Host`] for a
//! local file, a credential helper or a store of the caller.
//!
//! The classes: `Usage` for a value that the caller wrote (a reference, a
//! digest, a platform, a host); `NotFound` for a missing manifest, blob or
//! platform; `Refused` for a refused credential; `TempFail` for a limit of
//! the rate; `Policy` for a refusal by a limit of the crate or of the
//! policy; `Protocol` for a document or an answer that breaks the
//! protocol; `Unsupported` for a document or a scheme that the crate does
//! not read.

use std::fmt;

pub use pod_core::{Class, Failure, Stage};

/// The result of the crate.
pub type Result<T> = std::result::Result<T, Failure>;

/// A typed cause of a failure, for a caller that acts on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The computed digest is not the declared one. Both are named:
    /// "digest mismatch" alone gives no action.
    Digest { what: String, want: String, got: String },
    /// An index has no manifest for the wanted platform: the wanted
    /// platform and the offered ones, comma-joined. A pull of each tag skips
    /// such a tag; a pull of one tag fails on it.
    NoPlatform { want: String, offered: String },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Digest { what, want, got } => {
                write!(f, "{what}: digest mismatch, expected {want} and computed {got}. The bytes were discarded")
            }
            Refusal::NoPlatform { want, offered } => {
                write!(f, "this index offers no {want} manifest. It offers: {offered}")
            }
        }
    }
}

impl std::error::Error for Refusal {}

impl Refusal {
    /// The failure of this refusal, at [`Stage::Registry`]. Its text is the
    /// text of the refusal.
    pub fn failure(self) -> Failure {
        let class = match self {
            Refusal::Digest { .. } => Class::Protocol,
            Refusal::NoPlatform { .. } => Class::NotFound,
        };
        Failure::caused(Stage::Registry, class, self.to_string(), self)
    }
}

/// The [`Refusal`] in the chain of `f`, when one is.
pub fn refusal(f: &Failure) -> Option<&Refusal> {
    let mut next: Option<&(dyn std::error::Error + 'static)> = Some(f);
    while let Some(e) = next {
        if let Some(r) = e.downcast_ref::<Refusal>() {
            return Some(r);
        }
        next = e.source();
    }
    None
}

/// A failure at [`Stage::Registry`].
pub(crate) fn registry(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Registry, class, text)
}

/// A value that the caller wrote and that the crate cannot use.
pub(crate) fn usage(text: impl Into<String>) -> Failure {
    Failure::new(Stage::Registry, Class::Usage, text)
}

/// An error of I/O on a local path, with the path in the text.
pub(crate) fn io(path: impl fmt::Display, e: std::io::Error) -> Failure {
    let class = Class::of_io(e.kind());
    Failure::caused(Stage::Host, class, format!("{path}: {e}"), e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_comes_back_from_the_chain_with_its_class() {
        let f = Refusal::NoPlatform { want: "linux/arm64".into(), offered: "linux/amd64".into() }.failure();
        assert_eq!((f.stage, f.class), (Stage::Registry, Class::NotFound));
        assert!(f.to_string().contains("offers no linux/arm64 manifest"), "{f}");
        assert!(matches!(refusal(&f), Some(Refusal::NoPlatform { .. })));
        let wrapped = Failure::caused(Stage::Registry, f.class, "pull", f);
        assert!(matches!(refusal(&wrapped), Some(Refusal::NoPlatform { .. })));
        assert!(refusal(&usage("bad")).is_none());
    }
}
