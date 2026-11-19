//! The runner broker of each minted configuration: the routes that the
//! official runner calls with a JIT configuration (the token of its OAuth
//! client, the session, the long poll, the run service) and the results
//! service of the Worker's logs. The routes follow podrunner's fake broker.
//!
//! * The token of the OAuth client goes only to a client assertion that
//!   the made-up key signed, for the client id of a runner of the fake: the
//!   `sub` of the assertion names the runner.
//! * The session, the long poll and `acquirejob` take that token.
//! * `renewjob`, `completejob` and the results take the job token of the
//!   job message, or the runner's token.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Shared, State, Status, body, credential, has_labels, hook, json_answer, key, message, now};
use crate::stand::{Answer, Request};

/// How long a long poll of `/message` waits for a job.
const POLL: Duration = Duration::from_secs(2);

/// The answer of a route of the broker, or none for another path.
pub(super) fn route(shared: &Shared, req: &Request, path: &str) -> Option<Answer> {
    let refused = || Answer::new(401, "the fake gives this route only to its own runner");
    let answer = match (req.method.as_str(), path) {
        ("POST", "/_apis/oauth2/token") => grant(shared, &req.body),
        ("POST", "/session") => match runner(shared, req) {
            Some(id) => {
                let mut session = body(req);
                session["sessionId"] = json!(format!("11111111-1111-1111-1111-{id:012x}"));
                json_answer(200, &session)
            }
            None => refused(),
        },
        ("DELETE", "/session") => match runner(shared, req) {
            Some(_) => Answer::new(200, Vec::new()),
            None => refused(),
        },
        ("GET", "/message") => match runner(shared, req) {
            Some(id) => poll(shared, id),
            None => refused(),
        },
        ("POST", "/acquirejob") => match runner(shared, req) {
            Some(id) => acquire(shared, id, &body(req)),
            None => refused(),
        },
        ("POST", "/renewjob") => match job(shared, req) {
            Some(_) => json_answer(200, &json!({ "lockedUntil": "2099-01-01T00:00:00Z" })),
            None => refused(),
        },
        ("POST", "/completejob") => match job(shared, req) {
            Some(id) => complete(shared, id, &body(req)),
            None => refused(),
        },
        ("POST", p) if p.starts_with("/twirp/") => match job(shared, req) {
            Some(_) => results(shared, p),
            None => refused(),
        },
        // A log blob: the fake keeps no log.
        ("PUT", p) if p.starts_with("/_blobs/") => Answer::new(201, Vec::new()),
        _ => return None,
    };
    Some(answer)
}

/// The runner of the request. The lock ends in this function: a
/// guard in the scrutinee of a `match` lives until the end of the `match`,
/// and the route takes the lock again.
fn runner(shared: &Shared, req: &Request) -> Option<u64> {
    runner_of(&shared.lock(), req)
}

/// The job of the request; the lock ends in this function, as in [`runner`].
fn job(shared: &Shared, req: &Request) -> Option<u64> {
    job_of(&shared.lock(), req)
}

/// The runner of the bearer token of the request.
fn runner_of(st: &State, req: &Request) -> Option<u64> {
    let token = credential(req)?;
    st.runners.iter().find(|r| r.token.as_deref().is_some_and(|t| key::same(t, token))).map(|r| r.id)
}

/// The job of the job token of the request, or the job of the runner of its
/// token.
fn job_of(st: &State, req: &Request) -> Option<u64> {
    let token = credential(req)?;
    if let Some(job) = st.jobs.iter().find(|j| key::same(&j.token, token)) {
        return Some(job.id);
    }
    let runner = runner_of(st, req)?;
    st.runners.iter().find(|r| r.id == runner)?.job
}

/// The access token of a runner for the form of `oauth2/token`.
fn grant(shared: &Shared, form: &[u8]) -> Answer {
    let refused = || json_answer(401, &json!({ "error": "invalid_client" }));
    let Some(assertion) = std::str::from_utf8(form)
        .ok()
        .and_then(|f| f.split('&').find_map(|kv| kv.strip_prefix("client_assertion=")))
        .and_then(percent_decode)
    else {
        return refused();
    };
    let Some(claims) = shared.key.verify(&assertion) else { return refused() };
    let mut st = shared.lock();
    let Some(runner) = st.runners.iter_mut().find(|r| !r.gone && claims["sub"] == r.client_id.as_str()) else {
        return refused();
    };
    let token = runner.token.get_or_insert_with(|| message::job_token(&key::random_hex(32))).clone();
    json_answer(200, &json!({ "access_token": token, "token_type": "bearer", "expires_in": 3600 }))
}

/// The long poll of a runner: the message of its job, or 202 after
/// [`POLL`] with none.
fn poll(shared: &Shared, runner: u64) -> Answer {
    let deadline = Instant::now() + POLL;
    let mut st = shared.lock();
    loop {
        if let Some(job) = pick(&mut st, runner) {
            return json_answer(200, &message::job_request(&shared.base, job));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Answer::new(202, Vec::new());
        }
        st = shared.changed.wait_timeout(st, left).unwrap_or_else(|e| e.into_inner()).0;
    }
}

