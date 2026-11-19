//! The parts that the tests of both fakes share: a binary of a fake on a
//! free port, one call of JSON, and the official runner of minted files
//! against the broker of the fake GitHub.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use pod_testkit::github::Key;
use pod_testkit::github::client::call_json;
use pod_testkit::stand;
use serde_json::{Map, Value, json};

/// A binary of a fake on a free port; it ends with the test.
pub struct Program {
    child: Child,
    /// The base URL, the first line of its stdout.
    pub base: String,
}

impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start the binary `bin` with `args`, and read its base URL.
pub fn start(bin: &str, args: &[&str]) -> Program {
    let mut child = Command::new(bin).args(args).stdout(Stdio::piped()).spawn().expect("the fake starts");
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).expect("the base URL");
    Program { child, base: line.trim().to_string() }
}

/// One call with an optional credential and a JSON body; the status and the
/// JSON of the answer (`Null` for no JSON).
pub fn call(method: &str, url: &str, token: Option<&str>, body: &Value) -> (u16, Value) {
    call_json(method, url, token, body).unwrap()
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

/// The JSON of each file of a runner, by its name.
pub fn files<'a>(named: impl IntoIterator<Item = (&'a str, &'a str)>) -> Value {
    let map: Map<String, Value> =
        named.into_iter().map(|(name, text)| (name.to_string(), serde_json::from_str(text).unwrap())).collect();
    Value::Object(map)
}

/// A runner of the minted files, with its OAuth token and its session.
pub struct Runner {
    base: String,
    token: String,
}

impl Runner {
    pub fn connect(base: &str, files: &Value) -> Result<Runner, u16> {
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
    pub fn poll(&self) -> Option<u64> {
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
    pub fn run(&self, request: u64, conclusion: &str) -> Value {
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
