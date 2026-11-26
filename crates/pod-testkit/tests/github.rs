//! The fake GitHub (feature `github`): the acceptance of githome's G-30
//! through the binary `pod-fake-github`, and the refusals of the fake. The
//! test is the App, its webhook receiver and the runner: it speaks the
//! routes that the official runner calls.
#![cfg(feature = "github")]

mod fakes;

use std::sync::{Mutex, mpsc};
use std::time::Duration;

use base64::Engine as _;
use fakes::{Program, Runner, call, files, now};
use pod_testkit::github::client::{installation_token as token_of, jit_runner};
use pod_testkit::github::{APP_ID, FakeGithub, Key, hub_signature};
use pod_testkit::need_loopback;
use pod_testkit::stand::{self, Answer, Request, Stand};
use serde_json::{Value, json};

/// The labels of each job and runner of the acceptance.
const LABELS: [&str; 2] = ["self-hosted", "linux"];

fn program(hook: &str) -> Program {
    fakes::start(env!("CARGO_BIN_EXE_pod-fake-github"), &["--hook", hook])
}

/// The webhooks that the receiver got, in order.
struct Hooks {
    stand: Stand,
    got: mpsc::Receiver<Request>,
}

fn hooks() -> Hooks {
    let (send, got) = mpsc::channel();
    let send = Mutex::new(send);
    let stand = stand::http(move |r| {
        let _ = send.lock().unwrap().send(r.clone());
        Answer::new(200, "")
    })
    .unwrap();
    Hooks { stand, got }
}

impl Hooks {
    fn url(&self) -> String {
        format!("http://{}/hook", self.stand.addr())
    }

    /// The next webhook, after a check of its signature with `secret`.
    fn next(&self, secret: &str) -> Value {
        let r = self.got.recv_timeout(Duration::from_secs(20)).expect("a webhook within 20 s");
        assert_eq!(r.header("x-github-event"), Some("workflow_job"));
        assert_eq!(r.header("x-hub-signature-256"), Some(hub_signature(secret, &r.body).as_str()), "the signature");
        serde_json::from_slice(&r.body).unwrap()
    }
}

/// The App: its JWT, the installation of the repository, a token.
fn installation_token(base: &str, key: &Key, repo: &str) -> String {
    token_of(base, key, repo).unwrap()
}

/// A JIT runner of `repo`: its id and its three files, decoded.
fn mint(base: &str, token: &str, repo: &str, name: &str) -> (u64, Value) {
    let runner = jit_runner(base, token, repo, name, &LABELS.map(String::from)).unwrap();
    let decoded = files(runner.files.iter().map(|(n, t)| (n.as_str(), t.as_str())));
    assert_eq!(decoded.as_object().unwrap().len(), 3, "{decoded}");
    assert_eq!(decoded[".runner"]["agentName"], name);
    assert_eq!(decoded[".runner"]["serverUrlV2"], base);
    (runner.id, decoded)
}

fn queue(base: &str, repo: &str, name: &str) -> u64 {
    let job = json!({ "repository": repo, "name": name, "labels": LABELS, "steps": ["echo one"] });
    let (status, v) = call("POST", &format!("{base}_fake/jobs"), None, &job);
    assert_eq!(status, 201, "{v}");
    v["id"].as_u64().unwrap()
}

