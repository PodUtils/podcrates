//! The messages of the broker: the job request of the long poll, and the
//! job message of `acquirejob` that the Worker of the runner runs. The
//! shapes are the ones that the official runner reads
//! (`AgentJobRequestMessage`, camel case), for a job of `run:` steps; a
//! field that the runner does not need is left out.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use super::Job;

fn guid(n: u64) -> String {
    format!("00000000-0000-0000-0000-{n:012x}")
}

/// A job token with the random signature part `sig`. The runner reads its
/// claims and checks no signature; the fake compares the whole token.
pub(super) fn job_token(sig: &str) -> String {
    let b64 = |v: Value| URL_SAFE_NO_PAD.encode(v.to_string());
    let head = json!({ "typ": "JWT", "alg": "RS256" });
    let claims = json!({
        "nameid": "dddddddd-dddd-dddd-dddd-dddddddddddd",
        "scp": "Actions.GenericRead:00000000-0000-0000-0000-000000000000 Actions.Results:1:2",
        "iat": 1_700_000_000u64, "nbf": 1_700_000_000u64, "exp": 4_102_444_800u64,
        "iss": "pod-fake-github", "aud": "pod-fake-github",
    });
    format!("{}.{}.{sig}", b64(head), b64(claims))
}

/// The body of a broker message: a reference to the job `id` on the run
/// service at `base`.
pub(super) fn job_request(base: &str, id: u64) -> Value {
    let reference = json!({
        "runner_request_id": id.to_string(), "run_service_url": base,
        "billing_owner_id": "pod-fake-github", "should_acknowledge": false,
    });
    json!({ "messageId": id, "messageType": "RunnerJobRequest", "body": reference.to_string() })
}

/// A mapping token of the runner (`TemplateToken` type 2).
fn mapping(pairs: &[(&str, &str)]) -> Value {
    let map: Vec<Value> = pairs.iter().map(|(k, v)| json!({ "Key": k, "Value": v })).collect();
    json!({ "type": 2, "map": map })
}

/// The job message of `job` on the services at `base`.
pub(super) fn job_message(base: &str, job: &Job) -> Value {
    let id = job.id;
    let steps: Vec<Value> = job
        .steps
        .iter()
        .enumerate()
        .map(|(i, script)| {
            json!({
                "type": "action", "reference": { "type": "script" },
                "id": guid(0x100 + i as u64), "name": format!("__run_{i}"), "contextName": format!("__run_{i}"),
                "displayName": format!("Run step {}", i + 1), "condition": "success()",
                "inputs": mapping(&[("script", script)]),
            })
        })
        .collect();
    let s = |k: &str, v: &str| json!({ "k": k, "v": v });
    json!({
        "messageType": "RunnerJobRequest",
        "plan": { "scopeIdentifier": guid(1), "planType": "actions", "version": 1, "planId": guid(2 + 0x10 * id) },
        "timeline": { "id": guid(3 + 0x10 * id), "changeId": 1 },
        "jobId": guid(4 + 0x10 * id),
        "jobDisplayName": job.name, "jobName": job.name, "requestId": id,
        "lockedUntil": "2099-01-01T00:00:00Z",
        "resources": {
            "endpoints": [{
                "name": "SystemVssConnection", "url": base, "isReady": true,
                "authorization": { "scheme": "OAuth", "parameters": { "AccessToken": job.token } },
                // ResultsServiceUrl gives ACTIONS_RESULTS_URL to the steps.
                "data": { "ServerId": guid(5), "ServerName": "pod-fake-github", "ResultsServiceUrl": base },
            }],
            "repositories": [],
        },
        "workspace": {},
        "jobContainer": null,
        "environmentVariables": [],
        "variables": {
            "system.github.job": { "value": job.name },
            // The results service of the Worker (logs, step updates): this fake.
            "system.github.results_endpoint": { "value": base },
            // `github.token` of the steps: a made-up value that no forge takes.
            "system.github.token": { "value": "pod-fake-github-made-up-token", "isSecret": true },
        },
        "contextData": { "github": { "t": 2, "d": [
            s("repository", &job.repo), s("workflow", "pod-fake-github"), s("job", &job.name),
            s("run_id", &job.run_id.to_string()), s("event_name", "push"),
            s("server_url", "https://github.com"), s("api_url", "https://api.github.com"),
            s("graphql_url", "https://api.github.com/graphql"),
        ] } },
        "steps": steps,
        "fileTable": [],
        "mask": [{ "type": "regex", "value": job.token }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Status;

    #[test]
    fn the_messages_have_the_shapes_of_the_runner() {
        let r = job_request("http://127.0.0.1:1/", 3);
        assert_eq!(r["messageType"], "RunnerJobRequest");
        let body: Value = serde_json::from_str(r["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["runner_request_id"], "3");
        let job = Job {
            id: 3,
            run_id: 9,
            repo: "o/r".into(),
            name: "build".into(),
            labels: vec!["self-hosted".into()],
            steps: vec!["true".into(), "false".into()],
            status: Status::Given,
            assigned: None,
            runner: None,
            token: job_token("c2ln"),
            reported: None,
            conclusion: None,
            created_at: 0,
            started_at: None,
            completed_at: None,
        };
        let m = job_message("http://127.0.0.1:1/", &job);
        assert_eq!(m["requestId"], 3);
        assert_eq!(m["steps"].as_array().unwrap().len(), 2);
        assert_eq!(m["steps"][1]["inputs"]["map"][0]["Value"], "false");
        assert_eq!(m["resources"]["endpoints"][0]["authorization"]["parameters"]["AccessToken"], job.token);
        // The job token is a JWT with three parts and an expiry.
        let parts: Vec<&str> = job.token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert!(claims["exp"].as_u64().unwrap() > 4_000_000_000);
    }
}
