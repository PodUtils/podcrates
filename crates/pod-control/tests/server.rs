//! The server on a Unix socket, with a client: each method, the tiers, the
//! budgets of calls back, the caps and the deadlines.

#![cfg(unix)]

mod common;

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use pod_control::{Client, ClientSettings, Code, Drain, Error, Fault, Tier, Token, unix};
use serde_json::{Value, json};

fn client(path: &std::path::Path) -> Client {
    Client::connect_unix(path, ClientSettings::default()).unwrap()
}

fn code(r: Result<Value, Error>) -> Code {
    match r {
        Err(Error::Remote(f)) => f.code,
        other => panic!("not an error of the server: {other:?}"),
    }
}

#[test]
fn the_socket_has_its_mode_and_answers_each_method() {
    let (server, path, log) = common::start("each", common::settings());
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    let mut c = client(&path);
    let v = c.version().unwrap();
    assert_eq!((v.program.as_str(), v.version.as_str(), v.protocol), ("pod-control-test", "1.0.0", 1));
    let status = c.status().unwrap();
    assert_eq!(status["slots"][0]["boundary"], "microvm");
    assert_eq!(c.metrics().unwrap().counters["jobs_done"], 3);
    assert_eq!(c.drain(Drain::default()).unwrap()["in_flight"], 2);
    assert_eq!(c.call("drain", json!({ "exit": false })).unwrap()["draining"], true);
    assert_eq!(c.stop().unwrap(), json!("stopping"));
    // reload has the master tier: this connection has the operate tier.
    assert_eq!(code(c.reload()), Code::UnknownMethod);
    assert_eq!(code(c.call("status", json!({ "x": 1 }))), Code::BadParams);
    let names: Vec<String> = c.methods().unwrap().into_iter().map(|m| m.name).collect();
    assert_eq!(names, ["version", "methods", "auth", "status", "metrics", "drain", "stop", "ask"]);
    assert_eq!(*log.lock().unwrap(), ["status operate", "metrics", "drain exit=true", "drain exit=false", "stop"]);
    drop(server);
    assert!(!path.exists(), "the drop of the server removes its socket");
}

