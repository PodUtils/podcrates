//! `run-report.v1` and `run-report-public.v1`: the end of one job. A source
//! that is not `operator_controlled` gets only the public report. The
//! public report never names the host, and it holds no detail of the
//! boundary (the proof, the principal, the hold, the unit id, the payload,
//! the residue): only the class of the boundary.

use std::collections::BTreeMap;

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
    /// The unit ended the job over a bound of its limits (disk now).
    Limit,
    /// Refused before a unit started: the report has `refusal_code`.
    Refused,
    /// Withdrawn at `wait_deadline_s`.
    Withdrawn,
    /// The runner name or the job id of the `started` event did not agree.
    JobMismatch,
    /// The runner lost the job (a restart from `claimed` on).
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

/// A count (of `destroy`, or of the use of a unit), or `unknown`.
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
#[cfg_attr(any(test, feature = "schema"), schemars(transform = crate::json_schema::run_report_rules))]
#[serde(deny_unknown_fields)]
pub struct RunReport {
    /// The version of the record: `SCHEMA`, 1.
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::version"))]
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
    /// The CPU time of the processes of the unit, user and system, from its
    /// start to its destroy, in seconds: a count, or `unknown` when the host
    /// cannot measure it (never 0 for unknown). Absent when no unit ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_s: Option<Residual>,
    /// The highest resident memory of the processes of the unit during the
    /// run, in MiB: a count, or `unknown`. Absent when no unit ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_rss_mib: Option<Residual>,
    /// The time from the claim to the hook of the job's first step, in
    /// seconds; absent when no step started. Not in the public report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_to_first_step_s: Option<u64>,
    /// The digests of the images of the job (`sha256:` and 64 lower-case
    /// hex digits). Not in the public report.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::digests"))]
    pub job_images: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_code: Option<RefusalCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<UnitFields>,
    /// The architecture of the unit: `x86_64` or `aarch64` (G-05, G-06).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::arch_or_none"))]
    pub arch: Option<String>,
    /// The real job that the runner got, when it is not the run of the
    /// offer, and the answer of the source (G-15).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned: Option<crate::Assigned>,
    /// The digest of the image of the unit (`sha256:HEX`; G-19).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::digest"))]
    pub image_digest: Option<String>,
    /// The version of the runner in the unit (G-19).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_version: Option<String>,
    /// The phases of the unit, in seconds, each rounded up as `cpu_s`: the
    /// prepare, the start, the run of the job and the destroy (G-19).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepare_s: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_s: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_s: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destroy_s: Option<u64>,
    /// The highest use of the disk of the unit, in bytes (G-19, G-18).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_peak_bytes: Option<Residual>,
    /// The bytes of the network of the unit, through its egress module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_rx_bytes: Option<Residual>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_tx_bytes: Option<Residual>,
    /// The bytes that the job read from and wrote to its cache (G-20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_bytes: Option<Residual>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_written_bytes: Option<Residual>,
    /// For each measured field that is `unknown`: why, in one line. A kind
    /// that cannot measure a fact says `unknown` with a reason, never 0.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unknown_reasons: BTreeMap<String, String>,
    /// The end of the log of a unit that failed before the runner
    /// connected: UTF-8, at most [`LOG_TAIL_MAX`] bytes, each credential
    /// scrubbed by the writer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_tail: Option<String>,
}

/// The most bytes of `log_tail`.
pub const LOG_TAIL_MAX: usize = 65_536;

/// The fields of a run report that are a count or `unknown`.
pub const MEASURED: [&str; 7] = [
    "cpu_s",
    "peak_rss_mib",
    "disk_peak_bytes",
    "net_rx_bytes",
    "net_tx_bytes",
    "cache_read_bytes",
    "cache_written_bytes",
];

