//! The link of `pod-fake-githome serve --github URL` to a `pod-fake-github`,
//! so that a runner of a minted job takes that job from the broker of the
//! fake GitHub:
//!
//! * a queued job is also queued in the fake GitHub (its control API), in
//!   the repository of the offer, with the labels of the offer and the
//!   `steps` of the control API (the fake GitHub's default `run:` step when
//!   there are none);
//! * `mint` acts as the App: it mints a JIT runner with the runner name of
//!   the claim and the labels of the offer, gives that job to that runner
//!   (the control API: else the runner takes the oldest queued job of its
//!   labels), and answers the files of `encoded_jit_config`.

use pod_job::JobOffer;
use pod_job::source::{Hidden, MintedFile};
use serde_json::{Value, json};

use crate::github::client::{app_key, call_expect, installation_token, jit_runner};

/// The labels of the job and of its runner in the fake GitHub: the labels
/// of the offer, else `self-hosted`, as GitHub mints no runner with no
/// label.
fn labels(offer: &JobOffer) -> Vec<String> {
    if offer.labels.is_empty() { vec!["self-hosted".to_string()] } else { offer.labels.clone() }
}

/// `OWNER/REPO` of the offer.
fn repository(offer: &JobOffer) -> String {
    format!("{}/{}", offer.trust_inputs.owner, offer.trust_inputs.repo)
}

/// A call of the control API of the fake GitHub at `github` that must
/// answer `want`; the JSON of the answer.
fn control(github: &str, method: &str, path: &str, body: &Value, want: u16) -> Result<Value, String> {
    call_expect(method, &format!("{github}_fake/{path}"), None, body, want).map_err(|e| format!("the fake GitHub: {e}"))
}

/// Queue the job of `offer` in the fake GitHub, with the scripts `steps`
/// (`Null` for the default step); the id of the job there.
pub(super) fn queue(github: &str, offer: &JobOffer, steps: &Value) -> Result<u64, String> {
    let job = json!({
        "repository": repository(offer), "name": offer.job_id, "labels": labels(offer),
        "run_id": offer.trust_inputs.run_id, "steps": steps,
    });
    let v = control(github, "POST", "jobs", &job, 201)?;
    v["id"].as_u64().ok_or_else(|| format!("the fake GitHub gives no id of the job: {v}"))
}

/// A JIT runner named `runner_name` for the job `github_job` of the fake
/// GitHub, which gets that job: the id of the runner and its files.
pub(super) fn mint(
    github: &str,
    offer: &JobOffer,
    github_job: u64,
    runner_name: &str,
) -> Result<(u64, Vec<MintedFile>), String> {
    let repo = repository(offer);
    let token = installation_token(github, &app_key(github)?, &repo)?;
    let runner = jit_runner(github, &token, &repo, runner_name, &labels(offer))?;
    control(github, "POST", &format!("jobs/{github_job}/assign"), &json!({ "runner_id": runner.id }), 200)?;
    let files = runner.files.into_iter().map(|(name, text)| MintedFile { name, content: Hidden::new(text) }).collect();
    Ok((runner.id, files))
}
