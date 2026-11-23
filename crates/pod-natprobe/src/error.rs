//! Typed crate errors for "probe could not run" failures. An unreachable server is a report
//! finding, never a [`NatError`].

use std::fmt;

use pod_core::{Class, Failure, Stage};

/// Error returned when a probe cannot execute at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NatError {
    /// Invalid caller settings.
    InvalidSettings(String),
    /// A probe run is already active on this `Prober` (single-flight).
    Busy,
    /// The injected entropy source failed to provide random bytes.
    NoRandom,
    /// No UDP socket could be bound.
    NoSocket(String),
    /// Internal report invariant failed during assembly.
    ReportInvariant(String),
}

impl fmt::Display for NatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSettings(msg) => write!(f, "invalid natprobe settings: {msg}"),
            Self::Busy => f.write_str("another natprobe run is already active"),
            Self::NoRandom => f.write_str("entropy source failed to produce random bytes"),
            Self::NoSocket(msg) => write!(f, "could not bind probe UDP socket: {msg}"),
            Self::ReportInvariant(msg) => write!(f, "report invariant failed: {msg}"),
        }
    }
}

impl std::error::Error for NatError {}

impl From<NatError> for Failure {
    fn from(err: NatError) -> Self {
        let (stage, class) = match &err {
            NatError::InvalidSettings(_) => (Stage::Host, Class::Usage),
            NatError::Busy => (Stage::Host, Class::Unavailable),
            NatError::NoRandom => (Stage::Host, Class::Unavailable),
            NatError::NoSocket(_) => (Stage::Connect, Class::Unavailable),
            NatError::ReportInvariant(_) => (Stage::Host, Class::Protocol),
        };
        Failure::new(stage, class, err.to_string())
    }
}
