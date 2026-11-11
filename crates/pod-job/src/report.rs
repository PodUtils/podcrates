//! `run-report.v1` and `run-report-public.v1`: the end of one job. A source
//! that is not `operator_controlled` gets only the public report, which
//! never names the host.

use serde::{Deserialize, Serialize};

use crate::{Boundary, EgressHold, JobId, Principal, RefusalCode, SCHEMA, Tier, UnitId};

/// How a job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ExitReason {
    Success,
    Failure,
    Cancelled,
    Timeout,
    /// Refused before a unit started: the report has `refusal_code`.
    Refused,
    /// Withdrawn at `wait_deadline_s`.
    Withdrawn,
    /// The runner name or the job id of the `started` event did not match.
    JobMismatch,
    /// podrunner lost the job (a restart from `claimed` on).
    Lost,
    UnitFailed,
}

/// The unit of a job that ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UnitFields {
    pub unit_id: UnitId,
    pub unit_kind: String,
    pub boundary: Boundary,
    pub principal: Principal,
    pub egress_hold: EgressHold,
    /// The digest of the payload.
    pub payload: String,
    /// The processes and files that `destroy` left; `unknown` when it
    /// could not count.
    pub residual_processes: Residual,
    pub residual_files: Residual,
}

/// A count of `destroy`, or `unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum Residual {
    Count(u64),
    Unknown(Unknown),
}

/// The word `unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Unknown {
    Unknown,
}

/// The full report (`run-report.v1`), for the operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RunReport {
    pub schema: u32,
    pub job_id: JobId,
    pub source: String,
    /// The tier, when the tier rule gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<Tier>,
    /// The name of the host.
    pub host: String,
    pub exit_reason: ExitReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// The time from the claim to the report, in seconds.
    pub wall_s: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_code: Option<RefusalCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<UnitFields>,
}

/// The public report (`run-report-public.v1`). It never names the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PublicReport {
    pub schema: u32,
    pub job_id: JobId,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<Tier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Boundary>,
    pub wall_s: u64,
    pub exit_reason: ExitReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_code: Option<RefusalCode>,
}

impl RunReport {
    /// The rules that serde does not check: a refused report has a refusal
    /// code and no unit; a report of a unit that ran has no refusal code.
    pub fn check(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("schema {} is not {SCHEMA}", self.schema));
        }
        match (self.exit_reason, &self.refusal_code, &self.unit) {
            (ExitReason::Refused, None, _) => Err("a refused report needs refusal_code".into()),
            (ExitReason::Refused, Some(_), Some(_)) => Err("a refused report has no unit fields".into()),
            (ExitReason::Refused, Some(_), None) => Ok(()),
            (_, Some(_), _) => Err("refusal_code needs exit_reason refused".into()),
            (ExitReason::Withdrawn, None, Some(_)) => Err("a withdrawn report has no unit fields".into()),
            _ => Ok(()),
        }
    }

    /// The subset for a source that is not `operator_controlled`.
    pub fn public(&self) -> PublicReport {
        PublicReport {
            schema: SCHEMA,
            job_id: self.job_id.clone(),
            source: self.source.clone(),
            tier: self.tier,
            unit_kind: self.unit.as_ref().map(|u| u.unit_kind.clone()),
            boundary: self.unit.as_ref().map(|u| u.boundary),
            wall_s: self.wall_s,
            exit_reason: self.exit_reason,
            refusal_code: self.refusal_code,
        }
    }
}
