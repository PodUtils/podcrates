//! The ids of a run and of a job in the runtime token of a step.
//!
//! `actions/upload-artifact@v4` and `actions/download-artifact@v4` send
//! `workflow_run_backend_id` and `workflow_job_run_backend_id` in each call.
//! They read both from the scope `Actions.Results:RUN:JOB` of the claim
//! `scp` of `ACTIONS_RUNTIME_TOKEN` (a JWT; the client checks no
//! signature). A broker or a forge that mints the token puts the scope in
//! it ([`results_scope`]); [`backend_ids`] reads it as the client does.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use serde_json::Value;

/// The first part of the scope of the results API.
pub const RESULTS_SCOPE: &str = "Actions.Results";

/// The ids of a run and of a job in the results API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendIds {
    /// `workflow_run_backend_id`.
    pub run: String,
    /// `workflow_job_run_backend_id`.
    pub job: String,
}

/// The scope that names `run` and `job`: `Actions.Results:RUN:JOB`. It
/// goes in the claim `scp` of the runtime token, after a space.
pub fn results_scope(run: &str, job: &str) -> String {
    format!("{RESULTS_SCOPE}:{run}:{job}")
}

/// The ids of the scope of the results API in the claim `scp` of the JWT
/// `token`. The first scope whose first part is `Actions.Results` gives
/// them; it must have three parts, else there are none.
pub fn backend_ids(token: &str) -> Option<BackendIds> {
    let claims = token.split('.').nth(1)?.trim_end_matches('=');
    let bytes = URL_SAFE_NO_PAD.decode(claims).or_else(|_| STANDARD_NO_PAD.decode(claims)).ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    let scope = claims["scp"].as_str()?.split(' ').find(|s| s.split(':').next() == Some(RESULTS_SCOPE))?;
    match scope.split(':').collect::<Vec<_>>()[..] {
        [_, run, job] => Some(BackendIds { run: run.to_string(), job: job.to_string() }),
        _ => None,
    }
}
