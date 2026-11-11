//! `job-offer.v1`: a job that a source offers. It has no `tier` field:
//! podrunner computes the tier from `trust_inputs` (GitHub #18).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Boundary, Fact, JobId, SCHEMA};

/// The event that started the workflow. Each value that the tier rule does
/// not name is `other`; a value that the source cannot read is `unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Push,
    PullRequest,
    PullRequestTarget,
    WorkflowDispatch,
    Schedule,
    MergeGroup,
    WorkflowRun,
    IssueComment,
    Release,
    RepositoryDispatch,
    Other,
    Unknown,
}

/// The relation of the actor to the organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ActorClass {
    Member,
    Outside,
    Unknown,
}

/// The visibility of the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    Private,
    Internal,
    Public,
    Unknown,
}

/// The job metadata that the tier rule reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TrustInputs {
    pub event: Event,
    /// `yes` when the head of a pull request is in a fork.
    pub head_is_fork: Fact,
    pub actor_class: ActorClass,
    pub repo_visibility: Visibility,
    pub owner: String,
    pub repo: String,
    pub run_id: u64,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub workflow_ref: String,
    /// The numeric ids, when the source gives them: the intake check
    /// compares them too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<u64>,
}

/// What the job needs of a unit, beyond its tier.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UnitNeed {
    /// A boundary above the minimum of the tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_boundary: Option<Boundary>,
    #[serde(default)]
    pub container: bool,
    #[serde(default)]
    pub kvm: bool,
}

/// A job offer (`job-offer.v1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct JobOffer {
    pub schema: u32,
    pub job_id: JobId,
    /// The name of the source in the configuration.
    pub source: String,
    pub trust_inputs: TrustInputs,
    pub labels: Vec<String>,
    /// `x86_64` or `aarch64`.
    pub arch: String,
    /// The limits that the job asks for, by name (`wall` in seconds,
    /// `memory` in bytes, `cpu` in milli-cores, `pids`, `disk` in bytes).
    #[serde(default)]
    pub limits: BTreeMap<String, u64>,
    /// The names of the limits that a unit must enforce.
    #[serde(default)]
    pub limits_required: Vec<String>,
    /// The egress policy (the `broker-policy.v1` schema of `pod-broker`).
    #[serde(default)]
    pub policy: Map<String, Value>,
    /// After this time, a job that waits for a slot is withdrawn.
    pub wait_deadline_s: u64,
    /// A payload digest, only from a source that may give one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_need: Option<UnitNeed>,
}

/// The names of the limits.
pub const LIMITS: [&str; 5] = ["wall", "cpu", "memory", "pids", "disk"];

impl JobOffer {
    /// The rules that serde does not check. `scaleset` is true for an offer
    /// of a `github-scaleset` source, which gives no payload.
    pub fn check(&self, scaleset: bool) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("schema {} is not {SCHEMA}", self.schema));
        }
        if !matches!(self.arch.as_str(), "x86_64" | "aarch64") {
            return Err("arch is not x86_64 or aarch64".into());
        }
        if let Some(name) = self.limits.keys().chain(&self.limits_required).find(|n| !LIMITS.contains(&n.as_str())) {
            return Err(format!("unknown limit {name:?}"));
        }
        if let Some(name) = self.limits_required.iter().find(|n| !self.limits.contains_key(*n)) {
            return Err(format!("the required limit {name:?} has no value"));
        }
        if scaleset && self.payload.is_some() {
            return Err("a github-scaleset source gives no payload".into());
        }
        if self.payload.as_deref().is_some_and(|p| !is_digest(p)) {
            return Err("the payload is not sha256:HEX".into());
        }
        Ok(())
    }
}

/// True for `sha256:` and 64 lower-case hex digits.
pub fn is_digest(s: &str) -> bool {
    s.strip_prefix("sha256:")
        .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