/// The public report (`run-report-public.v1`). It never names the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[cfg_attr(any(test, feature = "schema"), schemars(transform = crate::json_schema::public_report_rules))]
#[serde(deny_unknown_fields)]
pub struct PublicReport {
    /// The version of the record: `SCHEMA`, 1.
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::version"))]
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
    /// The CPU time of the processes of the unit, user and system, from its
    /// start to its destroy, in seconds: a count, or `unknown` when the host
    /// cannot measure it (never 0 for unknown). Absent when no unit ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_s: Option<Residual>,
    /// The highest resident memory of the processes of the unit during the
    /// run, in MiB: a count, or `unknown`. Absent when no unit ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_rss_mib: Option<Residual>,
    pub exit_reason: ExitReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_code: Option<RefusalCode>,
}

impl RunReport {
    /// The rules that serde does not check: a refused report has a refusal
    /// code and no unit; a report of a unit that ran has no refusal code.
    pub fn check(&self) -> Result<(), String> {
        if let Some(image) = self.job_images.iter().find(|i| !crate::offer::is_digest(i)) {
            return Err(format!("the job image {image:?} is not sha256:HEX"));
        }
        if let Some(arch) = self.arch.as_deref().filter(|a| !crate::ARCHES.contains(a)) {
            return Err(format!("arch {arch:?} is not x86_64 or aarch64"));
        }
        if let Some(image) = self.image_digest.as_deref().filter(|i| !crate::offer::is_digest(i)) {
            return Err(format!("the image digest {image:?} is not sha256:HEX"));
        }
        if self.log_tail.as_ref().is_some_and(|t| t.len() > LOG_TAIL_MAX) {
            return Err(format!("log_tail has more than {LOG_TAIL_MAX} bytes"));
        }
        if let Some(field) =
            self.unknown_reasons.keys().find(|f| self.measured(f) != Some(Residual::Unknown(Unknown::Unknown)))
        {
            return Err(format!("unknown_reasons names {field:?}, which is not an unknown measured field"));
        }
        rules(self.schema, self.exit_reason, self.refusal_code.is_some(), self.unit.is_some())
    }

    /// The measured field `name` of [`MEASURED`], where the report has it.
    fn measured(&self, name: &str) -> Option<Residual> {
        match name {
            "cpu_s" => self.cpu_s,
            "peak_rss_mib" => self.peak_rss_mib,
            "disk_peak_bytes" => self.disk_peak_bytes,
            "net_rx_bytes" => self.net_rx_bytes,
            "net_tx_bytes" => self.net_tx_bytes,
            "cache_read_bytes" => self.cache_read_bytes,
            "cache_written_bytes" => self.cache_written_bytes,
            _ => None,
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
            cpu_s: self.cpu_s,
            peak_rss_mib: self.peak_rss_mib,
            exit_reason: self.exit_reason,
            refusal_code: self.refusal_code,
        }
    }
}

impl PublicReport {
    /// The rules of [`RunReport::check`] on the public fields: `unit_kind`
    /// and `boundary` stand for the unit. The public report of a report
    /// that passes its check passes this check.
    pub fn check(&self) -> Result<(), String> {
        let unit = self.unit_kind.is_some() || self.boundary.is_some();
        rules(self.schema, self.exit_reason, self.refusal_code.is_some(), unit)
    }
}

/// The rules of a report that serde does not check: the version; a refused
/// report has a refusal code and no unit; a report of a unit that ran has
/// no refusal code; a withdrawn report has no unit.
fn rules(schema: u32, exit: ExitReason, code: bool, unit: bool) -> Result<(), String> {
    if schema != SCHEMA {
        return Err(format!("schema {schema} is not {SCHEMA}"));
    }
    match (exit, code, unit) {
        (ExitReason::Refused, false, _) => Err("a refused report needs refusal_code".into()),
        (ExitReason::Refused, true, true) => Err("a refused report has no unit fields".into()),
        (ExitReason::Refused, true, false) => Ok(()),
        (_, true, _) => Err("refusal_code needs exit_reason refused".into()),
        (ExitReason::Withdrawn, false, true) => Err("a withdrawn report has no unit fields".into()),
        _ => Ok(()),
    }
}
