//! The operations of the source wire for one session, as githome answers
//! them (podrunner `docs/contracts/source-driver.md`):
//!
//! | Op | The rule |
//! | --- | --- |
//! | `hello` | the name of the source and `operator_controlled`; the facts of the host are kept for `next` |
//! | `next` | the control events of the session, then up to `capacity` queued jobs that fit the host facts and that the session does not hold or refused; with nothing to give, it waits up to `wait_s` (at most [`WAIT_MAX`]) |
//! | `ack` | a held job |
//! | `claim` | a held job that no session claimed: else `already-claimed`, or `unknown-job` for a job that ended |
//! | `mint` | the claim of the session for this runner name, else `not-claimed`; one credential for each claim (`credential-already-issued`); with a linked fake GitHub, the files of a JIT runner there that gets the job, else `credential-unavailable` |
//! | `report` | the claim of the session, else `not-claimed`; a second report replaces the first |
//! | `audit` | each line |
//! | `withdraw` | a held job that the session did not claim (`already-claimed`: release it); a reason that is not `wait-deadline`, `cancelled` or `conformance` is a refusal, and the session never gets the job again |
//! | `release` | a held job; a claim with no report, or a report of `exit_reason` `lost`, queues the job again; a report with a `refusal_code` queues it for the other sessions; another report ends it |
//! | `assigned` | the claim of the session, else `not-claimed`; `keep` with the trust facts of the real job, or `end` |
//!
//! A job that the session does not hold is `unknown-job` for each of them.
//! The settings of the control API make each operation `unavailable`, or
//! each `mint` `source-revoked`.

use std::time::Instant;

use pod_job::JobId;
use pod_job::source::{
    Event, Hidden, Minted, MintedFile, Reply, Request, SourceCode, SourceError, parse_request, reply_line,
};
use serde_json::{Map, Value, json};

use super::{Job, Shared, State, Status, WAIT_MAX, error, link, offer, session_of};
use crate::stand::{Answer, Request as Call};

/// The reasons of `withdraw` that are not a refusal: the job can come again.
pub const NOT_REFUSALS: [&str; 3] = ["wait-deadline", "cancelled", "conformance"];

/// `POST /_runner/source/OP`: one request line of a session, and its reply
/// line. `OP` `refusal` records a refusal of the program itself (a line
/// that is not of the wire).
pub(super) fn route(shared: &Shared, call: &Call, op: &str) -> Answer {
    let Some(id) = session_of(shared, call) else { return error(404, "no such session") };
    if op == "refusal" {
        let v = super::body(call);
        shared.lock().log("refusals", json!({ "session": id, "op": null, "code": v["code"], "detail": v["detail"] }));
        return Answer::new(204, Vec::new());
    }
    let req = match parse_request(&call.body) {
        Ok(r) if r.op() == op => r,
        Ok(_) => return error(400, "the op of the path is not the op of the body"),
        Err(e) => return reply_answer(&refuse(shared, id, None, None, e)),
    };
    reply_answer(&answer(shared, id, req))
}

fn reply_answer(reply: &Reply) -> Answer {
    Answer::new(200, reply_line(reply)).header("Content-Type", "application/json")
}

/// The reply of the fake to one request of the session `id`.
pub(super) fn answer(shared: &Shared, id: u64, req: Request) -> Reply {
    let op = req.op();
    let job = job_of(&req).cloned();
    let result = if shared.lock().unavailable {
        Err(SourceError::new(SourceCode::Unavailable, "the source is not available"))
    } else if let Request::Next { capacity, wait_s } = req {
        Ok(next(shared, id, capacity, wait_s))
    } else {
        let mut st = shared.lock();
        let result = apply(&mut st, id, req);
        shared.changed.notify_all();
        result
    };
    result.unwrap_or_else(|e| refuse(shared, id, Some(op), job.as_ref(), e))
}

/// Record a refusal; its reply.
fn refuse(shared: &Shared, id: u64, op: Option<&str>, job: Option<&JobId>, e: SourceError) -> Reply {
    let line = json!({ "session": id, "op": op, "job_id": job, "code": e.code, "detail": e.detail });
    shared.lock().log("refusals", line);
    Reply::Error { code: e.code, detail: e.detail }
}

fn job_of(req: &Request) -> Option<&JobId> {
    match req {
        Request::Ack { job_id }
        | Request::Claim { job_id, .. }
        | Request::Mint { job_id, .. }
        | Request::Report { job_id, .. }
        | Request::Withdraw { job_id, .. }
        | Request::Release { job_id }
        | Request::Assigned { job_id, .. } => Some(job_id),
        Request::Hello { .. } | Request::Next { .. } | Request::Audit { .. } => None,
    }
}

fn refused(code: SourceCode, detail: &str) -> SourceError {
    SourceError::new(code, detail)
}

