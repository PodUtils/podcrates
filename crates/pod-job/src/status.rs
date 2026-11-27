//! `status.v1`: what the supervisor of a host of podrunner holds now, by
//! source, then by tier, and each unit kind with its record (GitHub #22 of
//! podrunner). The types only: podrunner fills them from its engine. Each
//! field keeps the name and the form of podrunner's `status.v1`; `degraded`
//! and `key_age_s` are `null` when they have no value.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Capability, SCHEMA, Tier};

/// One source in the status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SourceStatus {
    pub name: String,
    pub tier: Tier,
    pub arch: String,
    pub revoked: bool,
    /// The jobs of the source in each state that is not final.
    pub jobs: BTreeMap<String, u64>,
    /// The refusals of the source, by code.
    pub refused: BTreeMap<String, u64>,
    /// The offers whose trust inputs hold `unknown`: the source could not
    /// read the run or the membership.
    pub trust_unknown: u64,
    /// The age of the credential of each job that is not final, in
    /// seconds, oldest first.
    pub credential_ages_s: Vec<u64>,
    /// The age of the key file of the source (the App key of
    /// `github-scaleset`, the key of `remote`), from its time of change.
    /// No value of the key is read.
    pub key_age_s: Option<u64>,
    /// An operator drained the source: it takes no new offer.
    #[serde(default)]
    pub draining: bool,
}

/// One unit kind in the status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct KindStatus {
    pub name: String,
    pub slots: u32,
    pub busy: u32,
    /// The capability record of the kind (`unit-capability.v1`).
    pub probe: Capability,
    /// The warm units of the kind; absent when the kind keeps none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warm: Option<WarmStatus>,
}

/// The warm units of one kind: units that boot before a job and wait,
/// fresh, for one claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WarmStatus {
    /// The warm units that the kind keeps.
    pub target: u32,
    /// The warm units that boot now.
    pub booting: u32,
    /// The warm units that wait for a claim now.
    pub ready: u32,
    /// Why the kind boots no new warm unit now; absent when it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
}

/// One pinned payload in the status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PinStatus {
    pub arch: String,
    pub reference: String,
    pub age_s: u64,
}

/// One cause of `no_tier_served`: why a unit kind of a source does not meet
/// the floor of the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NoTierCause {
    /// The name of the source.
    pub source: String,
    /// The unit kind.
    pub kind: String,
    /// The boundary or the hold below the floor of the source, or the claim
    /// that the start lowered, with its proof (for example
    /// `guest-self-test-failed`).
    pub cause: String,
    /// The missing key or flag, when the writer knows one; absent when not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// The status of a host (`status.v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Status {
    /// [`SCHEMA`].
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::version"))]
    pub schema: u32,
    pub host: String,
    pub draining: bool,
    /// Why the host is degraded: capacity 0 until an operator clears it.
    pub degraded: Option<String>,
    /// No unit kind of the host meets the floor of any source: no source
    /// gets an offer.
    pub no_tier_served: bool,
    /// Why no unit kind meets the floor: one entry for each kind of each
    /// source. Absent when empty. [`Status::check`] refuses entries when
    /// `no_tier_served` is false; a writer that knows no cause leaves the
    /// list empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub no_tier_served_causes: Vec<NoTierCause>,
    pub sources: Vec<SourceStatus>,
    pub kinds: Vec<KindStatus>,
    pub pins: Vec<PinStatus>,
    /// The jobs that ended without a `completed` event of their source.
    pub ended_without_completion: u64,
    /// The reports that a source refused, in the spool.
    pub spooled_reports: usize,
    /// The most units of the host across its kinds; `null` is no cap.
    #[serde(default)]
    pub max_units: Option<u32>,
    /// The units that run on the host now.
    #[serde(default)]
    pub units: u32,
    /// The warm units of the host now, across its kinds, booting and ready.
    #[serde(default)]
    pub warm_units: u32,
}

impl Status {
    /// The rules that serde does not check: the version, the causes of
    /// `no_tier_served`, and the record of each kind.
    pub fn check(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("schema {} is not {SCHEMA}", self.schema));
        }
        if !self.no_tier_served && !self.no_tier_served_causes.is_empty() {
            return Err("no_tier_served_causes needs no_tier_served".into());
        }
        for kind in &self.kinds {
            kind.probe.check().map_err(|e| format!("the probe of the kind {:?}: {e}", kind.name))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{NoTierCause, Status};

    fn example(name: &str) -> String {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/examples/status/");
        std::fs::read_to_string(format!("{path}{name}")).expect("an example")
    }

    #[test]
    fn a_status_with_no_draining_max_units_or_units_reads_with_the_defaults() {
        let s: Status = serde_json::from_str(&example("valid-running.json")).unwrap();
        assert!(!s.sources[0].draining);
        assert_eq!((s.max_units, s.units), (None, 0));
        assert_eq!((s.kinds[0].warm.as_ref(), s.warm_units), (None, 0));
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.contains("\"warm\""), "{text}");
    }

    #[test]
    fn the_warm_units_of_a_kind_read_and_write_back_the_same() {
        let s: Status = serde_json::from_str(&example("valid-warm.json")).unwrap();
        let warm = s.kinds[0].warm.as_ref().expect("the warm units of the kind");
        assert_eq!((warm.target, warm.booting, warm.ready), (2, 1, 0));
        assert_eq!(warm.paused_reason.as_deref(), Some("the disk of the host is low"));
        assert_eq!(s.warm_units, 1);
        assert_eq!(s.check(), Ok(()));
        let again: Status = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(again, s);
        let mut running = s.clone();
        running.kinds[0].warm.as_mut().expect("warm").paused_reason = None;
        let text = serde_json::to_string(&running).unwrap();
        assert!(!text.contains("paused_reason"), "{text}");
    }

    #[test]
    fn the_causes_of_no_tier_served_read_and_write_back_the_same() {
        let s: Status = serde_json::from_str(&example("valid-no-tier-causes.json")).unwrap();
        assert_eq!(s.check(), Ok(()));
        assert_eq!(s.no_tier_served_causes.len(), 2);
        assert_eq!(s.no_tier_served_causes[0].cause, "guest-self-test-failed");
        assert_eq!(s.no_tier_served_causes[1].fix, None);
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(text.matches("\"fix\"").count(), 1, "{text}");
        let again: Status = serde_json::from_str(&text).unwrap();
        assert_eq!(again, s);
    }

    #[test]
    fn a_status_with_no_causes_writes_no_list_and_causes_need_the_flag() {
        let mut s: Status = serde_json::from_str(&example("valid-degraded.json")).unwrap();
        assert!(s.no_tier_served_causes.is_empty());
        assert!(!serde_json::to_string(&s).unwrap().contains("no_tier_served_causes"));
        let cause = NoTierCause { source: "a".into(), kind: "k".into(), cause: "c".into(), fix: None };
        s.no_tier_served_causes.push(cause);
        assert_eq!(s.check(), Ok(()));
        s.no_tier_served = false;
        assert_eq!(s.check(), Err("no_tier_served_causes needs no_tier_served".into()));
    }

    #[test]
    fn the_status_fields_of_capacity_read_and_write_back_the_same() {
        let s: Status = serde_json::from_str(&example("valid-capacity.json")).unwrap();
        assert!(s.sources[0].draining);
        assert_eq!((s.max_units, s.units), (Some(4), 1));
        let again: Status = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(again, s);
    }
}
