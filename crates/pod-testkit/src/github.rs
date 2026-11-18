//! A fake GitHub on the loopback for the CI of a consumer that drives a
//! GitHub App and its runners with no account (feature `github`; the binary
//! `pod-fake-github`). One port serves four parts:
//!
//! * the App: an RS256 JWT of [`APP_ID`], signed with the made-up key
//!   ([`key::MADE_UP_KEY_PEM`]), gets an installation token
//!   (`GET /repos/OWNER/REPO/installation`,
//!   `POST /app/installations/ID/access_tokens`);
//! * the runners of a repository with that token:
//!   `POST /repos/OWNER/REPO/actions/runners/generate-jitconfig` (a runner
//!   id and an `encoded_jit_config`), `GET .../actions/runners`,
//!   `DELETE .../actions/runners/ID`, `GET .../actions/jobs/ID`;
//! * the runner broker of each minted configuration (the token of its
//!   OAuth client, the session, the long poll of messages, `acquirejob`,
//!   `renewjob`, `completejob` and the results service of the logs), the
//!   routes that the official runner calls;
//! * the `workflow_job` webhooks `queued`, `in_progress` and `completed`,
//!   signed with the webhook secret (`X-Hub-Signature-256`), to the URL of
//!   the hook;
//! * the control API of the test under `/_fake/` (module `control`): queue a
//!   job, give a queued job to a runner, read the jobs and their
//!   conclusions.
//!
//! A runner takes the job that the control API gave it, else the oldest
//! queued job of its repository whose labels it has, as GitHub gives a job
//! to any idle runner with its labels. Each runner is ephemeral: one job.
//! Each credential is made up: the key, the webhook secret, and the random
//! tokens of each run.

mod api;
mod broker;
mod control;
mod hook;
pub mod key;
mod message;

use std::io;
use std::net::TcpListener;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, mpsc};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::fake::{body, json_answer};
use crate::stand::{Answer, Request, Stand};
pub use key::{Key, hub_signature};

/// The id of the one App of the fake: the `iss` of its JWT.
pub const APP_ID: u64 = 1;
/// The id of the one installation of the App; it holds each repository.
pub const INSTALLATION_ID: u64 = 1;
/// The webhook secret until the control API sets another.
pub const MADE_UP_WEBHOOK_SECRET: &str = "pod-fake-github-made-up-webhook-secret";

/// A running fake GitHub.
pub struct FakeGithub {
    stand: Stand,
    shared: Arc<Shared>,
}

/// What the routes share.
pub(crate) struct Shared {
    /// `http://ADDR/`: the base of each URL that the fake gives.
    base: String,
    key: Key,
    state: Mutex<State>,
    /// A change of the jobs: a long poll of a runner looks again.
    changed: Condvar,
    hooks: Mutex<mpsc::Sender<hook::Delivery>>,
}

#[derive(Default)]
pub(crate) struct State {
    hook_url: Option<String>,
    hook_secret: String,
    /// The installation tokens that the fake gave.
    tokens: Vec<String>,
    jobs: Vec<Job>,
    runners: Vec<Runner>,
    /// The last id of a job or a runner: one count for both.
    last_id: u64,
    /// Each webhook: its action, its job and the status of its delivery.
    deliveries: Vec<Value>,
}

/// Where a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Queued,
    /// A runner got its message and did not acquire it yet: still queued
    /// for GitHub.
    Given,
    InProgress,
    Completed,
}

pub(crate) struct Job {
    id: u64,
    run_id: u64,
    repo: String,
    name: String,
    labels: Vec<String>,
    /// The script of each `run:` step.
    steps: Vec<String>,
    status: Status,
    /// The runner that the control API gave the job to.
    assigned: Option<u64>,
    runner: Option<u64>,
    /// The job token of the job message: the Worker sends it on each call.
    token: String,
    /// The conclusion as the runner sent it (`succeeded`), and as GitHub
    /// gives it (`success`).
    reported: Option<String>,
    conclusion: Option<String>,
    created_at: u64,
    started_at: Option<u64>,
    completed_at: Option<u64>,
}

