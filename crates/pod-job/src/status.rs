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
}

impl Status {
    /// The rules that serde does not check: the version, and the record of
    /// each kind.
    pub fn check(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("schema {} is not {SCHEMA}", self.schema));
        }
        for kind in &self.kinds {
            kind.probe.check().map_err(|e| format!("the probe of the kind {:?}: {e}", kind.name))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Status;

    fn example(name: &str) -> String {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/examples/status/");
        std::fs::read_to_string(format!("{path}{name}")).expect("an example")
    }

    #[test]
    fn a_status_with_no_draining_max_units_or_units_reads_with_the_defaults() {
        let s: Status = serde_json::from_str(&example("valid-running.json")).unwrap();
        assert!(!s.sources[0].draining);
        assert_eq!((s.max_units, s.units), (None, 0));
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