#[test]
fn a_method_above_the_tier_answers_as_an_unknown_one() {
    let mut settings = common::settings();
    settings.same_user = Tier::Read;
    let (_server, path, log) = common::start("tier", settings);
    let mut c = client(&path);
    let unknown = c.raw(br#"{"v":1,"id":9,"method":"no-such-method"}"#).unwrap().unwrap();
    for name in ["drain", "stop", "reload"] {
        // Params that do not fit give no hint either: the tier comes first.
        let line = format!(r#"{{"v":1,"id":9,"method":"{name}","params":{{"bogus":1}}}}"#);
        assert_eq!(c.raw(line.as_bytes()).unwrap().unwrap(), unknown, "{name}");
    }
    let names: Vec<String> = c.methods().unwrap().into_iter().map(|m| m.name).collect();
    assert_eq!(names, ["version", "methods", "auth", "status", "metrics", "ask"]);
    assert!(log.lock().unwrap().is_empty(), "the host saw a hidden method: {:?}", log.lock().unwrap());
}

#[test]
fn auth_gives_the_master_tier_and_a_wrong_token_closes() {
    let mut settings = common::settings();
    settings.master = Some(Token::new("made-up-master-token"));
    let (_server, path, log) = common::start("auth", settings);
    let mut c = client(&path);
    assert_eq!(code(c.reload()), Code::UnknownMethod);
    assert_eq!(c.auth(&Token::new("made-up-master-token")).unwrap(), Tier::Master);
    assert_eq!(c.reload().unwrap(), json!("reloaded"));
    assert!(c.methods().unwrap().iter().any(|m| m.name == "reload"));
    assert_eq!(log.lock().unwrap().last().map(String::as_str), Some("reload"));
    let mut wrong = client(&path);
    match wrong.auth(&Token::new("made-up-other-token")) {
        Err(Error::Remote(f)) => {
            assert_eq!(f.code, Code::Refused);
            assert!(!f.detail.contains("made-up"), "the detail holds the token: {f}");
        }
        other => panic!("{other:?}"),
    }
    assert!(wrong.version().is_err(), "a wrong token ends the connection");
    // With no master token, auth is refused.
    let (_server, path, _) = common::start("auth-none", common::settings());
    assert_eq!(code(client(&path).call("auth", json!({ "token": "made-up-x" }))), Code::Refused);
}

#[test]
fn a_request_past_its_callback_budget_is_a_fault() {
    let (_server, path, _) = common::start("budget", common::settings());
    let mut c = client(&path);
    let mut seen = 0;
    let mut back = |m: &str, _: &Value| -> Result<Value, Fault> {
        assert_eq!(m, "confirm");
        seen += 1;
        Ok(json!(true))
    };
    let r = c.call_with("ask", json!({ "n": pod_control::CALLBACKS_PER_REQUEST }), &mut back).unwrap();
    assert_eq!(r, json!({ "answered": pod_control::CALLBACKS_PER_REQUEST }));
    // One call more than the budget: the request answers budget.
    let r = c.call_with("ask", json!({ "n": pod_control::CALLBACKS_PER_REQUEST + 1 }), &mut back);
    assert!(matches!(&r, Err(Error::Remote(f)) if f.code == Code::Budget), "{r:?}");
    assert_eq!(seen, 2 * pod_control::CALLBACKS_PER_REQUEST);
}

#[test]
fn the_calls_back_of_a_connection_have_a_total() {
    let mut settings = common::settings();
    settings.callbacks_per_connection = 5;
    let (_server, path, _) = common::start("total", settings);
    let mut c = client(&path);
    let mut back = |_: &str, _: &Value| -> Result<Value, Fault> { Ok(json!(true)) };
    assert_eq!(c.call_with("ask", json!({ "n": 3 }), &mut back).unwrap(), json!({ "answered": 3 }));
    let r = c.call_with("ask", json!({ "n": 3 }), &mut back);
    assert!(matches!(&r, Err(Error::Remote(f)) if f.code == Code::Budget), "{r:?}");
    assert!(c.version().is_err(), "the server ends a connection that spent its total");
}

#[test]
fn a_client_drops_a_server_past_its_budget() {
    let (_server, path, _) = common::start("client-budget", common::settings());
    let settings = ClientSettings { callbacks_per_request: 2, ..ClientSettings::default() };
    let mut c = Client::connect_unix(&path, settings).unwrap();
    let mut back = |_: &str, _: &Value| -> Result<Value, Fault> { Ok(json!(true)) };
    assert!(matches!(c.call_with("ask", json!({ "n": 3 }), &mut back), Err(Error::Budget)));
    assert!(matches!(c.version(), Err(Error::Closed)));
}

#[test]
fn a_call_back_that_the_client_refuses_reaches_the_host() {
    let (_server, path, _) = common::start("refused-back", common::settings());
    let r = client(&path).call("ask", json!({ "n": 2 })).unwrap();
    assert_eq!(r["answered"], 0);
    assert!(r["fault"].as_str().unwrap().contains("unsupported"), "{r}");
}

#[test]
fn a_slow_client_is_cut_at_the_read_timeout() {
    let mut settings = common::settings();
    settings.read_timeout = Duration::from_millis(300);
    let (_server, path, _) = common::start("slow", settings);
    let mut s = UnixStream::connect(&path).unwrap();
    let started = Instant::now();
    let mut closed = false;
    // One byte each 50 ms and never a line end: each read gets a byte, but
    // the line is not whole at the deadline.
    while started.elapsed() < Duration::from_secs(5) {
        if s.write_all(b" ").is_err() {
            closed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(closed, "the connection was not cut");
    assert!(started.elapsed() >= Duration::from_millis(300), "{:?}", started.elapsed());
}

#[test]
fn a_line_over_the_cap_closes_and_a_line_not_utf8_does_not() {
    let mut settings = common::settings();
    settings.line_max = 64;
    let (_server, path, _) = common::start("cap", settings);
    let mut c = client(&path);
    let a = c.raw(b"\xff\n").unwrap().unwrap();
    assert_eq!((a["id"].clone(), a["error"]["code"].clone()), (Value::Null, json!("not-utf8")));
    c.version().unwrap();
    let a = c.raw(&[b' '; 65]).unwrap().unwrap();
    assert_eq!(a["error"]["code"], "too-long");
    assert!(c.version().is_err());
    // A line at the cap is read.
    let mut c = client(&path);
    let mut line = br#"{"v":1,"id":1,"method":"version"}"#.to_vec();
    line.resize(64, b' ');
    assert_eq!(c.raw(&line).unwrap().unwrap()["ok"], true);
}

#[test]
fn status_takes_the_snapshot_while_the_host_is_busy() {
    let mut settings = common::settings();
    settings.answer_wait = Duration::from_millis(200);
    let path = common::dir("snapshot").join("ctl.sock");
    // No thread of the host answers.
    let (server, _incoming) = unix::serve(&path, settings, common::catalogue()).unwrap();
    let mut c = client(&path);
    assert_eq!(code(c.status()), Code::Busy);
    server.snapshot().set(json!({ "jobs": 3 }));
    assert_eq!(c.status().unwrap(), json!({ "jobs": 3 }));
    assert_eq!(code(c.drain(Drain::default())), Code::Busy);
    // The local methods answer while the host is busy.
    assert_eq!(c.version().unwrap().protocol, 1);
}

#[test]
fn a_client_past_the_limit_is_busy() {
    let mut settings = common::settings();
    settings.clients = 2;
    let (server, path, _) = common::start("clients", settings);
    let idle: Vec<Client> = (0..2).map(|_| client(&path)).collect();
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.active() < 2 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    let mut s = UnixStream::connect(&path).unwrap();
    let mut line = String::new();
    s.read_to_string(&mut line).unwrap();
    assert!(line.contains("\"busy\""), "{line}");
    drop(idle);
}

#[test]
fn bind_keeps_a_live_socket_and_a_file_and_replaces_a_stale_socket() {
    let dir = common::dir("bind");
    let path = dir.join("ctl.sock");
    let live = unix::bind(&path, 0o600).unwrap();
    assert_eq!(unix::bind(&path, 0o600).unwrap_err().kind(), std::io::ErrorKind::AddrInUse);
    drop(live);
    assert!(unix::bind(&path, 0o600).is_ok(), "the socket of a dead listener is replaced");
    let file = dir.join("file");
    std::fs::write(&file, "x").unwrap();
    assert_eq!(unix::bind(&file, 0o600).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&file).unwrap(), b"x");
}

/// A `busy` line that comes, with a close, before the request is written is
/// still the answer, not a broken pipe (podrunner's test of its client).
#[test]
fn a_reply_before_the_request_is_read() {
    let path = common::dir("early").join("ctl.sock");
    let l = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let server = std::thread::spawn(move || {
        let (s, _) = l.accept().unwrap();
        (&s).write_all(
            b"{\"v\":1,\"id\":null,\"ok\":false,\"error\":{\"code\":\"busy\",\"detail\":\"too many clients\"}}\n",
        )
        .unwrap();
    });
    let mut c = client(&path);
    server.join().unwrap();
    // The server side is closed now: the write of the request can fail.
    assert_eq!(code(c.status()), Code::Busy);
}