/// The job for the next message of `runner`: the job that it got and did
/// not acquire; else the queued job that the control API gave it; else the
/// oldest queued job of its repository with its labels that the control
/// API gave to no runner. An ephemeral runner gets one job.
fn pick(st: &mut State, runner: u64) -> Option<u64> {
    let r = st.runners.iter().find(|r| r.id == runner)?;
    if let Some(job) = r.job {
        return st.jobs.iter().any(|j| j.id == job && j.status == Status::Given).then_some(job);
    }
    if r.gone {
        return None;
    }
    let queued = |j: &&super::Job| j.status == Status::Queued;
    let job = st
        .jobs
        .iter()
        .filter(queued)
        .find(|j| j.assigned == Some(runner))
        .or_else(|| {
            st.jobs
                .iter()
                .filter(queued)
                .find(|j| j.assigned.is_none() && j.repo == r.repo && has_labels(&r.labels, &j.labels))
        })?
        .id;
    if let Some(j) = st.job(job) {
        j.status = Status::Given;
        j.runner = Some(runner);
    }
    if let Some(r) = st.runner(runner) {
        r.job = Some(job);
    }
    Some(job)
}

/// The job message of the runner's job; the job is in progress from its
/// first acquire.
fn acquire(shared: &Shared, runner: u64, req: &Value) -> Answer {
    let asked = req["jobMessageId"].as_str().and_then(|s| s.parse().ok()).or_else(|| req["jobMessageId"].as_u64());
    let mut st = shared.lock();
    let Some(job) = st.runners.iter().find(|r| r.id == runner).and_then(|r| r.job).filter(|j| asked == Some(*j)) else {
        return Answer::new(404, "the runner has no such job");
    };
    let Some(j) = st.job(job) else { return Answer::new(404, "the runner has no such job") };
    if j.status == Status::Given {
        j.status = Status::InProgress;
        j.started_at = Some(now());
        hook::send(shared, &mut st, "in_progress", job);
    }
    match st.jobs.iter().find(|j| j.id == job) {
        Some(j) => json_answer(200, &message::job_message(&shared.base, j)),
        None => Answer::new(404, "the runner has no such job"),
    }
}

/// The end of a job as the runner sends it. The runner ends with it.
fn complete(shared: &Shared, job: u64, req: &Value) -> Answer {
    let reported = req["conclusion"].as_str().unwrap_or("").to_ascii_lowercase();
    let conclusion = match reported.as_str() {
        "succeeded" | "success" => "success",
        "cancelled" | "canceled" => "cancelled",
        "skipped" => "skipped",
        _ => "failure",
    };
    let mut st = shared.lock();
    let Some(j) = st.job(job).filter(|j| j.status == Status::InProgress) else {
        return Answer::new(409, "the job is not in progress");
    };
    j.status = Status::Completed;
    j.reported = Some(reported);
    j.conclusion = Some(conclusion.to_string());
    j.completed_at = Some(now());
    let runner = j.runner;
    if let Some(r) = runner.and_then(|r| st.runner(r)) {
        r.gone = true;
    }
    hook::send(shared, &mut st, "completed", job);
    shared.changed.notify_all();
    Answer::new(200, Vec::new())
}

/// The results service of the Worker: a signed URL is a blob of the fake,
/// and each other call is `ok`.
fn results(shared: &Shared, path: &str) -> Answer {
    let receiver = "/twirp/results.services.receiver.Receiver/";
    match path.strip_prefix(receiver) {
        Some(call) if call.starts_with("Get") => {
            let url = format!("{}_blobs/{}", shared.base, call.trim_start_matches("Get"));
            json_answer(
                200,
                &json!({
                    "logs_url": url, "summary_url": url, "blob_storage_type": "BLOB_STORAGE_TYPE_UNSPECIFIED",
                    "soft_size_limit": 1u64 << 30,
                }),
            )
        }
        _ => json_answer(200, &json!({ "ok": true })),
    }
}

/// The text of a form value: `%XX` and `+`.
fn percent_decode(s: &str) -> Option<String> {
    let (mut out, b) = (Vec::with_capacity(s.len()), s.as_bytes());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                out.push(u8::from_str_radix(std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?, 16).ok()?);
                i += 3;
            }
            b'+' => (out.push(b' '), i += 1).1,
            c => (out.push(c), i += 1).1,
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_value_decodes() {
        assert_eq!(percent_decode("a%2Eb+c").as_deref(), Some("a.b c"));
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%2"), None);
    }

    fn job(id: u64) -> super::super::Job {
        super::super::Job {
            id,
            run_id: id,
            repo: "example-org/app".into(),
            name: format!("build-{id}"),
            labels: Vec::new(),
            steps: Vec::new(),
            status: Status::Queued,
            assigned: None,
            runner: None,
            token: String::new(),
            reported: None,
            conclusion: None,
            created_at: 0,
            started_at: None,
            completed_at: None,
        }
    }

    /// The runner polls `/message` again while it runs its job (for a
    /// cancel): that poll takes no queued job, else that job is lost.
    #[test]
    fn a_runner_that_runs_its_job_takes_no_other() {
        let runner = super::super::Runner {
            id: 9,
            name: "runner-a".into(),
            repo: "example-org/app".into(),
            labels: Vec::new(),
            client_id: String::new(),
            token: None,
            job: None,
            gone: false,
        };
        let mut st = State { jobs: vec![job(1), job(2)], runners: vec![runner], ..State::default() };
        assert_eq!(pick(&mut st, 9), Some(1));
        assert_eq!(pick(&mut st, 9), Some(1), "a job that it did not acquire comes again");
        st.job(1).unwrap().status = Status::InProgress;
        assert_eq!(pick(&mut st, 9), None, "the runner has its job: no second one");
        assert_eq!(st.jobs[1].status, Status::Queued, "the other job stays queued");
    }
}
