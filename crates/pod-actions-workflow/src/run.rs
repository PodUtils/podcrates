//! The types of the expansion: the results of jobs, the concrete jobs and
//! the skips.

use std::fmt;

use pod_actions_expr::{Limits, Value};

use crate::call::{Call, CallLimits, Secrets};

/// The contexts of a run that do not change: `github` (a local stand-in
/// that the caller makes), `vars` and `inputs`. Each is an empty object by
/// default. `calls` bounds the calls of reusable workflows.
#[derive(Clone, Debug)]
pub struct Base {
    pub github: Value,
    pub vars: Value,
    pub inputs: Value,
    pub limits: Limits,
    pub calls: CallLimits,
}

impl Default for Base {
    fn default() -> Base {
        let empty = || Value::object(Vec::<(String, Value)>::new());
        Base {
            github: empty(),
            vars: empty(),
            inputs: empty(),
            limits: Limits::default(),
            calls: CallLimits::default(),
        }
    }
}

/// The result of a job, as `needs.<id>.result` gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobResult {
    Success,
    Failure,
    Cancelled,
    Skipped,
}

impl JobResult {
    pub fn as_str(self) -> &'static str {
        match self {
            JobResult::Success => "success",
            JobResult::Failure => "failure",
            JobResult::Cancelled => "cancelled",
            JobResult::Skipped => "skipped",
        }
    }

    /// The result of a job from the results of its cells: a failure if one
    /// failed; else cancelled if one was cancelled; else success if one
    /// succeeded; else skipped.
    pub fn combine(results: impl IntoIterator<Item = JobResult>) -> JobResult {
        let all: Vec<JobResult> = results.into_iter().collect();
        for kind in [JobResult::Failure, JobResult::Cancelled, JobResult::Success] {
            if all.contains(&kind) {
                return kind;
            }
        }
        JobResult::Skipped
    }
}

impl fmt::Display for JobResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The end of a job: its result and its outputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    pub result: JobResult,
    pub outputs: Vec<(String, String)>,
}

impl Done {
    /// A result with no output.
    pub fn of(result: JobResult) -> Done {
        Done { result, outputs: Vec::new() }
    }
}

/// `runs-on`, evaluated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunsOn {
    pub group: Option<String>,
    pub labels: Vec<String>,
}

/// One concrete job: one cell of the matrix of a job, or the job with no
/// matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    /// The id of the job: `CALLER/CALLEE` for a called job.
    pub job: String,
    /// The name to show: `name` evaluated, or the id; the values of the
    /// cell follow in parentheses where `name` does not read `matrix`. A
    /// called job has the name of its caller first: `CALLER / CALLEE`.
    pub name: String,
    /// The cell: the `matrix` context; null with no matrix.
    pub matrix: Value,
    /// The `strategy` context: `fail-fast`, `job-index`, `job-total`,
    /// `max-parallel`.
    pub strategy: Value,
    /// The `needs` context of the job.
    pub needs: Value,
    /// `runs-on`, evaluated; none for a reusable workflow.
    pub runs_on: Option<RunsOn>,
    /// The reusable workflow that the job calls, for a run with no
    /// [`Called`](crate::Called) source: named, not expanded.
    pub uses: Option<String>,
    /// `timeout-minutes`, evaluated: 360 by default.
    pub timeout_minutes: f64,
    /// `continue-on-error`, evaluated: false by default.
    pub continue_on_error: bool,
    /// The `inputs` context of the job: the run's, or the inputs of the
    /// call for a called job.
    pub inputs: Value,
    /// The secrets that the job may read.
    pub secrets: Secrets,
}

/// A job that does not run, with its reason: its condition was false.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skip {
    pub job: String,
    /// The condition that the runner evaluated, with its implicit
    /// `success() &&`.
    pub condition: String,
    /// The result of each job before it (each need, and their needs), in
    /// the topological order: the input of the status functions.
    pub ancestors: Vec<(String, JobResult)>,
    /// The run was cancelled.
    pub cancelled: bool,
}

impl fmt::Display for Skip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the job `{}` is skipped: `{}` is false", self.job, self.condition)?;
        let not_ok: Vec<String> = self
            .ancestors
            .iter()
            .filter(|(_, r)| *r != JobResult::Success)
            .map(|(id, r)| format!("{id} {r}"))
            .collect();
        if !not_ok.is_empty() {
            write!(f, " ({})", not_ok.join(", "))?;
        }
        if self.cancelled {
            f.write_str(" (the run is cancelled)")?;
        }
        Ok(())
    }
}

/// What a job becomes when its needs are done.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Its concrete jobs, in the order of the cells.
    Run(Vec<Instance>),
    /// Its condition was false: it ends as `skipped`.
    Skipped(Skip),
    /// It calls a reusable workflow: one call for each cell. The called
    /// jobs are now jobs of the run; the job ends when they end.
    Called(Vec<Call>),
}
