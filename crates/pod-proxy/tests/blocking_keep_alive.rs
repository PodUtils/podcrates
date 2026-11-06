//! The idle connections of the blocking client (feature `blocking`): its
//! requests to one host share one connection, as those of the async face.

#[allow(dead_code, reason = "the async tests use the rest of the server")]
mod kept;

use std::io::Read;
use std::sync::Arc;

use kept::{Act, NAME, WAIT, mark, ok, pins, plain};
use pod_proxy::blocking::{Client, Dialer};
use pod_proxy::client::Request;
use pod_proxy::{ProxyChoice, Settings};

#[test]
fn the_blocking_client_keeps_its_connection() {
    pod_testkit::need_loopback!();
    fn marked(conn: usize, nth: usize, _head: &str) -> Act {
        Act::Answer(ok(&mark(conn, nth)))
    }
    let kept = plain(marked);
    let dialer = Dialer::new(Settings::new("podvm", ProxyChoice::Direct), Arc::new(pins())).expect("a runtime");
    let client = Client::new(kept.context.clone(), &dialer);
    let get = || Request::new("GET", format!("http://{NAME}:{}/", kept.port)).timeout(WAIT);
    let first = client.send(get()).expect("an answer");
    let mut streamed = client.send_streamed(get()).expect("an answer");
    let mut second = String::new();
    streamed.read_to_string(&mut second).expect("the body");
    drop(streamed);
    let third = client.send(get()).expect("an answer");
    assert_eq!((first.text(), second, third.text()), ("c1r1".into(), "c1r2".into(), "c1r3".into()));
    assert_eq!(kept.accepted(), 1);
    let off = client.clone().with_keep_alive(false);
    assert_eq!(off.send(get()).expect("an answer").text(), "c2r1");
    assert_eq!(kept.accepted(), 2);
}
