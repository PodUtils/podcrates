//! The tier of a job and the refusal codes (GitHub #21). The table of
//! `docs/contracts/codes.md` lists the same codes; a test compares them.

use serde::{Deserialize, Serialize};

/// The trust tier of a job. It comes from the trust inputs of an offer,
/// never from a label; `local` comes only from the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// A pull request from a fork.
    Untrusted,
    /// A member job of a configured organization.
    Trusted,
    /// Set only by the configuration.
    Local,
}

/// Why podrunner refuses a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RefusalCode {
    NoSlotOfKind,
    /// Reserved.
    PortClaimed,
    ImageStale,
    EventNotAllowed,
    TierMismatch,
    TierNeedsGuest,
    BoundaryNotAvailable,
    LimitNotEnforceable,
    HoldNotAvailable,
    EgressPolicyUnsupported,
    ArchMismatch,
    PayloadNotPinned,
    RepoScopeRefused,
    ContainerNotSupported,
    ContainerOptionRefused,
    BuildNotSupported,
    SourceRevoked,
    CredentialUnavailable,
    UnitStartFailed,
}

impl RefusalCode {
    /// Each code, in the order of the table.
    pub const ALL: [RefusalCode; 19] = [
        RefusalCode::NoSlotOfKind,
        RefusalCode::PortClaimed,
        RefusalCode::ImageStale,
        RefusalCode::EventNotAllowed,
        RefusalCode::TierMismatch,
        RefusalCode::TierNeedsGuest,
        RefusalCode::BoundaryNotAvailable,
        RefusalCode::LimitNotEnforceable,
        RefusalCode::HoldNotAvailable,
        RefusalCode::EgressPolicyUnsupported,
        RefusalCode::ArchMismatch,
        RefusalCode::PayloadNotPinned,
        RefusalCode::RepoScopeRefused,
        RefusalCode::ContainerNotSupported,
        RefusalCode::ContainerOptionRefused,
        RefusalCode::BuildNotSupported,
        RefusalCode::SourceRevoked,
        RefusalCode::CredentialUnavailable,
        RefusalCode::UnitStartFailed,
    ];

    /// The code as it is on a wire and in a report.
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalCode::NoSlotOfKind => "no-slot-of-kind",
            RefusalCode::PortClaimed => "port-claimed",
            RefusalCode::ImageStale => "image-stale",
            RefusalCode::EventNotAllowed => "event-not-allowed",
            RefusalCode::TierMismatch => "tier-mismatch",
            RefusalCode::TierNeedsGuest => "tier-needs-guest",
            RefusalCode::BoundaryNotAvailable => "boundary-not-available",
            RefusalCode::LimitNotEnforceable => "limit-not-enforceable",
            RefusalCode::HoldNotAvailable => "hold-not-available",
            RefusalCode::EgressPolicyUnsupported => "egress-policy-unsupported",
            RefusalCode::ArchMismatch => "arch-mismatch",
            RefusalCode::PayloadNotPinned => "payload-not-pinned",
            RefusalCode::RepoScopeRefused => "repo-scope-refused",
            RefusalCode::ContainerNotSupported => "container-not-supported",
            RefusalCode::ContainerOptionRefused => "container-option-refused",
            RefusalCode::BuildNotSupported => "build-not-supported",
            RefusalCode::SourceRevoked => "source-revoked",
            RefusalCode::CredentialUnavailable => "credential-unavailable",
            RefusalCode::UnitStartFailed => "unit-start-failed",
        }
    }

    /// A `wait` code holds the offer until a slot comes or the wait
    /// deadline ends; a terminal code refuses the job at once and
    /// suppresses its job id for `terminal_suppress_s`.
    pub fn is_wait(self) -> bool {
        matches!(self, RefusalCode::NoSlotOfKind | RefusalCode::PortClaimed | RefusalCode::CredentialUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_is_the_wire_name() {
        for c in RefusalCode::ALL {
            assert_eq!(serde_json::to_string(&c).unwrap(), format!("\"{}\"", c.as_str()));
        }
    }

    #[test]
    fn the_table_of_codes_is_the_enum() {
        let doc = include_str!("../../../docs/contracts/codes.md");
        let rows: Vec<&str> = doc
            .lines()
            .filter_map(|l| l.strip_prefix("| `"))
            .filter_map(|l| l.split_once('`').map(|(c, _)| c))
            .collect();
        let codes: Vec<&str> = RefusalCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(rows, codes, "the table of docs/contracts/codes.md");
        for c in RefusalCode::ALL {
            let kind = if c.is_wait() { "wait" } else { "terminal" };
            assert!(doc.contains(&format!("| `{}` | {kind} |", c.as_str())), "{} is {kind}", c.as_str());
        }
    }
}
