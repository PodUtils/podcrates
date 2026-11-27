//! The control API of the test, under `/_fake/`. It needs no credential:
//! the fake serves the loopback of a test.
//!
//! | Route | What |
//! | --- | --- |
//! | `POST /_fake/jobs` | the fields of a `job-offer.v1`, each optional, with `repository` and `tier` (module `offer`): a queued job; with a linked fake GitHub, `steps` (the scripts of its `run:` steps), `container` and `services` too (as `POST /_fake/jobs` of the fake GitHub takes them), and the job is queued there |
//! | `GET /_fake/jobs`, `GET /_fake/jobs/ID` | each job: its `offer`, `status`, `claim`, `minted` and `github_job_id` |
//! | `POST /_fake/jobs/ID/cancel` | the job ends; each session that holds it gets the event `cancel` |
//! | `POST /_fake/jobs/ID/started` | `{"runner_name": NAME}` (default: the runner of the claim): the event `started` |
//! | `POST /_fake/jobs/ID/completed` | `{"result": TEXT}`: the event `completed` |
//! | `POST /_fake/jobs/ID/mint-error` | `{"code": CODE}`: the next `mint` of the job fails with the code |
//! | `GET /_fake/settings`, `PUT /_fake/settings` | `source`, `operator_controlled`, `unavailable`, `revoked` |
//! | `GET /_fake/NAME` | the records of [`super::LOGS`], in order |

use pod_job::JobId;
use pod_job::source::{Event, SourceCode};
use serde_json::{Map, Value, json};

use super::{Job, LOGS, Shared, State, Status, body, error, json_answer, link, offer};
use crate::stand::{Answer, Request};

/// The answer of a route of the control API.
pub(super) fn route(shared: &Shared, req: &Request, parts: &[&str]) -> Answer {
    match (req.method.as_str(), parts) {
        ("POST", ["jobs"]) => queue(shared, &body(req)),
        ("GET", ["jobs"]) => {
            let st = shared.lock();
            json_answer(200, &Value::Array(st.jobs.iter().map(job_json).collect()))
        }
        ("GET", ["jobs", id]) => match shared.lock().jobs.iter().find(|j| j.id().as_str() == *id) {
            Some(j) => json_answer(200, &job_json(j)),
            None => error(404, "no such job"),
        },
        ("POST", ["jobs", id, what]) => event(shared, id, what, &body(req)),
        ("GET", ["settings"]) => json_answer(200, &settings(&shared.lock())),
        ("PUT", ["settings"]) => {
            let v = body(req);
            let mut st = shared.lock();
            if let Some(s) = v["source"].as_str().filter(|s| !s.is_empty()) {
                st.source = s.to_string();
            }
            let flag = |name: &str, old: bool| v[name].as_bool().unwrap_or(old);
            st.operator_controlled = flag("operator_controlled", st.operator_controlled);
            st.unavailable = flag("unavailable", st.unavailable);
            st.revoked = flag("revoked", st.revoked);
            shared.changed.notify_all();
            json_answer(200, &settings(&st))
        }
        ("GET", [name]) if LOGS.contains(name) => {
            let st = shared.lock();
            json_answer(200, &Value::Array(st.logs.get(name).cloned().unwrap_or_default()))
        }
        _ => error(404, "no such route"),
    }
}

fn settings(st: &State) -> Value {
    json!({
        "source": st.source, "operator_controlled": st.operator_controlled,
        "unavailable": st.unavailable, "revoked": st.revoked,
    })
}

fn job_json(j: &Job) -> Value {
    json!({
        "job_id": j.id(),
        "status": j.status.as_str(),
        "offer": j.offer,
        "claim": j.claim.as_ref().map(|(session, runner)| json!({ "session": session, "runner_name": runner })),
        "minted": j.minted,
        "refused_by": j.refused_by,
        "github_job_id": j.github_job,
    })
}

/// A queued job. With a linked fake GitHub, the body can name the fields of
/// [`link::FIELDS`], and the job is queued there first: a job that the fake
/// GitHub refuses is not queued.
fn queue(shared: &Shared, req: &Value) -> Answer {
    let mut st = shared.lock();
    let n = st.next_id();
    let source = st.source.clone();
    let mut req = req.clone();
    let mut job = Map::new();
    if let (Some(_), Some(fields)) = (&st.github, req.as_object_mut()) {
        for name in link::FIELDS {
            if let Some(v) = fields.remove(name) {
                job.insert(name.to_string(), v);
            }
        }
    }
    let offer = match offer::build(&req, n, &source) {
        Ok(o) => o,
        Err(e) => return error(422, &e),
    };
    if st.jobs.iter().any(|j| j.id() == &offer.job_id) {
        return error(409, "a job has this job_id");
    }
    let github_job = match st.github.as_deref().map(|g| link::queue(g, &offer, &job)).transpose() {
        Ok(id) => id,
        Err(e) => return error(502, &e),
    };
    st.jobs.push(Job {
        offer,
        status: Status::Queued,
        claim: None,
        report: None,
        minted: false,
        mint_error: None,
        github_job,
        refused_by: Vec::new(),
    });
    shared.changed.notify_all();
    json_answer(201, &job_json(st.jobs.last().expect("the new job")))
}

/// An event of the forge for a job, or a mint error.
fn event(shared: &Shared, id: &str, what: &str, req: &Value) -> Answer {
    let Ok(job_id) = JobId::new(id) else { return error(404, "no such job") };
    let mut st = shared.lock();
    let Some(j) = st.job(&job_id) else { return error(404, "no such job") };
    let runner = j.claim.as_ref().map(|c| c.1.clone());
    let event = match what {
        "cancel" => {
            j.status = Status::Cancelled;
            Event::Cancel { job_id: job_id.clone() }
        }
        "started" => match req["runner_name"].as_str().map(str::to_string).or(runner) {
            Some(runner_name) => Event::Started { job_id: job_id.clone(), runner_name },
            None => return error(422, "the job has no claim: give a runner_name"),
        },
        "completed" => match req["result"].as_str() {
            Some(result) => Event::Completed { job_id: job_id.clone(), result: result.to_string() },
            None => return error(422, "give a result"),
        },
        "mint-error" => {
            return match serde_json::from_value::<SourceCode>(req["code"].clone()) {
                Ok(code) => {
                    j.mint_error = Some(code);
                    Answer::new(204, Vec::new())
                }
                Err(_) => error(422, "the code is not a code of the source wire"),
            };
        }
        _ => return error(404, "no such route"),
    };
    let mut told = 0;
    for s in st.sessions.iter_mut().filter(|s| s.held.contains_key(&job_id)) {
        s.events.push_back(event.clone());
        told += 1;
    }
    shared.changed.notify_all();
    json_answer(200, &json!({ "sessions": told }))
}
