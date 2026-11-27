//! The results service of the Worker and the logs that it uploads. The
//! runner asks a signed URL for the log of each step and of the job
//! (`ResultsHttpClient`: `GetStepLogsSignedBlobURL` with `step_backend_id`,
//! `GetJobLogsSignedBlobURL`), and with the blob type `UNSPECIFIED` it puts
//! the log with plain HTTP: one `PUT` of the whole log, or a `PUT` with no
//! body and then a `PUT` of `URL&comp=appendblock` for each block. It
//! uploads each log before `completejob`, whose `stepResults` give the
//! number and the name of each step. The control API gives the logs of a
//! job that ended (`GET /_fake/jobs/ID/logs`).

use serde_json::{Value, json};

use super::{Shared, State, Status, error, json_answer};
use crate::stand::{Answer, Request};

/// The most bytes of one log: a test log is far smaller.
const MAX_LOG: usize = 64 << 20;

/// The logs of one job.
#[derive(Default)]
pub(crate) struct Logs {
    /// The log of each step, by its step backend id, in the order of the
    /// first upload.
    steps: Vec<(String, Vec<u8>)>,
    /// The log of the whole job.
    job: Vec<u8>,
    /// The `stepResults` of `completejob`.
    results: Vec<Value>,
}

impl Logs {
    /// Keep the `stepResults` of the `completejob` body `req`.
    pub(super) fn ended(&mut self, req: &Value) {
        self.results = req["stepResults"].as_array().cloned().unwrap_or_default();
    }
}

/// A call of the results service of `job`: a signed URL is a blob of the
/// fake, and each other call is `ok`.
pub(super) fn results(shared: &Shared, job: u64, path: &str, req: &Value) -> Answer {
    let Some(call) = path.strip_prefix("/twirp/results.services.receiver.Receiver/") else {
        return json_answer(200, &json!({ "ok": true }));
    };
    if !call.starts_with("Get") {
        return json_answer(200, &json!({ "ok": true }));
    }
    let blob = match call {
        "GetStepLogsSignedBlobURL" => match req["step_backend_id"].as_str().filter(|s| step_id(s)) {
            Some(step) => format!("steps/{}", step.to_ascii_lowercase()),
            None => return error(400, "the request names no step_backend_id"),
        },
        "GetJobLogsSignedBlobURL" => "job".to_string(),
        _ => "other".to_string(),
    };
    // The query makes the `&comp=appendblock` of the runner a query too.
    let url = format!("{}_blobs/{job}/{blob}?sv=pod-fake-github", shared.base);
    json_answer(
        200,
        &json!({
            "logs_url": url, "summary_url": url, "diag_logs_url": url,
            "blob_storage_type": "BLOB_STORAGE_TYPE_UNSPECIFIED", "soft_size_limit": MAX_LOG,
        }),
    )
}

/// A step backend id: a GUID of the runner.
fn step_id(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// A `PUT` of a blob of [`results`]: a whole log, or a block of it.
pub(super) fn put(shared: &Shared, req: &Request) -> Answer {
    let path = req.path();
    let mut parts = path.trim_start_matches("/_blobs/").splitn(2, '/');
    let (Some(job), Some(blob)) = (parts.next().and_then(|j| j.parse::<u64>().ok()), parts.next()) else {
        return error(404, "no such blob");
    };
    let query = req.target.split_once('?').map_or("", |(_, q)| q);
    let append = query.split('&').any(|kv| kv == "comp=appendblock");
    let mut st = shared.lock();
    let Some(j) = st.job(job) else { return error(404, "no such blob") };
    let log = match blob.strip_prefix("steps/") {
        Some(step) if step_id(step) => match j.logs.steps.iter().position(|(s, _)| s.eq_ignore_ascii_case(step)) {
            Some(i) => &mut j.logs.steps[i].1,
            None => {
                j.logs.steps.push((step.to_ascii_lowercase(), Vec::new()));
                &mut j.logs.steps.last_mut().expect("the new log").1
            }
        },
        Some(_) => return error(404, "no such blob"),
        None if blob == "job" => &mut j.logs.job,
        // A summary or the diagnostic logs: the fake keeps none.
        None => return Answer::new(201, Vec::new()),
    };
    if !append {
        log.clear();
    }
    if log.len() + req.body.len() > MAX_LOG {
        return error(413, "the log is too large for the fake");
    }
    log.extend_from_slice(&req.body);
    Answer::new(201, Vec::new())
}

/// `GET /_fake/jobs/ID/logs`: 404 for no such job, 409 for a job that did
/// not end; else the job log and each step with its log (`null` for a step
/// with no upload), in the order of the step numbers.
pub(super) fn answer(st: &State, id: &str) -> Answer {
    let Some(j) = st.jobs.iter().find(|j| id.parse() == Ok(j.id)) else { return error(404, "Not Found") };
    if j.status != Status::Completed {
        return error(409, "the job did not end: its logs are not whole");
    }
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let mut results: Vec<&Value> = j.logs.results.iter().collect();
    results.sort_by_key(|r| r["number"].as_u64().unwrap_or(u64::MAX));
    let mut steps = Vec::with_capacity(results.len());
    for r in results {
        let id = r["external_id"].as_str().unwrap_or_default().to_ascii_lowercase();
        let log = j.logs.steps.iter().find(|(s, _)| *s == id).map(|(_, b)| text(b));
        steps.push(json!({
            "id": id, "number": r["number"], "name": r["name"], "conclusion": r["conclusion"], "log": log,
        }));
    }
    for (id, b) in &j.logs.steps {
        if !steps.iter().any(|s| s["id"] == id.as_str()) {
            steps.push(json!({ "id": id, "number": null, "name": null, "conclusion": null, "log": text(b) }));
        }
    }
    json_answer(
        200,
        &json!({
            "id": j.id, "conclusion": j.conclusion, "job_log": text(&j.logs.job), "steps": steps,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_id_is_a_guid_of_the_runner() {
        assert!(step_id("00000000-0000-0000-0000-000000000100"));
        assert!(step_id("ABCdef-01"));
        for bad in ["", "..", "a/b", "a b", "g", &"a".repeat(65)] {
            assert!(!step_id(bad), "{bad:?}");
        }
    }
}
