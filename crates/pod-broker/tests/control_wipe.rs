//! No block of memory that the control of the broker frees holds the master
//! token, the token of a job or the value of a secret (T-1023): `auth`, a
//! `load` and its `drop`, and a `load` whose secrets fail to read. The
//! allocator of this binary searches each freed block for the marker
//! ([`pod_testkit::freed`]), so the binary has this one test.

#![cfg(unix)]

mod common;
mod tls_common;

use pod_control::{Client, ClientSettings, Error, Tier, Token};
use pod_testkit::freed::{Scanning, scan};
use serde_json::{Value, json};
use tls_common::*;

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
            "policy": policy,
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
        // The thread of the host answers in order: this answer comes after
        // the drop of each request before it.
        assert_eq!(c.call("list", json!({})).unwrap()["jobs"], json!([]));
        drop(c);
        drop(server);
    });
    assert_eq!(found.map(|f| f.blocks), Some(0), "freed blocks with a token or a secret: {found:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