/// githome's G-30: two jobs of one repository with the same labels; the
/// second job goes to the runner minted for the first; both conclusions
/// come back, in the webhooks and in the control API.
#[test]
fn the_second_job_runs_on_the_runner_minted_for_the_first() {
    need_loopback!();
    let hooks = hooks();
    let fake = program(&hooks.url());
    let base = fake.base.as_str();
    let (_, app) = call("GET", &format!("{base}_fake/app"), None, &Value::Null);
    let (key, secret) =
        (Key::from_pem(app["private_key"].as_str().unwrap()).unwrap(), app["webhook_secret"].as_str().unwrap());
    let repo = "example-org/app";
    let (first, second) = (queue(base, repo, "build-1"), queue(base, repo, "build-2"));
    for job in [first, second] {
        let h = hooks.next(secret);
        assert_eq!((h["action"].as_str(), h["workflow_job"]["id"].as_u64()), (Some("queued"), Some(job)));
        assert_eq!(h["workflow_job"]["labels"], json!(LABELS));
        assert_eq!(h["repository"]["full_name"], repo);
    }

    // The App mints a runner for the first job; GitHub gives it the second.
    let token = installation_token(base, &key, repo);
    let (runner_a, files_a) = mint(base, &token, repo, "runner-a");
    let (status, v) =
        call("POST", &format!("{base}_fake/jobs/{second}/assign"), None, &json!({ "runner_id": runner_a }));
    assert_eq!(status, 200, "{v}");
    let a = Runner::connect(base, &files_a).unwrap();
    assert_eq!(a.poll(), Some(second), "the runner of the first job gets the second");
    assert_eq!(a.run(second, "succeeded")["jobDisplayName"], "build-2");
    let h = hooks.next(secret);
    assert_eq!((h["action"].as_str(), h["workflow_job"]["id"].as_u64()), (Some("in_progress"), Some(second)));
    assert_eq!(h["workflow_job"]["runner_name"], "runner-a");
    let h = hooks.next(secret);
    assert_eq!((h["action"].as_str(), h["workflow_job"]["conclusion"].as_str()), (Some("completed"), Some("success")));
    assert_eq!(a.poll(), None, "an ephemeral runner gets one job");

    // A runner minted later takes the first job, which no runner got.
    let (_, files_b) = mint(base, &token, repo, "runner-b");
    let b = Runner::connect(base, &files_b).unwrap();
    assert_eq!(b.poll(), Some(first));
    b.run(first, "failed");
    let h = hooks.next(secret);
    assert_eq!(
        (h["action"].as_str(), h["workflow_job"]["runner_name"].as_str()),
        (Some("in_progress"), Some("runner-b"))
    );
    let h = hooks.next(secret);
    assert_eq!(
        (h["workflow_job"]["id"].as_u64(), h["workflow_job"]["conclusion"].as_str()),
        (Some(first), Some("failure"))
    );

    // Both conclusions, from the control API and from the REST API.
    let (_, jobs) = call("GET", &format!("{base}_fake/jobs"), None, &Value::Null);
    let seen: Vec<(u64, &str, &str, &str)> = jobs
        .as_array()
        .unwrap()
        .iter()
        .map(|j| {
            let s = |k: &str| j[k].as_str().unwrap_or("");
            (j["id"].as_u64().unwrap(), s("status"), s("conclusion"), s("runner_name"))
        })
        .collect();
    assert_eq!(seen, [(first, "completed", "failure", "runner-b"), (second, "completed", "success", "runner-a")]);
    let (status, job) = call("GET", &format!("{base}repos/{repo}/actions/jobs/{second}"), Some(&token), &Value::Null);
    assert_eq!((status, job["conclusion"].as_str()), (200, Some("success")));
    let (_, deliveries) = call("GET", &format!("{base}_fake/deliveries"), None, &Value::Null);
    assert!(deliveries.as_array().unwrap().iter().all(|d| d["status"] == 200 || d["status"].is_null()), "{deliveries}");
}

