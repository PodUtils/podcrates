//! The control API of the test, under `/_fake/`. It needs no credential:
//! the fake serves the loopback of a test.
//!
//! | Route | What |
//! | --- | --- |
//! | `GET /_fake/app` | the App id, the installation id, the made-up key in PEM, the webhook secret |
//! | `PUT /_fake/hook` | `{"url": URL, "secret": TEXT}`: where each webhook goes, and its secret |
//! | `POST /_fake/jobs` | `{"repository": "OWNER/REPO", "name": NAME, "labels": [...], "steps": [SCRIPT...], "container": C, "services": {NAME: C}}`: a queued job; its `queued` webhook. `steps`, `container` and `services` are optional. A container `C` is an image, or `image`, `options`, `env`, `ports`, `volumes` and `credentials` as a workflow gives them; the job message has it as `jobContainer` and `jobServiceContainers` (module `container`) |
//! | `GET /_fake/jobs`, `GET /_fake/jobs/ID` | each job as a `workflow_job`, with `assigned_runner_id` and `reported` |
//! | `GET /_fake/jobs/ID/logs` | the logs of a job that ended: `conclusion`, `job_log`, and `steps`, each with `id`, `number`, `name`, `conclusion` and the text of its `log` (`null` with no upload); 404 for no such job, 409 for a job that did not end (module `logs`) |
//! | `POST /_fake/jobs/ID/assign` | `{"runner_id": ID}`: the runner gets this queued job at its next poll |
//! | `GET /_fake/runners` | each runner of each repository, with `repository` and `job_id` |
//! | `GET /_fake/deliveries` | each webhook: its action, its job, the status of its delivery or its error |

use serde_json::{Value, json};

use super::{
    Job, Shared, State, Status, api, body, container, error, has_labels, hook, json_answer, key, labels, logs, message,
    now,
};
use crate::stand::{Answer, Request};

/// The answer of a route of the control API, or none for another path.
pub(super) fn route(shared: &Shared, req: &Request, parts: &[&str]) -> Option<Answer> {
    let ["_fake", rest @ ..] = parts else { return None };
    let answer = match (req.method.as_str(), rest) {
        ("GET", ["app"]) => {
            let st = shared.lock();
            json_answer(
                200,
                &json!({
                    "app_id": super::APP_ID, "installation_id": super::INSTALLATION_ID,
                    "private_key": key::MADE_UP_KEY_PEM, "webhook_secret": st.hook_secret,
                }),
            )
        }
        ("PUT", ["hook"]) => {
            let v = body(req);
            let mut st = shared.lock();
            st.hook_url = v["url"].as_str().map(str::to_string);
            if let Some(secret) = v["secret"].as_str() {
                st.hook_secret = secret.to_string();
            }
            Answer::new(204, Vec::new())
        }
        ("POST", ["jobs"]) => queue(shared, &body(req)),
        ("GET", ["jobs"]) => {
            let st = shared.lock();
            json_answer(200, &Value::Array(st.jobs.iter().map(|j| job(shared, &st, j)).collect()))
        }
        ("GET", ["jobs", id]) => {
            let st = shared.lock();
            match st.jobs.iter().find(|j| id.parse() == Ok(j.id)) {
                Some(j) => json_answer(200, &job(shared, &st, j)),
                None => error(404, "Not Found"),
            }
        }
        ("GET", ["jobs", id, "logs"]) => logs::answer(&shared.lock(), id),
        ("POST", ["jobs", id, "assign"]) => assign(shared, id, &body(req)),
        ("GET", ["runners"]) => {
            let st = shared.lock();
            let all = st.runners.iter().map(|r| {
                let mut v = api::runner_json(&st, r);
                v["repository"] = json!(r.repo);
                v["job_id"] = json!(r.job);
                v
            });
            json_answer(200, &Value::Array(all.collect()))
        }
        ("GET", ["deliveries"]) => json_answer(200, &Value::Array(shared.lock().deliveries.clone())),
        _ => error(404, "Not Found"),
    };
    Some(answer)
}

/// A job of the control API: the `workflow_job` and what only the fake
/// knows.
fn job(shared: &Shared, st: &State, j: &Job) -> Value {
    let mut v = hook::job_json(&shared.base, st, j);
    v["repository"] = json!(j.repo);
    v["assigned_runner_id"] = json!(j.assigned);
    v["reported"] = json!(j.reported);
    v
}

fn queue(shared: &Shared, req: &Value) -> Answer {
    let repo = req["repository"].as_str().filter(|r| r.split('/').filter(|p| !p.is_empty()).count() == 2);
    let (Some(repo), Some(name)) = (repo, req["name"].as_str()) else {
        return error(422, "the job needs a repository OWNER/REPO and a name");
    };
    let Some(labels) = labels(&req["labels"]) else { return error(422, "the labels are an array of strings") };
    let steps = match &req["steps"] {
        Value::Null => vec!["true".to_string()],
        v => match super::labels(v) {
            Some(s) => s,
            None => return error(422, "the steps are an array of scripts"),
        },
    };
    let containers =
        container::container(&req["container"]).and_then(|c| Ok((c, container::services(&req["services"])?)));
    let (container, services) = match containers {
        Ok(both) => both,
        Err(e) => return error(422, &e),
    };
    let mut st = shared.lock();
    let id = st.next_id();
    let run_id = req["run_id"].as_u64().unwrap_or(id);
    st.jobs.push(Job {
        id,
        run_id,
        repo: repo.to_string(),
        name: name.to_string(),
        labels,
        steps,
        container,
        services,
        status: Status::Queued,
        assigned: None,
        runner: None,
        token: message::job_token(&key::random_hex(32)),
        reported: None,
        conclusion: None,
        created_at: now(),
        started_at: None,
        completed_at: None,
        logs: Default::default(),
    });
    hook::send(shared, &mut st, "queued", id);
    shared.changed.notify_all();
    let answer = job(shared, &st, st.jobs.last().expect("the new job"));
    json_answer(201, &answer)
}

/// Give a queued job to a runner of its repository that has its labels and
/// no job.
fn assign(shared: &Shared, id: &str, req: &Value) -> Answer {
    let Some(runner) = req["runner_id"].as_u64() else { return error(422, "the body names no runner_id") };
    let mut st = shared.lock();
    let Some(j) = st.jobs.iter().find(|j| id.parse() == Ok(j.id)) else { return error(404, "Not Found") };
    let Some(r) = st.runners.iter().find(|r| r.id == runner) else { return error(404, "no such runner") };
    if j.status != Status::Queued {
        return error(409, "the job is not queued");
    }
    if r.gone || r.job.is_some() {
        return error(409, "the runner has a job, or it is gone");
    }
    if r.repo != j.repo || !has_labels(&r.labels, &j.labels) {
        return error(409, "the runner is of another repository, or it lacks a label of the job");
    }
    let job = j.id;
    for other in st.jobs.iter_mut().filter(|o| o.assigned == Some(runner)) {
        other.assigned = None;
    }
    if let Some(j) = st.job(job) {
        j.assigned = Some(runner);
    }
    shared.changed.notify_all();
    let answer = st.jobs.iter().find(|j| j.id == job).map(|j| self::job(shared, &st, j));
    json_answer(200, &answer.unwrap_or(Value::Null))
}
