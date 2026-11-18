//! The events of `next`, and the real job of `assigned`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::JobId;

/// A job offer. The fields other than `job_id` are those of `job-offer.v1`
/// ([`crate::JobOffer`]); the wire keeps them as an object, and podrunner
/// reads them with `JobOffer` after the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
pub struct Offer {
    pub job_id: JobId,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

/// An event of a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[cfg_attr(any(test, feature = "schema"), schemars(rename = "SourceEvent"))]
#[serde(tag = "event", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Event {
    /// A job that podrunner can take.
    Offer { offer: Offer },
    /// The forge cancelled a job.
    Cancel { job_id: JobId },
    /// The forge says that a job ended.
    Completed { job_id: JobId, result: String },
    /// The forge says that a runner took a job. podrunner compares the
    /// runner name with the one of the unit (`job-mismatch`).
    Started { job_id: JobId, runner_name: String },
}

impl Event {
    pub fn job_id(&self) -> &JobId {
        match self {
            Event::Offer { offer } => &offer.job_id,
            Event::Cancel { job_id } | Event::Completed { job_id, .. } | Event::Started { job_id, .. } => job_id,
        }
    }
}

/// The facts of the job that the forge gave to a runner, as the job-started
/// hook of the runner reads them (githome's G-15). The forge, not the
/// source, picks the queued job that a runner takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RealJob {
    pub run_id: u64,
    /// The key of the job in its workflow (`GITHUB_JOB`).
    pub job_name: String,
    pub event: String,
    /// `OWNER/REPO`.
    pub repository: String,
    /// `OWNER/REPO` of the head of a pull request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_is_fork: Option<bool>,
    pub actor: String,
    /// The numeric id of the job at the forge, when the hook knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<u64>,
}
