//! The failures of a probe that cannot run. A server that does not answer is a finding of the
//! report, not a [`NatError`].

use std::fmt;

use pod_core::{Class, Failure, Stage};

/// A probe that cannot run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NatError {
    /// The settings of the caller are not valid.
    InvalidSettings(String),
    /// A probe runs already on this `Prober` (one run at a time).
    Busy,
    /// The entropy source of the caller gave no random bytes.
    NoRandom,
    /// The probe cannot bind a UDP socket.
    NoSocket(String),
    /// The report is not consistent (a defect of the probe).
    ReportInvariant(String),
}

impl fmt::Display for NatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSettings(msg) => write!(f, "the settings of the NAT probe are not valid: {msg}"),
            Self::Busy => f.write_str("a NAT probe runs already on this prober"),
            Self::NoRandom => f.write_str("the entropy source gave no random bytes"),
            Self::NoSocket(msg) => write!(f, "the NAT probe cannot bind a UDP socket: {msg}"),
            Self::ReportInvariant(msg) => write!(f, "the report of the NAT probe is not consistent: {msg}"),
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