/// The long poll of `next`.
fn next(shared: &Shared, id: u64, capacity: u32, wait_s: u32) -> Reply {
    let deadline = Instant::now() + WAIT_MAX.min(std::time::Duration::from_secs(wait_s.into()));
    let mut st = shared.lock();
    loop {
        let events = take(&mut st, id, capacity);
        let left = deadline.saturating_duration_since(Instant::now());
        if !events.is_empty() || left.is_zero() {
            return Reply::Events { events };
        }
        st = shared.changed.wait_timeout(st, left).unwrap_or_else(|e| e.into_inner()).0;
    }
}

/// The events of the session, then the offers of up to `capacity` jobs.
fn take(st: &mut State, id: u64, capacity: u32) -> Vec<Event> {
    let State { jobs, sessions, .. } = st;
    let Some(s) = sessions.iter_mut().find(|s| s.id == id) else { return Vec::new() };
    let mut events: Vec<Event> = s.events.drain(..).collect();
    let open: Vec<&Job> = jobs
        .iter()
        .filter(|j| {
            j.status == Status::Queued
                && !s.held.contains_key(j.id())
                && !j.refused_by.contains(&id)
                && offer::fits(s.host.as_ref(), &j.offer)
        })
        .take(capacity as usize)
        .collect();
    for j in open {
        s.held.insert(j.id().clone(), "offered");
        events.push(Event::Offer { offer: offer::wire(&j.offer) });
    }
    events
}

/// Each operation other than `next`.
fn apply(st: &mut State, id: u64, req: Request) -> Result<Reply, SourceError> {
    let done = Ok(Reply::Done {});
    match req {
        Request::Hello { client, version, host } => {
            let line = json!({ "session": id, "client": client, "version": version, "host": host });
            st.log("hosts", line);
            if let Some(s) = st.session(id) {
                s.host = host;
            }
            Ok(Reply::Hello { source: st.source.clone(), operator_controlled: st.operator_controlled })
        }
        Request::Next { .. } => Err(refused(SourceCode::BadRequest, "next is a long poll")),
        Request::Ack { job_id } => {
            if held(st, id, &job_id)? == "offered" {
                set_held(st, id, &job_id, "acked");
            }
            done
        }
        Request::Claim { job_id, runner_name } => {
            held(st, id, &job_id)?;
            let j = job(st, &job_id)?;
            match j.status {
                Status::Done | Status::Cancelled => return Err(refused(SourceCode::UnknownJob, "the job ended")),
                Status::Claimed | Status::Reported => {
                    return Err(refused(SourceCode::AlreadyClaimed, "a runner claimed the job"));
                }
                Status::Queued => {}
            }
            j.status = Status::Claimed;
            j.claim = Some((id, runner_name));
            j.report = None;
            set_held(st, id, &job_id, "claimed");
            done
        }
        Request::Mint { job_id, runner_name } => mint(st, id, &job_id, &runner_name),
        Request::Report { job_id, report } => {
            let j = claimed(st, id, &job_id)?;
            j.status = Status::Reported;
            j.report = Some(report.clone());
            let runner = j.claim.as_ref().map(|c| c.1.clone());
            st.log("reports", json!({ "session": id, "job_id": job_id, "runner_name": runner, "report": report }));
            done
        }
        Request::Audit { line } => {
            st.log("audits", json!({ "session": id, "line": line }));
            done
        }
        Request::Withdraw { job_id, reason } => {
            held(st, id, &job_id)?;
            let j = job(st, &job_id)?;
            if j.claimed_by(id) {
                return Err(refused(SourceCode::AlreadyClaimed, "the job is claimed: release it"));
            }
            let refusal = !NOT_REFUSALS.contains(&reason.as_str());
            if refusal {
                j.refused_by.push(id);
            }
            unhold(st, id, &job_id);
            st.log("withdrawals", json!({ "session": id, "job_id": job_id, "reason": reason, "refusal": refusal }));
            done
        }
        Request::Release { job_id } => {
            held(st, id, &job_id)?;
            let j = job(st, &job_id)?;
            let outcome = if j.claimed_by(id) { release(j, id) } else { "offer-ended" };
            unhold(st, id, &job_id);
            st.log("releases", json!({ "session": id, "job_id": job_id, "outcome": outcome }));
            done
        }
        Request::Assigned { job_id, real } => {
            let j = claimed(st, id, &job_id)?;
            let reply = offer::assigned(&j.offer, &real);
            let verdict = match &reply {
                Reply::Keep { .. } => json!("keep"),
                Reply::End { code, .. } => json!(code),
                _ => Value::Null,
            };
            st.log("assignments", json!({ "session": id, "job_id": job_id, "real": real, "verdict": verdict }));
            Ok(reply)
        }
    }
}

