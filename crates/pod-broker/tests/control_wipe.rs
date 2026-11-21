//! No block of memory that the control of the broker frees holds the master
//! token, the token of a job or the value of a secret (T-1023): `auth`, a
//! `load` and its `drop`, and a `load` whose secrets fail to read. A token
//! and a secret with a JSON escape (`\"`, `\\`, `\uXXXX`, `\n`) are read
//! into a buffer of their own (T-1048). The
//! allocator of this binary searches each freed block for the marker
//! ([`pod_testkit::freed`]), so the binary has this one test.

#![cfg(unix)]

mod common;
mod tls_common;

use pod_control::{Client, ClientSettings, Error, Tier, Token};
use pod_testkit::freed::{Scanning, scan};
use serde_json::{Value, json};
use tls_common::*;
use zeroize::Zeroizing;

#[global_allocator]
static ALLOC: Scanning = Scanning;

/// The start of each made-up token and secret of the test.
macro_rules! marker {
    () => {
        "made-up-wipe-marker-b7d2"
    };
}

const MARKER: &str = marker!();

#[test]
fn no_freed_block_of_the_control_holds_a_token_or_a_secret() {
    let ((), found) = scan(MARKER.as_bytes(), || drop(std::hint::black_box(MARKER.to_string())));
    let found = found.expect("the scanning allocator is the allocator of this binary");
    assert!(found.blocks >= 1, "the search found no unwiped copy: {found:?}");

    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let dir = pod_testkit::host::socket_dir("pod-broker-wipe");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("control.sock");
    let policy: Value = serde_json::from_str(&policy("", "")).unwrap();
    let ((), found) = scan(MARKER.as_bytes(), || {
        let (broker, _) = tls_broker(TlsFar::new(echo()));
        let mut settings = pod_broker::control::settings("pod-broker-test", "1.0.0");
        settings.master = Some(Token::new(concat!(marker!(), "-master")));
        let server = {
            let _entered = rt.enter();
            pod_broker::control::serve_control(broker, &path, settings).unwrap()
        };
        let mut c = Client::connect_unix(&path, ClientSettings::default()).unwrap();
        assert_eq!(c.auth(&Token::new(concat!(marker!(), "-master"))).unwrap(), Tier::Master);
        let load = json!({
            "job": "job1",
            "token": concat!(marker!(), "-job-token"),
            "ttl_s": 60,
            "policy": policy.clone(),
            "secrets": { "github": concat!(marker!(), "-secret") },
        });
        let answer = c.call("load", load).unwrap();
        assert!(answer["ca_pem"].is_string(), "{answer}");
        assert_eq!(c.call("drop", json!({ "job": "job1" })).unwrap()["dropped"], true);
        // The secrets fail to read after the first value.
        let load = json!({
            "job": "job2",
            "token": concat!(marker!(), "-job-token"),
            "ttl_s": 60,
            "policy": { "schema": 1, "mode": "open" },
            "secrets": { "a": concat!(marker!(), "-secret"), "b": 5 },
        });
        match c.call("load", load) {
            Err(Error::Remote(f)) => assert_eq!(f.code, pod_control::Code::BadParams),
            other => panic!("{other:?}"),
        }
        // A token and a secret with escapes: the client writes `\"` and
        // `\\`, the line of `raw` has `\u0041`, and a token with `\n` is
        // not printable.
        let load = json!({
            "job": "job3",
            "token": concat!(marker!(), "-job\"to\\ken"),
            "ttl_s": 60,
            "policy": policy.clone(),
            "secrets": { "github": concat!(marker!(), "-se\\cr\"et") },
        });
        assert!(c.call("load", load).unwrap()["ca_pem"].is_string());
        assert_eq!(c.call("drop", json!({ "job": "job3" })).unwrap()["dropped"], true);
        let parts = [
            r#"{"schema":1,"id":90,"method":"load","params":{"job":"job4","token":""#,
            concat!(marker!(), r#"\u0041-job-token","ttl_s":60,"policy":"#),
            &policy.to_string(),
            concat!(r#","secrets":{"github":""#, marker!(), r#"\u0041-secret"}}}"#),
        ];
        let mut line = Zeroizing::new(String::with_capacity(parts.iter().map(|p| p.len()).sum()));
        parts.iter().for_each(|p| line.push_str(p));
        let answer = c.raw(line.as_bytes()).unwrap().unwrap();
        drop(line);
        assert_eq!(answer["ok"], true, "{answer}");
        assert_eq!(c.call("drop", json!({ "job": "job4" })).unwrap()["dropped"], true);
        let load = json!({
            "job": "job5",
            "token": concat!(marker!(), "-job\ntoken"),
            "ttl_s": 60,
            "policy": { "schema": 1, "mode": "open" },
        });
        match c.call("load", load) {
            Err(Error::Remote(f)) => assert_eq!(f.code, pod_control::Code::BadParams),
            other => panic!("{other:?}"),
        }
        // The thread of the host answers in order: this answer comes after
        // the drop of each request before it.
        assert_eq!(c.call("list", json!({})).unwrap()["jobs"], json!([]));
        drop(c);
        drop(server);
    });
    assert_eq!(found.map(|f| f.blocks), Some(0), "freed blocks with a token or a secret: {found:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