/// The App gets a token only with a JWT of the made-up key, of its id, alive
/// and short; the API takes only a token that the fake gave.
#[test]
fn the_app_gets_a_token_only_with_a_good_jwt() {
    need_loopback!();
    let fake = FakeGithub::start().unwrap();
    let base = fake.base();
    let key = Key::made_up();
    let url = format!("{base}app/installations/1/access_tokens");
    let t = now();
    for (claims, why) in [
        (json!({ "iat": t - 30, "exp": t + 540, "iss": "2" }), "another App"),
        (json!({ "iat": t - 600, "exp": t - 10, "iss": APP_ID }), "expired"),
        (json!({ "iat": t - 30, "exp": t + 3600, "iss": APP_ID }), "a life over ten minutes"),
        (json!({ "iss": APP_ID }), "no times"),
    ] {
        assert_eq!(call("POST", &url, Some(&key.jwt(&claims)), &Value::Null).0, 401, "{why}");
    }
    let good = key.jwt(&json!({ "iat": t - 30, "exp": t + 540, "iss": APP_ID }));
    let (head, rest) = good.split_once('.').unwrap();
    let (_, sig) = rest.rsplit_once('.').unwrap();
    let longer = json!({ "iat": t - 30, "exp": t + 3600, "iss": APP_ID }).to_string();
    let forged = format!("{head}.{}.{sig}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(longer));
    assert_eq!(call("POST", &url, Some(&forged), &Value::Null).0, 401, "a changed JWT");
    assert_eq!(call("POST", &url, None, &Value::Null).0, 401, "no JWT");
    assert_eq!(call("POST", &format!("{base}app/installations/2/access_tokens"), Some(&good), &Value::Null).0, 404);
    let jit = format!("{base}repos/o/r/actions/runners/generate-jitconfig");
    let body = json!({ "name": "r", "labels": LABELS });
    assert_eq!(call("POST", &jit, Some(&good), &body).0, 401, "the JWT is not an installation token");
    assert_eq!(call("POST", &jit, Some("fake-installation-0"), &body).0, 401, "a made-up token");
    let token = installation_token(base, &key, "o/r");
    assert_eq!(call("POST", &jit, Some(&token), &json!({ "name": "r" })).0, 422, "no label");
    assert_eq!(call("POST", &jit, Some(&token), &body).0, 201);
    assert_eq!(call("POST", &jit, Some(&token), &body).0, 409, "the same name");
}

/// A runner gets no job of another repository or with a label that it
/// lacks, and the control API gives it none; a client id that the fake did
/// not mint gets no token.
#[test]
fn a_runner_gets_only_a_job_of_its_repository_and_labels() {
    need_loopback!();
    let fake = FakeGithub::start().unwrap();
    let base = fake.base();
    let token = installation_token(base, &Key::made_up(), "o/r");
    let other = queue(base, "o/other", "elsewhere");
    let job = json!({ "repository": "o/r", "name": "gpu", "labels": ["self-hosted", "gpu"] });
    let (_, gpu) = call("POST", &format!("{base}_fake/jobs"), None, &job);
    let (id, files) = mint(base, &token, "o/r", "plain");
    for j in [other, gpu["id"].as_u64().unwrap()] {
        let (status, _) = call("POST", &format!("{base}_fake/jobs/{j}/assign"), None, &json!({ "runner_id": id }));
        assert_eq!(status, 409, "job {j}");
    }
    let runner = Runner::connect(base, &files).unwrap();
    assert_eq!(runner.poll(), None);
    assert_eq!(call("GET", &format!("{base}message"), Some("made-up"), &Value::Null).0, 401, "a made-up token");
    let mut stranger = files.clone();
    stranger[".credentials"]["data"]["clientId"] = json!("00000000-0000-4000-8000-999999999999");
    assert_eq!(Runner::connect(base, &stranger).err(), Some(401));
}

/// podrunner's `container:` and `services:` jobs (PodUtils/podrunner#373):
/// the job message gives both as the runner reads them, a container that
/// the runner refuses is refused, and the control API gives the log of each
/// step of a job that ended: 404 for no such job, 409 before its end.
#[test]
fn a_container_job_gives_its_containers_and_the_log_of_each_step() {
    need_loopback!();
    let fake = FakeGithub::start().unwrap();
    let base = fake.base();
    let jobs = format!("{base}_fake/jobs");
    let bad =
        json!({ "repository": "o/r", "name": "bad", "labels": LABELS, "container": { "image": "a", "command": "x" } });
    assert_eq!(call("POST", &jobs, None, &bad).0, 422, "a key that the runner refuses");
    let job = json!({
        "repository": "o/r", "name": "with-db", "labels": LABELS, "steps": ["echo one", "echo two"],
        "container": { "image": "node:22", "env": { "CI": true }, "options": "--cpus 1" },
        "services": { "db": { "image": "postgres:16", "ports": [5432], "volumes": ["/d:/d"] }, "cache": "redis:7" },
    });
    let (status, v) = call("POST", &jobs, None, &job);
    assert_eq!(status, 201, "{v}");
    let id = v["id"].as_u64().unwrap();
    assert_eq!(call("GET", &format!("{jobs}/{id}/logs"), None, &Value::Null).0, 409, "a queued job");
    assert_eq!(call("GET", &format!("{jobs}/999/logs"), None, &Value::Null).0, 404, "no such job");

    let token = installation_token(base, &Key::made_up(), "o/r");
    let (_, files) = mint(base, &token, "o/r", "runner-c");
    let runner = Runner::connect(base, &files).unwrap();
    assert_eq!(runner.poll(), Some(id));
    let m = runner.run(id, "failed");
    let pair = |k: &str, v: Value| json!({ "Key": k, "Value": v });
    let c = &m["jobContainer"]["map"];
    assert!(c.as_array().unwrap().contains(&pair("image", json!("node:22"))), "{c}");
    assert!(c.as_array().unwrap().contains(&pair("env", json!({ "type": 2, "map": [pair("CI", json!("true"))] }))));
    let s = m["jobServiceContainers"]["map"].as_array().unwrap();
    let db = &s.iter().find(|p| p["Key"] == "db").unwrap()["Value"]["map"];
    assert!(db.as_array().unwrap().contains(&pair("ports", json!({ "type": 1, "seq": ["5432"] }))), "{db}");
    assert!(s.contains(&pair("cache", json!("redis:7"))), "{s:?}");

    let (status, logs) = call("GET", &format!("{jobs}/{id}/logs"), None, &Value::Null);
    assert_eq!(status, 200, "{logs}");
    assert_eq!(
        (logs["conclusion"].as_str(), logs["job_log"].as_str()),
        (Some("failure"), Some("the log of the job\n"))
    );
    let steps: Vec<(u64, &str, &str)> = logs["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["number"].as_u64().unwrap(), s["name"].as_str().unwrap(), s["log"].as_str().unwrap()))
        .collect();
    assert_eq!(
        steps,
        [(2, "Run step 1", "a line of step 1\nthe end of step 1\n"), (3, "Run step 2", "a line of step 2\n")]
    );
}