/// The end of the claim of a job at its `release`: the outcome.
fn release(j: &mut Job, id: u64) -> &'static str {
    let report = j.report.take();
    j.claim = None;
    j.minted = false;
    let field = |name: &str| report.as_ref().and_then(|r| r.get(name)).and_then(Value::as_str).map(str::to_string);
    let (status, outcome) = match (report.is_some(), field("refusal_code"), field("exit_reason").as_deref()) {
        (false, _, _) => (Status::Queued, "given-back"),
        (true, Some(_), _) => {
            j.refused_by.push(id);
            (Status::Queued, "refused")
        }
        (true, None, Some("lost")) => (Status::Queued, "lost"),
        (true, None, _) => (Status::Done, "done"),
    };
    j.status = status;
    outcome
}

fn mint(st: &mut State, id: u64, job_id: &JobId, runner_name: &str) -> Result<Reply, SourceError> {
    let (revoked, github) = (st.revoked, st.github.clone());
    let j = claimed(st, id, job_id)?;
    if j.claim.as_ref().is_some_and(|c| c.1 != runner_name) {
        return Err(refused(SourceCode::NotClaimed, "the job is claimed for another runner name"));
    }
    if revoked {
        return Err(refused(SourceCode::SourceRevoked, "the credential of the source is revoked"));
    }
    if let Some(code) = j.mint_error.take() {
        return Err(refused(code, "a mint error of the control API"));
    }
    if j.minted {
        return Err(refused(SourceCode::CredentialAlreadyIssued, "the claim has its credential"));
    }
    let linked = match (github.as_deref(), j.github_job) {
        (None, _) => None,
        (Some(g), Some(github_job)) => Some(
            link::mint(g, &j.offer, github_job, runner_name)
                .map_err(|e| refused(SourceCode::CredentialUnavailable, &e))?,
        ),
        (Some(_), None) => {
            return Err(refused(SourceCode::CredentialUnavailable, "the job has no job in the linked fake GitHub"));
        }
    };
    j.minted = true;
    let n = st.next_id();
    let (runner_id, files) = match linked {
        Some((runner_id, files)) => (Some(runner_id), files),
        None => (None, made_up_files(job_id, runner_name, n)),
    };
    let shown: Map<String, Value> = files.iter().map(|f| (f.name.clone(), json!(f.content.expose()))).collect();
    let line = json!({
        "session": id, "job_id": job_id, "runner_name": runner_name, "files": shown, "github_runner_id": runner_id,
    });
    st.log("mints", line);
    Ok(Reply::Credential { credential: Minted { files } })
}

/// The made-up files of the runner of a job: the names of the official
/// runner, after podrunner's canned credential, with a count for each mint
/// so that a test can find each one.
fn made_up_files(job: &JobId, runner_name: &str, n: u64) -> Vec<MintedFile> {
    let file = |name: &str, text: String| MintedFile { name: name.to_string(), content: Hidden::new(text) };
    vec![
        file(
            ".runner",
            json!({ "agentName": runner_name, "job": job, "serverUrl": "http://githome.invalid/" }).to_string(),
        ),
        file(
            ".credentials",
            json!({ "scheme": "OAuth", "data": { "clientId": format!("pod-fake-githome-made-up-{n}") } }).to_string(),
        ),
        file(".credentials_rsaparams", json!({ "fake": true, "mint": n }).to_string()),
    ]
}

/// The step of a job that the session holds, else `unknown-job`.
fn held(st: &State, id: u64, job: &JobId) -> Result<&'static str, SourceError> {
    let s = st.sessions.iter().find(|s| s.id == id);
    s.and_then(|s| s.held.get(job).copied())
        .ok_or_else(|| refused(SourceCode::UnknownJob, "the job is not offered to this source"))
}

fn set_held(st: &mut State, id: u64, job: &JobId, step: &'static str) {
    if let Some(s) = st.session(id) {
        s.held.insert(job.clone(), step);
    }
}

fn unhold(st: &mut State, id: u64, job: &JobId) {
    if let Some(s) = st.session(id) {
        s.held.remove(job);
    }
}

fn job<'a>(st: &'a mut State, job: &JobId) -> Result<&'a mut Job, SourceError> {
    st.job(job).ok_or_else(|| refused(SourceCode::UnknownJob, "no such job"))
}

/// A held job of the claim of the session, else `not-claimed`.
fn claimed<'a>(st: &'a mut State, id: u64, job_id: &JobId) -> Result<&'a mut Job, SourceError> {
    held(st, id, job_id)?;
    let j = job(st, job_id)?;
    if !j.claimed_by(id) {
        return Err(refused(SourceCode::NotClaimed, "this source holds no claim of the job"));
    }
    Ok(j)
}
