//! The fake of githome's source (feature `githome`) through the binary
//! `pod-fake-githome`: the steps of podrunner's `source-conformance`
//! against the program `source`, one job from its offer to its report, and
//! the refusals and the settings of the fake. The test is podrunner: it
//! writes request lines to the program and reads its reply lines.
#![cfg(feature = "githome")]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use pod_job::source::{
    Event, HostFacts, KindFacts, LINE_MAX, RealJob, Reply, Request, SourceCode, parse_reply, request_line,
};
use pod_job::{Boundary, ContainerRoute, JobId, JobOffer};
use pod_testkit::githome::SERVER_VAR;
use pod_testkit::need_loopback;
use pod_testkit::stand;
use serde_json::{Map, Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_pod-fake-githome");

/// `pod-fake-githome serve` on a free port; it ends with the test.
struct Serve {
    child: Child,
    base: String,
}

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn serve() -> Serve {
    let mut child = Command::new(BIN).arg("serve").stdout(Stdio::piped()).spawn().expect("serve starts");
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).expect("the base URL");
    Serve { child, base: line.trim().to_string() }
}

impl Serve {
    /// One call of the control API; the status and the JSON of the answer.
    fn control(&self, method: &str, path: &str, body: &Value) -> (u16, Value) {
        let url = format!("{}_fake/{path}", self.base);
        let (status, bytes) = stand::call(method, &url, &[], body.to_string().as_bytes()).expect("the control API");
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    fn queue(&self, body: Value) -> JobId {
        let (status, job) = self.control("POST", "jobs", &body);
        assert_eq!(status, 201, "{job}");
        JobId::new(job["job_id"].as_str().unwrap()).unwrap()
    }

    fn log(&self, name: &str) -> Vec<Value> {
        let (status, v) = self.control("GET", name, &Value::Null);
        assert_eq!(status, 200);
        v.as_array().cloned().unwrap()
    }

    fn status(&self, job: &JobId) -> String {
        self.control("GET", &format!("jobs/{job}"), &Value::Null).1["status"].as_str().unwrap().to_string()
    }

    /// A fresh `pod-fake-githome source`: one host of podrunner.
    fn source(&self) -> Source {
        let mut child = Command::new(BIN)
            .arg("source")
            .env(SERVER_VAR, &self.base)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("source starts");
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Source { child, stdin, stdout }
    }
}

struct Source {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Drop for Source {
    fn drop(&mut self) {
        // The end of stdin ends the program.
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

impl Source {
    fn raw(&mut self, line: &[u8]) -> Reply {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(line).and_then(|()| stdin.write_all(b"\n")).and_then(|()| stdin.flush()).unwrap();
        let mut reply = String::new();
        self.stdout.read_line(&mut reply).expect("a reply line");
        parse_reply(reply.as_bytes()).unwrap_or_else(|e| panic!("a reply of the wire: {e}: {reply}"))
    }

    fn ask(&mut self, req: Request) -> Reply {
        self.raw(request_line(&req).trim_end().as_bytes())
    }

    fn hello(&mut self, host: Option<HostFacts>) -> Reply {
        self.ask(Request::Hello { client: "podrunner".into(), version: "0".into(), host })
    }

    /// The events of `next`.
    fn next(&mut self, capacity: u32, wait_s: u32) -> Vec<Event> {
        match self.ask(Request::Next { capacity, wait_s }) {
            Reply::Events { events } => events,
            other => panic!("next: {other:?}"),
        }
    }

    fn offers(&mut self, capacity: u32) -> Vec<JobId> {
        let events = self.next(capacity, 5);
        events.iter().filter(|e| matches!(e, Event::Offer { .. })).map(|e| e.job_id().clone()).collect()
    }

    fn ack(&mut self, job: &JobId) -> Reply {
        self.ask(Request::Ack { job_id: job.clone() })
    }

    fn claim(&mut self, job: &JobId, runner: &str) -> Reply {
        self.ask(Request::Claim { job_id: job.clone(), runner_name: runner.into() })
    }

    fn mint(&mut self, job: &JobId, runner: &str) -> Reply {
        self.ask(Request::Mint { job_id: job.clone(), runner_name: runner.into() })
    }

    fn report(&mut self, job: &JobId, report: Value) -> Reply {
        let Value::Object(report) = report else { panic!("a report is an object") };
        self.ask(Request::Report { job_id: job.clone(), report })
    }

    fn withdraw(&mut self, job: &JobId, reason: &str) -> Reply {
        self.ask(Request::Withdraw { job_id: job.clone(), reason: reason.into() })
    }

    fn release(&mut self, job: &JobId) -> Reply {
        self.ask(Request::Release { job_id: job.clone() })
    }

    fn assigned(&mut self, job: &JobId) -> Reply {
        let real = RealJob {
            run_id: 2,
            job_name: "build".into(),
            event: "push".into(),
            repository: "example-org/app".into(),
            head_repository: None,
            head_is_fork: Some(false),
            actor: "conformance".into(),
            job_id: None,
        };
        self.ask(Request::Assigned { job_id: job.clone(), real })
    }

    /// A fresh offer of the source, acked and claimed for `runner-u-1`.
    fn claimed(&mut self) -> JobId {
        let job = self.offers(1).pop().expect("an offer");
        done(self.ack(&job));
        done(self.claim(&job, "runner-u-1"));
        job
    }
}

#[track_caller]
fn done(reply: Reply) {
    assert_eq!(reply, Reply::Done {});
}

#[track_caller]
fn code(reply: Reply, want: SourceCode) {
    match reply {
        Reply::Error { code, .. } => assert_eq!(code, want),
        other => panic!("{other:?}, not {}", want.as_str()),
    }
}

fn host(arch: &str) -> HostFacts {
    HostFacts {
        podrunner: "0".into(),
        podvm: Some("0.2.0".into()),
        podnet: Some("0.4.0".into()),
        arch: arch.into(),
        kvm: false,
        kinds: vec![KindFacts {
            name: "conformance".into(),
            boundary: Boundary::Confined,
            container: ContainerRoute::None,
            cpu: Some(2000),
            memory: Some(1 << 30),
            disk: None,
            pids: Some(512),
        }],
    }
}

/// The 14 tests of podrunner's `source-conformance`
/// (`docs/contracts/source-driver.md`), each with a fresh program on one
/// `serve` with two queued jobs, as podrunner runs them. Each test gives its
/// job back; the report and the release of test 7 end its job, so a run
/// ends one job and leaves the other queued.
#[test]
fn the_conformance_steps_of_podrunner_pass() {
    need_loopback!();
    let fake = serve();
    let jobs = [fake.queue(json!({ "labels": ["podrunner"] })), fake.queue(json!({ "labels": ["podrunner"] }))];
    // 1 hello-names-the-source
    let named = |r: Reply| match r {
        Reply::Hello { source, .. } if !source.is_empty() => source,
        other => panic!("hello: {other:?}"),
    };
    let plain = named(fake.source().hello(None));
    // 2 no-capacity-gives-no-offer
    assert!(fake.source().next(0, 0).is_empty());
    // 3 next-gives-at-most-capacity-offers
    assert_eq!(fake.source().offers(1).len(), 1);
    // 4 ack-of-an-offer
    let mut s = fake.source();
    let job = s.offers(1).pop().unwrap();
    done(s.ack(&job));
    code(s.ack(&JobId::new("never-offered-0").unwrap()), SourceCode::UnknownJob);
    done(s.withdraw(&job, "conformance"));
    // 5 a-job-is-claimed-once
    let mut s = fake.source();
    let job = s.claimed();
    code(s.claim(&job, "runner-u-2"), SourceCode::AlreadyClaimed);
    done(s.release(&job));
    // 6 mint-needs-a-claim
    let mut s = fake.source();
    let job = s.offers(1).pop().unwrap();
    done(s.ack(&job));
    code(s.mint(&job, "runner-u-1"), SourceCode::NotClaimed);
    done(s.claim(&job, "runner-u-1"));
    match s.mint(&job, "runner-u-1") {
        Reply::Credential { credential } => {
            assert!(!credential.files.is_empty());
            assert!(credential.files.iter().all(|f| !f.name.contains('/') && f.name != "." && f.name != ".."));
        }
        other => panic!("mint: {other:?}"),
    }
    done(s.release(&job));
    // 7 report-of-a-claimed-job
    let mut s = fake.source();
    let job = s.claimed();
    done(s.report(&job, json!({ "exit_reason": "success" })));
    done(s.release(&job));
    // 8 audit-is-accepted
    let mut line = Map::new();
    line.insert("kind".into(), "offer".into());
    done(fake.source().ask(Request::Audit { line }));
    // 9 withdraw-ends-an-offer
    let mut s = fake.source();
    let job = s.offers(1).pop().unwrap();
    done(s.ack(&job));
    done(s.withdraw(&job, "wait-deadline"));
    code(s.claim(&job, "runner-u-1"), SourceCode::UnknownJob);
    // 10 release-ends-a-claim
    let mut s = fake.source();
    let job = s.claimed();
    done(s.release(&job));
    code(s.claim(&job, "runner-u-2"), SourceCode::UnknownJob);
    // 11 an-unknown-op-is-bad-request
    code(fake.source().raw(br#"{"schema":1,"op":"steal","job_id":"j-1"}"#), SourceCode::BadRequest);
    // 12 the-wire-refuses-a-long-line-a-path-and-a-schema
    let mut long = br#"{"schema":1,"op":"audit","line":{"pad":""#.to_vec();
    long.resize(LINE_MAX + 1, b'a');
    long.extend_from_slice(br#""}}"#);
    for bad in [
        &long[..],
        br#"{"schema":1,"op":"ack","job_id":"../x"}"#,
        br#"{"schema":2,"op":"hello","client":"c","version":"v"}"#,
        br#"{"op":"hello","client":"c","version":"v"}"#,
    ] {
        code(fake.source().raw(bad), SourceCode::BadRequest);
    }
    // 13 assigned-needs-a-claim
    let mut s = fake.source();
    let job = s.offers(1).pop().unwrap();
    code(s.assigned(&job), SourceCode::NotClaimed);
    done(s.ack(&job));
    done(s.claim(&job, "runner-u-1"));
    assert!(matches!(s.assigned(&job), Reply::Keep { .. }));
    done(s.release(&job));
    // 14 hello-takes-the-facts-of-the-host
    assert_eq!(named(fake.source().hello(Some(host("x86_64")))), plain);
    drop(s);
    assert_eq!(fake.status(&jobs[0]), "done", "test 7 reports the oldest job and releases it");
    assert_eq!(fake.status(&jobs[1]), "queued", "each other test gave its job back");
    let refusals = fake.log("refusals");
    assert_eq!(refusals.iter().filter(|r| r["code"] == "bad-request").count(), 5, "{refusals:?}");
}

/// One job from its offer to its report, on a host of x86_64: the job of
/// aarch64 is not offered to it. The control API then reads the report.
#[test]
fn one_job_runs_from_its_offer_to_its_report() {
    need_loopback!();
    let fake = serve();
    let arm = fake.queue(json!({ "arch": "aarch64", "labels": ["podrunner"] }));
    let job = fake.queue(json!({ "labels": ["podrunner", "x86_64"], "repository": "example-org/site" }));
    let mut s = fake.source();
    assert_eq!(s.hello(Some(host("x86_64"))), Reply::Hello { source: "githome".into(), operator_controlled: true });
    let events = s.next(4, 0);
    let [Event::Offer { offer }] = events.as_slice() else { panic!("one offer: {events:?}") };
    assert_eq!(offer.job_id, job, "the job of aarch64 is not offered to x86_64");
    let mut whole = offer.fields.clone();
    whole.insert("job_id".into(), json!(offer.job_id));
    let parsed: JobOffer = serde_json::from_value(Value::Object(whole)).expect("a job-offer.v1");
    parsed.check(false).expect("the checks of an offer");
    assert_eq!(parsed.trust_inputs.repo, "site");
    done(s.ack(&job));
    done(s.claim(&job, "runner-u-1"));
    let Reply::Credential { credential } = s.mint(&job, "runner-u-1") else { panic!("a credential") };
    let names: Vec<&str> = credential.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, [".runner", ".credentials", ".credentials_rsaparams"]);
    let (status, _) = fake.control("POST", &format!("jobs/{job}/started"), &json!({}));
    assert_eq!(status, 200);
    let started = s.next(1, 5);
    assert_eq!(started, [Event::Started { job_id: job.clone(), runner_name: "runner-u-1".into() }]);
    let mut line = Map::new();
    line.insert("job_id".into(), json!(job));
    line.insert("verdict".into(), "allow".into());
    done(s.ask(Request::Audit { line }));
    done(s.report(&job, json!({ "exit_reason": "success", "cpu_s": 3 })));
    done(s.release(&job));
    assert_eq!(fake.status(&job), "done");
    assert_eq!(fake.status(&arm), "queued");
    let reports = fake.log("reports");
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["job_id"], json!(job));
    assert_eq!(reports[0]["runner_name"], "runner-u-1");
    assert_eq!(reports[0]["report"]["exit_reason"], "success");
    assert_eq!(fake.log("audits")[0]["line"]["verdict"], "allow");
    assert_eq!(fake.log("hosts")[0]["host"]["arch"], "x86_64");
    let mints = fake.log("mints");
    let client = credential.files[1].content.expose();
    assert_eq!(mints[0]["files"][".credentials"], client, "the control API shows the made-up credential");
    assert!(fake.log("refusals").is_empty());
}

/// Two hosts on one forge, and the refusals of githome: one claim, one
/// credential for each claim, no report or mint without the claim, a
/// refusal that the host never sees again, a cancel, and the settings.
#[test]
fn the_refusals_and_the_settings_of_the_fake() {
    need_loopback!();
    let fake = serve();
    let job = fake.queue(json!({ "job_id": "j-a" }));
    let other = fake.queue(json!({ "job_id": "j-b" }));
    let (mut one, mut two) = (fake.source(), fake.source());
    assert_eq!(one.offers(1), std::slice::from_ref(&job));
    assert_eq!(two.offers(1), std::slice::from_ref(&job), "a queued job goes to each host until a claim");
    done(one.claim(&job, "runner-u-1"));
    code(two.claim(&job, "runner-u-9"), SourceCode::AlreadyClaimed);
    code(two.report(&job, json!({ "exit_reason": "success" })), SourceCode::NotClaimed);
    code(two.withdraw(&JobId::new("j-zz").unwrap(), "cancelled"), SourceCode::UnknownJob);
    code(one.mint(&job, "runner-u-2"), SourceCode::NotClaimed);
    assert!(matches!(one.mint(&job, "runner-u-1"), Reply::Credential { .. }));
    code(one.mint(&job, "runner-u-1"), SourceCode::CredentialAlreadyIssued);
    code(one.withdraw(&job, "tier-mismatch"), SourceCode::AlreadyClaimed);
    // A lost job comes again; the refusal of a host keeps it from that host.
    done(one.report(&job, json!({ "exit_reason": "lost" })));
    done(one.release(&job));
    assert_eq!(fake.status(&job), "queued");
    done(two.withdraw(&job, "tier-mismatch"));
    assert_eq!(two.offers(2), std::slice::from_ref(&other), "a host gets no job that it refused");
    assert_eq!(one.offers(2), [job.clone(), other.clone()]);
    // A mint error of the control API, then a revoked source.
    let (status, _) = fake.control("POST", "jobs/j-b/mint-error", &json!({ "code": "credential-unavailable" }));
    assert_eq!(status, 204);
    done(two.claim(&other, "runner-u-3"));
    code(two.mint(&other, "runner-u-3"), SourceCode::CredentialUnavailable);
    fake.control("PUT", "settings", &json!({ "revoked": true }));
    code(two.mint(&other, "runner-u-3"), SourceCode::SourceRevoked);
    // A cancel goes to each host that holds the job.
    let (status, told) = fake.control("POST", "jobs/j-a/cancel", &Value::Null);
    assert_eq!((status, told["sessions"].as_u64()), (200, Some(1)));
    assert_eq!(one.next(1, 5), [Event::Cancel { job_id: job.clone() }]);
    code(one.claim(&job, "runner-u-1"), SourceCode::UnknownJob);
    fake.control("PUT", "settings", &json!({ "unavailable": true }));
    code(one.ask(Request::Next { capacity: 1, wait_s: 0 }), SourceCode::Unavailable);
    let codes: Vec<String> = fake.log("refusals").iter().map(|r| r["code"].as_str().unwrap().to_string()).collect();
    let want = [
        "already-claimed",
        "not-claimed",
        "unknown-job",
        "not-claimed",
        "credential-already-issued",
        "already-claimed",
        "credential-unavailable",
        "source-revoked",
        "unknown-job",
        "unavailable",
    ];
    assert_eq!(codes, want);
    let withdrawals = fake.log("withdrawals");
    assert_eq!(withdrawals[0]["refusal"], true, "{withdrawals:?}");
    assert_eq!(fake.log("releases")[0]["outcome"], "lost");
}

/// The program refuses a call with no server, and names the variable.
#[test]
fn the_program_needs_the_address_of_serve() {
    let out = Command::new(BIN).arg("source").env_remove(SERVER_VAR).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains(SERVER_VAR));
}
