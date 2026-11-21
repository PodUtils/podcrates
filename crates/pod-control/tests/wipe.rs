//! No block of memory that the server or the client frees holds a token of
//! a line (T-1023): the master token of `auth`, a token in the params of a
//! delegated request and in the answer of a call back, and a token in each
//! line that the server refuses. A token with a JSON escape (`\n`, `\"`,
//! `\\`, `\uXXXX`) is read into a buffer of its own (T-1048). The answer of
//! the host and of a call back is wiped on each path where the server drops
//! it (T-1060): after the write, past the budget of calls back, after `busy`,
//! and after the wait of [`pod_control::Context::call`].
//! The allocator of this binary searches each freed block for the marker
//! ([`pod_testkit::freed`]), so the binary has this one test.

#![cfg(unix)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pod_control::{
    CallFault, Catalogue, Client, ClientSettings, Code, Context, Drain, Error, Extra, Fault, Host, Metrics, Outcome,
    Tier, Token, unix, wire,
};
use pod_testkit::freed::{Scanning, scan};
use serde_json::{Value, json};
use zeroize::Zeroizing;

#[global_allocator]
static ALLOC: Scanning = Scanning;

/// The start of each made-up token of the test.
macro_rules! marker {
    () => {
        "made-up-wipe-marker-5c1e"
    };
}

const MARKER: &str = marker!();

#[test]
fn no_freed_block_holds_a_token_of_a_line() {
    // The check finds a copy that is not wiped.
    let ((), found) = scan(MARKER.as_bytes(), || drop(std::hint::black_box(MARKER.to_string())));
    let found = found.expect("the scanning allocator is the allocator of this binary");
    assert!(found.blocks >= 1, "the search found no unwiped copy: {found:?}");

    let ((), found) = scan(MARKER.as_bytes(), session);
    assert_eq!(found.map(|f| f.blocks), Some(0), "freed blocks with a token: {found:?}");

    // Each path of the server that drops an answer, in a scope of its own.
    let found: Vec<_> = [
        ("the answer of the host after its write", answer as fn(&Path)),
        ("the answer of the host past the budget of calls back", spent),
        ("the answer of the host after busy", late),
        ("the params of a call back after busy", late_back),
        ("the answer of a call back after the wait of the host", slow_back),
    ]
    .into_iter()
    .map(|(name, f)| (name, scan(MARKER.as_bytes(), || side(f)).1.map(|f| f.blocks)))
    .filter(|(_, blocks)| *blocks != Some(0))
    .collect();
    assert!(found.is_empty(), "freed blocks with a token: {found:?}");
}

fn client(path: &std::path::Path) -> Client {
    Client::connect_unix(path, ClientSettings::default()).unwrap()
}

