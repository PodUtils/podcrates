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
