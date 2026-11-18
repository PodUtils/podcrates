//! The refusals of a source: the code of each, and a detail with no value
//! of the request.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The class of a refusal of a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SourceCode {
    /// A line that the wire refuses: too long, not JSON, an unknown `op`, a
    /// bad `job_id`, another `schema`.
    BadRequest,
    /// No offer with this job id (never offered, withdrawn, or released).
    UnknownJob,
    /// The job is claimed already.
    AlreadyClaimed,
    /// The operation needs a claim first.
    NotClaimed,
    /// The source revoked the credential of podrunner (`source-revoked`).
    SourceRevoked,
    /// The source cannot give a credential now (`credential-unavailable`).
    CredentialUnavailable,
    /// The remote mint gave a credential for this job before: one for each
    /// job id.
    CredentialAlreadyIssued,
    /// The source is not reachable.
    Unavailable,
}

impl SourceCode {
    /// Each code, in the order of the contract.
    pub const ALL: [SourceCode; 8] = [
        SourceCode::BadRequest,
        SourceCode::UnknownJob,
        SourceCode::AlreadyClaimed,
        SourceCode::NotClaimed,
        SourceCode::SourceRevoked,
        SourceCode::CredentialUnavailable,
        SourceCode::CredentialAlreadyIssued,
        SourceCode::Unavailable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SourceCode::BadRequest => "bad-request",
            SourceCode::UnknownJob => "unknown-job",
            SourceCode::AlreadyClaimed => "already-claimed",
            SourceCode::NotClaimed => "not-claimed",
            SourceCode::SourceRevoked => "source-revoked",
            SourceCode::CredentialUnavailable => "credential-unavailable",
            SourceCode::CredentialAlreadyIssued => "credential-already-issued",
            SourceCode::Unavailable => "unavailable",
        }
    }
}

/// A refusal of a source: a code and a detail with no credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError {
    pub code: SourceCode,
    pub detail: String,
}

impl SourceError {
    pub fn new(code: SourceCode, detail: impl Into<String>) -> SourceError {
        SourceError { code, detail: detail.into() }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.detail)
    }
}

impl std::error::Error for SourceError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text of each code is its serde name.
    #[test]
    fn each_code_is_its_wire_name() {
        for code in SourceCode::ALL {
            assert_eq!(serde_json::to_value(code).unwrap(), code.as_str());
        }
    }
}
