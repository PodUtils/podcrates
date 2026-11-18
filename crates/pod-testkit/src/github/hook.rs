//! The `workflow_job` webhooks: the payload of each action, its signature,
//! and one thread that sends them in the order of the events.

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Weak};

use serde_json::{Value, json};

use super::{INSTALLATION_ID, Job, Shared, State, Status, key, rfc3339};
use crate::stand;

/// One webhook to send.
pub(crate) struct Delivery {
    url: Option<String>,
    secret: String,
    id: String,
    body: Vec<u8>,
    /// Its line in `State::deliveries`.
    index: usize,
}

/// The `workflow_job` object of `job`, as the webhooks and the REST API of
/// GitHub give it.
pub(super) fn job_json(base: &str, st: &State, job: &Job) -> Value {
    let status = match job.status {
        Status::Queued | Status::Given => "queued",
        Status::InProgress => "in_progress",
        Status::Completed => "completed",
    };
    let runner = job.runner.and_then(|r| st.runners.iter().find(|x| x.id == r));
    let shown = job.status != Status::Queued && job.status != Status::Given;
    let time = |t: Option<u64>| t.map_or(Value::Null, |t| json!(rfc3339(t)));
    json!({
        "id": job.id, "run_id": job.run_id, "run_attempt": 1, "workflow_name": "pod-fake-github",
        "url": format!("{base}repos/{}/actions/jobs/{}", job.repo, job.id),
        "html_url": format!("https://github.com/{}/actions/runs/{}/job/{}", job.repo, job.run_id, job.id),
        "head_branch": "main", "head_sha": "0000000000000000000000000000000000000000",
        "name": job.name, "labels": job.labels, "status": status, "conclusion": job.conclusion,
        "created_at": rfc3339(job.created_at), "started_at": time(job.started_at),
        "completed_at": time(job.completed_at), "steps": [],
        "runner_id": runner.filter(|_| shown).map(|r| r.id),
        "runner_name": runner.filter(|_| shown).map(|r| r.name.clone()),
        "runner_group_id": runner.filter(|_| shown).map(|_| 1),
        "runner_group_name": runner.filter(|_| shown).map(|_| "Default"),
    })
}

/// Queue the webhook `action` of `job`. The caller holds the state, so the
/// webhooks go in the order of the events.
pub(super) fn send(shared: &Shared, st: &mut State, action: &str, job_id: u64) {
    let Some(job) = st.jobs.iter().find(|j| j.id == job_id) else { return };
    let (owner, name) = job.repo.split_once('/').unwrap_or((&job.repo, ""));
    let payload = json!({
        "action": action,
        "workflow_job": job_json(&shared.base, st, job),
        "repository": {
            // One id for each name, the same in each payload.
            "id": job.repo.bytes().fold(7u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b))) % 1_000_000_007,
            "name": name, "full_name": job.repo, "private": true,
            "owner": { "login": owner, "type": "Organization" },
        },
        "installation": { "id": INSTALLATION_ID },
        "sender": { "login": "pod-fake-github", "type": "Bot" },
    });
    let index = st.deliveries.len();
    st.deliveries.push(json!({ "action": action, "job_id": job_id, "status": null, "error": null }));
    let delivery = Delivery {
        url: st.hook_url.clone(),
        secret: st.hook_secret.clone(),
        id: format!("{}-{}", key::random_hex(4), key::random_hex(12)),
        body: payload.to_string().into_bytes(),
        index,
    };
    let _ = shared.hooks.lock().unwrap_or_else(|e| e.into_inner()).send(delivery);
}

/// The thread of the deliveries. It ends with the fake.
pub(super) fn sender(shared: Weak<Shared>, deliveries: Receiver<Delivery>) {
    std::thread::spawn(move || {
        for d in deliveries {
            let result = match &d.url {
                None => Err("no URL of the hook".to_string()),
                Some(url) => {
                    let signature = key::hub_signature(&d.secret, &d.body);
                    let headers = [
                        ("Content-Type", "application/json"),
                        ("User-Agent", "GitHub-Hookshot/pod-fake-github"),
                        ("X-GitHub-Event", "workflow_job"),
                        ("X-GitHub-Delivery", d.id.as_str()),
                        ("X-GitHub-Hook-Installation-Target-Type", "integration"),
                        ("X-Hub-Signature-256", signature.as_str()),
                    ];
                    stand::call("POST", url, &headers, &d.body).map(|(status, _)| status).map_err(|e| e.to_string())
                }
            };
            let Some(shared) = shared.upgrade() else { return };
            record(&shared, d.index, result);
        }
    });
}

fn record(shared: &Arc<Shared>, index: usize, result: Result<u16, String>) {
    let mut st = shared.lock();
    let Some(line) = st.deliveries.get_mut(index) else { return };
    match result {
        Ok(status) => line["status"] = json!(status),
        Err(e) => line["error"] = json!(e),
    }
}
