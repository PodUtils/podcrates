//! The fake GitHub (feature `github`): the acceptance of githome's G-30
//! through the binary `pod-fake-github`, and the refusals of the fake. The
//! test is the App, its webhook receiver and the runner: it speaks the
//! routes that the official runner calls.
#![cfg(feature = "github")]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use pod_testkit::github::{APP_ID, FakeGithub, Key, hub_signature};
use pod_testkit::need_loopback;
use pod_testkit::stand::{self, Answer, Request, Stand};
use serde_json::{Value, json};

/// The labels of each job and runner of the acceptance.
const LABELS: [&str; 2] = ["self-hosted", "linux"];

/// The binary on a free port; it ends with the test.
struct Program {
    child: Child,
    base: String,
}

impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn program(hook: &str) -> Program {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pod-fake-github"))
        .args(["--hook", hook])
        .stdout(Stdio::piped())
        .spawn()
        .expect("pod-fake-github starts");
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).expect("the base URL");
    Program { child, base: line.trim().to_string() }
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

/// One call with an optional credential and a JSON body; the status and the
/// JSON of the answer (`Null` for no JSON).
fn call(method: &str, url: &str, token: Option<&str>, body: &Value) -> (u16, Value) {
    let auth = token.map(|t| format!("Bearer {t}"));
    let mut headers = vec![("Content-Type", "application/json")];
    if let Some(a) = &auth {
        headers.push(("Authorization", a.as_str()));
    }
    let raw = if body.is_null() { Vec::new() } else { body.to_string().into_bytes() };
    let (status, out) = stand::call(method, url, &headers, &raw).unwrap();
    (status, serde_json::from_slice(&out).unwrap_or(Value::Null))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

/// The App: its JWT, the installation of the repository, a token.
fn installation_token(base: &str, key: &Key, repo: &str) -> String {
    let jwt = key.jwt(&json!({ "iat": now() - 30, "exp": now() + 540, "iss": APP_ID.to_string() }));
    let (status, inst) = call("GET", &format!("{base}repos/{repo}/installation"), Some(&jwt), &Value::Null);
    assert_eq!(status, 200, "{inst}");
    let url = inst["access_tokens_url"].as_str().unwrap();
    let (status, token) = call("POST", url, Some(&jwt), &Value::Null);
    assert_eq!(status, 201, "{token}");
    token["token"].as_str().unwrap().to_string()
}

/// A JIT runner of `repo`: its id and its three files, decoded.
fn mint(base: &str, token: &str, repo: &str, name: &str) -> (u64, Value) {
    let url = format!("{base}repos/{repo}/actions/runners/generate-jitconfig");
    let (status, v) = call("POST", &url, Some(token), &json!({ "name": name, "runner_group_id": 1, "labels": LABELS }));
    assert_eq!(status, 201, "{v}");
    let files: Value =
        serde_json::from_slice(&STANDARD.decode(v["encoded_jit_config"].as_str().unwrap()).unwrap()).unwrap();
    let file = |n: &str| -> Value {
        let raw = STANDARD.decode(files[n].as_str().unwrap()).unwrap();
        serde_json::from_slice(&raw).unwrap()
    };
    let decoded = json!({
        ".runner": file(".runner"), ".credentials": file(".credentials"),
        ".credentials_rsaparams": file(".credentials_rsaparams"),
    });
    assert_eq!(decoded[".runner"]["agentName"], name);
    assert_eq!(decoded[".runner"]["serverUrlV2"], base);
    (v["runner"]["id"].as_u64().unwrap(), decoded)
}

/// A runner of the minted files, with its OAuth token and its session.
struct Runner {
    base: String,
    token: String,
}

impl Runner {
    fn connect(base: &str, files: &Value) -> Result<Runner, u16> {
        let key = Key::made_up();
        let params: Value = serde_json::from_str(&key.rsaparams()).unwrap();
        assert_eq!(files[".credentials_rsaparams"], params, "the runner signs with the made-up key");
        let data = &files[".credentials"]["data"];
        let (client, url) = (data["clientId"].as_str().unwrap(), data["authorizationUrl"].as_str().unwrap());
        let assertion = key.jwt(&json!({ "sub": client, "iss": client, "aud": url, "exp": now() + 300 }));
        let form = format!(
            "grant_type=client_credentials&client_assertion_type=urn%3Aietf%3Aparams%3Aoauth%3Aclient-assertion-type%3Ajwt-bearer&client_assertion={assertion}"
        );
        let headers = [("Content-Type", "application/x-www-form-urlencoded")];
        let (status, out) = stand::call("POST", url, &headers, form.as_bytes()).unwrap();
        if status != 200 {
            return Err(status);
        }
        let token = serde_json::from_slice::<Value>(&out).unwrap()["access_token"].as_str().unwrap().to_string();
        let runner = Runner { base: base.to_string(), token };
        let agent = json!({ "agent": { "id": files[".runner"]["agentId"], "name": files[".runner"]["agentName"] } });
        let (status, session) = call("POST", &format!("{base}session"), Some(&runner.token), &agent);
        assert_eq!(status, 200);
        assert!(session["sessionId"].is_string());
        Ok(runner)
    }

    /// The request id of the next message, or none when the poll ends empty.
    fn poll(&self) -> Option<u64> {
        let (status, m) = call("GET", &format!("{}message?status=Online", self.base), Some(&self.token), &Value::Null);
        if status == 202 {
            return None;
        }
        assert_eq!(status, 200, "{m}");
        let body: Value = serde_json::from_str(m["body"].as_str().unwrap()).unwrap();
        Some(body["runner_request_id"].as_str().unwrap().parse().unwrap())
    }

    /// Acquire the job, renew its lock and complete it with `conclusion`,
    /// with the job token as the Worker does; the job message.
    fn run(&self, request: u64, conclusion: &str) -> Value {
        let ask = json!({ "jobMessageId": request.to_string(), "runnerOS": "Linux" });
        let (status, m) = call("POST", &format!("{}acquirejob", self.base), Some(&self.token), &ask);
        assert_eq!(status, 200, "{m}");
        let job = m["resources"]["endpoints"][0]["authorization"]["parameters"]["AccessToken"].as_str().unwrap();
        let (status, _) = call("POST", &format!("{}renewjob", self.base), Some(job), &json!({ "jobId": m["jobId"] }));
        assert_eq!(status, 200);
        let logs = format!("{}twirp/results.services.receiver.Receiver/GetStepLogsSignedBlobURL", self.base);
        let (status, blob) = call("POST", &logs, Some(job), &json!({}));
        assert_eq!(status, 200);
        assert_eq!(stand::call("PUT", blob["logs_url"].as_str().unwrap(), &[], b"a line").unwrap().0, 201);
        let end = json!({ "planId": m["plan"]["planId"], "jobId": m["jobId"], "conclusion": conclusion });
        let (status, _) = call("POST", &format!("{}completejob", self.base), Some(job), &end);
        assert_eq!(status, 200);
        m
    }
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
