//! No block of memory that the server or the client frees holds a token of
//! a line (T-1023): the master token of `auth`, a token in the params of a
//! delegated request, and a token in each line that the server refuses.
//! The allocator of this binary searches each freed block for the marker
//! ([`pod_testkit::freed`]), so the binary has this one test.

#![cfg(unix)]

mod common;

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use pod_control::{Client, ClientSettings, Error, Tier, Token, unix};
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
    // Lines that the server refuses, each with a token.
    for (line, code) in [
        (concat!(r#"{"schema":1,"id":2,"method":"ask","params":{"t":""#, marker!(), r#""},"bogus":1}"#), "bad-request"),
        (concat!(r#"{"schema":1,"id":3,"method":"ask","params":[""#, marker!(), r#""]}"#), "bad-request"),
        (concat!(r#"{"schema":1,"id":4,"ok":true,"result":""#, marker!(), r#""}"#), "bad-request"),
        (concat!(r#"[""#, marker!(), r#""]"#), "bad-request"),
        (concat!(r#"{"schema":2,"t":""#, marker!(), r#""}"#), "bad-request"),
        (concat!(r#"{"schema":1,"id":5,"method":"version","params":{"t":""#, marker!(), r#""}}"#), "bad-params"),
        (concat!(r#"{"schema":1,"id":6,"method":"status","params":{"t":""#, marker!(), r#""}}"#), "bad-params"),
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