pub(crate) struct Runner {
    id: u64,
    name: String,
    repo: String,
    labels: Vec<String>,
    /// The client id of its OAuth client, the `sub` of its assertion.
    client_id: String,
    /// The access token of its OAuth client.
    token: Option<String>,
    job: Option<u64>,
    /// It ended its one job, or the API removed it.
    gone: bool,
}

impl FakeGithub {
    /// A fake on a free port of 127.0.0.1.
    pub fn start() -> io::Result<FakeGithub> {
        FakeGithub::start_on(TcpListener::bind("127.0.0.1:0")?)
    }

    /// A fake on the caller's listener.
    pub fn start_on(listener: TcpListener) -> io::Result<FakeGithub> {
        let base = format!("http://{}/", listener.local_addr()?);
        let (send, receive) = mpsc::channel();
        let state = State { hook_secret: MADE_UP_WEBHOOK_SECRET.to_string(), ..State::default() };
        let shared = Arc::new(Shared {
            base,
            key: Key::made_up(),
            state: Mutex::new(state),
            changed: Condvar::new(),
            hooks: Mutex::new(send),
        });
        hook::sender(Arc::downgrade(&shared), receive);
        let routes = shared.clone();
        let stand = Stand::http_on(listener, move |req| route(&routes, req))?;
        Ok(FakeGithub { stand, shared })
    }

    /// The base of each URL of the fake, `http://127.0.0.1:PORT/`.
    pub fn base(&self) -> &str {
        &self.shared.base
    }

    /// The HTTP stand-in under the fake: its address and its bytes.
    pub fn stand(&self) -> &Stand {
        &self.stand
    }

    /// Send each webhook to `url` from now on.
    pub fn set_hook(&self, url: &str) {
        self.shared.lock().hook_url = Some(url.to_string());
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl State {
    fn next_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }

    fn job(&mut self, id: u64) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    fn runner(&mut self, id: u64) -> Option<&mut Runner> {
        self.runners.iter_mut().find(|r| r.id == id)
    }
}

/// Each request: the control API, the API of GitHub, the broker.
fn route(shared: &Shared, req: &Request) -> Answer {
    let path = req.path().to_string();
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    control::route(shared, req, &parts)
        .or_else(|| api::route(shared, req, &parts))
        .or_else(|| broker::route(shared, req, &path))
        .unwrap_or_else(|| error(404, "Not Found"))
}

/// An error in the form of the REST API of GitHub.
fn error(status: u16, message: &str) -> Answer {
    json_answer(status, &json!({ "message": message, "documentation_url": "https://docs.github.com/rest" }))
}

/// The credential of `Authorization: Bearer X` or `Authorization: token X`.
fn credential(req: &Request) -> Option<&str> {
    let (scheme, value) = req.header("authorization")?.trim().split_once(' ')?;
    let known = scheme.eq_ignore_ascii_case("bearer") || scheme.eq_ignore_ascii_case("token");
    known.then(|| value.trim())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// An RFC 3339 UTC time of `secs` since the epoch, as GitHub writes it.
fn rfc3339(secs: u64) -> String {
    let secs = secs as i64;
    let (y, m, d) = pod_time::civil::civil(secs.div_euclid(pod_time::civil::DAY));
    let rest = secs.rem_euclid(pod_time::civil::DAY);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// The labels of a JSON array of strings; none for another value.
fn labels(v: &Value) -> Option<Vec<String>> {
    v.as_array()?.iter().map(|l| l.as_str().map(str::to_string)).collect()
}

/// Whether a runner with the labels `have` can take a job of `want`: GitHub
/// compares labels with no case.
fn has_labels(have: &[String], want: &[String]) -> bool {
    want.iter().all(|w| have.iter().any(|h| h.eq_ignore_ascii_case(w)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_times_are_in_the_form_of_github() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_795_000_000), "2026-11-18T11:06:40Z");
    }

    #[test]
    fn a_runner_takes_a_job_only_with_each_label() {
        let have = ["self-hosted".to_string(), "Linux".to_string()];
        assert!(has_labels(&have, &["linux".to_string()]));
        assert!(has_labels(&have, &[]));
        assert!(!has_labels(&have, &["linux".to_string(), "gpu".to_string()]));
    }
}