/// A server with a master token, each kind of line that holds a token, and
/// the end of each thread of the server.
fn session() {
    let path = common::dir("wipe").join("ctl.sock");
    let mut settings = common::settings();
    settings.master = Some(Token::new(concat!(marker!(), "-master")));
    let (server, incoming) = unix::serve(&path, settings, common::catalogue()).unwrap();
    let host = std::thread::spawn(move || incoming.serve(&mut common::Recorder { log: common::Log::default() }));

    let mut c = client(&path);
    assert_eq!(c.auth(&Token::new(concat!(marker!(), "-master"))).unwrap(), Tier::Master);
    // A delegated request. The token comes first in the line, and the line
    // is long: the buffers of the frame grow after the token is in them.
    let params = json!({ "a_token": concat!(marker!(), "-job"), "n": 1, "z_pad": "x".repeat(40_000) });
    let v = c.call_with("ask", params, &mut |_, _| Ok(json!(true))).unwrap();
    assert_eq!(v["answered"], 1);
    // A token with each escape of a string that the client writes: the line
    // of the request, and the answer of a call back.
    let escaped = concat!(marker!(), "-quote\"back\\slash\nline\u{1}control");
    let params = json!({ "a_token": escaped, "n": 1 });
    let v = c.call_with("ask", params, &mut |_, _| Ok(json!({ "back": escaped }))).unwrap();
    assert_eq!(v["answered"], 1);
    // Lines that the server refuses, each with a token.
    for (line, code) in [
        (concat!(r#"{"schema":1,"id":2,"method":"ask","params":{"t":""#, marker!(), r#""},"bogus":1}"#), "bad-request"),
        (concat!(r#"{"schema":1,"id":3,"method":"ask","params":[""#, marker!(), r#""]}"#), "bad-request"),
        (concat!(r#"{"schema":1,"id":4,"ok":true,"result":""#, marker!(), r#""}"#), "bad-request"),
        (concat!(r#"[""#, marker!(), r#""]"#), "bad-request"),
        (concat!(r#"{"schema":2,"t":""#, marker!(), r#""}"#), "bad-request"),
        (concat!(r#"{"schema":1,"id":5,"method":"version","params":{"t":""#, marker!(), r#""}}"#), "bad-params"),
        (concat!(r#"{"schema":1,"id":6,"method":"status","params":{"t":""#, marker!(), r#""}}"#), "bad-params"),
        // A token with an escape (T-1048).
        (concat!(r#"{"schema":1,"id":7,"method":"version","params":{"t":""#, marker!(), r#"\n"}}"#), "bad-params"),
        (concat!(r#"{"schema":1,"id":8,"method":"version","params":{"t":""#, marker!(), r#"\"q"}}"#), "bad-params"),
        (concat!(r#"{"schema":1,"id":9,"method":"version","params":{"t":""#, marker!(), r#"\\"}}"#), "bad-params"),
        (concat!(r#"{"schema":1,"id":10,"method":"version","params":{"t":""#, marker!(), r#"\u0041"}}"#), "bad-params"),
        (concat!(r#"{"schema":1,"id":11,"method":"version","params":{"t":"\t"#, marker!(), r#""}}"#), "bad-params"),
        (concat!(r#"{"schema":1,"id":12,"method":"version","params":{""#, marker!(), r#"\n":1}}"#), "bad-params"),
        // A token in a part of a line that is not JSON, and a token in a
        // field that a second field of the same name replaces.
        (concat!(r#"{"schema":1,"id":13,"method":"version","params":{"t":""#, marker!(), r#""}"#), "bad-request"),
        (concat!(r#"{"schema":1,"id":14,"method":"version","params":{"t":""#, marker!(), r#"\n"},"#), "bad-request"),
        (
            concat!(r#"{"schema":1,"id":15,"method":"version","params":{"t":""#, marker!(), r#""},"params":{"u":1}}"#),
            "bad-params",
        ),
    ] {
        let answer: Value = c.raw(line.as_bytes()).unwrap().unwrap();
        assert_eq!(answer["error"]["code"], code, "{answer}");
    }
    let mut not_utf8 = Zeroizing::new(Vec::with_capacity(MARKER.len() + 2));
    not_utf8.extend_from_slice(MARKER.as_bytes());
    not_utf8.extend_from_slice(b"\xff\n");
    assert_eq!(c.raw(&not_utf8).unwrap().unwrap()["error"]["code"], "not-utf8");
    drop(not_utf8);
    // A wrong master token closes its connection.
    let mut wrong = client(&path);
    assert!(matches!(wrong.auth(&Token::new(concat!(marker!(), "-wrong"))), Err(Error::Remote(_))));
    // A line past the cap closes its connection; the token is at its start.
    let mut long = UnixStream::connect(&path).unwrap();
    long.write_all(MARKER.as_bytes()).unwrap();
    long.write_all(&vec![b'x'; pod_control::LINE_MAX + 1]).unwrap();
    let _ = std::io::Read::read_to_end(&mut long, &mut Vec::new());
    drop((c, wrong, long));
    close(server, host);
}

/// Wait for the end of each thread of `server` and of `host`.
fn close(server: unix::Server, host: JoinHandle<()>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while server.active() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(server.active(), 0, "a connection did not end");
    drop(server);
    host.join().unwrap();
    // The threads of the connections drop the shared settings after their
    // slot: give them the time to end.
    std::thread::sleep(Duration::from_millis(200));
}

/// The wait of the host thread, past the answer wait of [`side`].
const LATE: Duration = Duration::from_millis(600);

/// A host whose answers and calls back hold a token, for each side path of
/// the server.
struct Sly;

impl Host for Sly {
    fn status(&mut self, _: &mut Context) -> Outcome {
        Ok(serde_json::json!({ "schema": 1 }))
    }

    fn metrics(&mut self, _: &mut Context) -> Result<Metrics, Fault> {
        Ok(Metrics::default())
    }

    fn reload(&mut self, _: &mut Context) -> Outcome {
        Ok(Value::Null)
    }

    fn drain(&mut self, _: Drain, _: &mut Context) -> Outcome {
        Ok(Value::Null)
    }

    fn stop(&mut self, _: &mut Context) -> Outcome {
        Ok(Value::Null)
    }

    fn extra(&mut self, name: &str, _: &Value, cx: &mut Context) -> Outcome {
        match name {
            "answer" => Ok(json!({ "t": concat!(marker!(), "-answer") })),
            "spent" => {
                let mut first = cx.call("confirm", json!({})).unwrap();
                wire::wipe(&mut first);
                assert_eq!(cx.call("confirm", json!({})), Err(CallFault::Budget));
                Ok(json!({ "t": concat!(marker!(), "-spent") }))
            }
            "late" => {
                std::thread::sleep(LATE);
                Ok(json!({ "t": concat!(marker!(), "-late") }))
            }
            "late-back" => {
                std::thread::sleep(LATE);
                let back = cx.call("confirm", json!({ "t": concat!(marker!(), "-late-back") }));
                assert_eq!(back, Err(CallFault::Gone));
                Ok(Value::Null)
            }
            "slow-back" => {
                assert_eq!(cx.call("confirm", json!({})), Err(CallFault::Gone));
                Ok(Value::Null)
            }
            _ => Err(Fault::new(Code::Unsupported, "no such method")),
        }
    }
}

/// Run `f` on a new server of [`Sly`]: one call back for each request, and
/// short waits.
fn side(f: fn(&Path)) {
    let path = common::dir("wipe-side").join("ctl.sock");
    let mut settings = common::settings();
    settings.read_timeout = Duration::from_millis(200);
    settings.write_timeout = Duration::from_millis(200);
    settings.answer_wait = Duration::from_millis(300);
    settings.callbacks_per_request = 1;
    let extras = ["answer", "spent", "late", "late-back", "slow-back"].map(|m| Extra::new(m, Tier::Read, "A test."));
    let (server, incoming) = unix::serve(&path, settings, Catalogue::new(extras.into()).unwrap()).unwrap();
    let host = std::thread::spawn(move || incoming.serve(&mut Sly));
    f(&path);
    close(server, host);
}

/// The server wipes the answer of the host after it writes it.
fn answer(path: &Path) {
    let mut v = client(path).call("answer", json!({})).unwrap();
    assert!(v["t"].is_string());
    wire::wipe(&mut v);
}

/// The server answers `budget` in place of the answer of the host.
fn spent(path: &Path) {
    let got = client(path).call_with("spent", json!({}), &mut |_, _| Ok(json!(true)));
    assert!(matches!(&got, Err(Error::Remote(f)) if f.code == Code::Budget), "{got:?}");
}

/// The server answers `busy`; the answer of the host comes later.
fn late(path: &Path) {
    let got = client(path).call("late", json!({}));
    assert!(matches!(&got, Err(Error::Remote(f)) if f.code == Code::Busy), "{got:?}");
}

/// The server answers `busy`; the host calls back later.
fn late_back(path: &Path) {
    let got = client(path).call("late-back", json!({}));
    assert!(matches!(&got, Err(Error::Remote(f)) if f.code == Code::Busy), "{got:?}");
}

/// The client holds the call back with blank lines past the wait of
/// [`Context::call`], then answers it with a token.
fn slow_back(path: &Path) {
    let mut stream = UnixStream::connect(path).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    stream.write_all(b"{\"schema\":1,\"id\":1,\"method\":\"slow-back\",\"params\":{}}\n").unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("\"for\":1"), "{line}");
    // The wait of the host: the read and the write timeout and one second.
    let end = Instant::now() + Duration::from_millis(1_800);
    while Instant::now() < end {
        stream.write_all(b"\n").unwrap();
        std::thread::sleep(Duration::from_millis(50));
    }
    stream
        .write_all(concat!(r#"{"schema":1,"id":1,"ok":true,"result":""#, marker!(), "-slow-back\"}\n").as_bytes())
        .unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("\"ok\":true"), "{line}");
}
