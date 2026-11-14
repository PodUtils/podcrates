//! The bounds and the broken answers: an agent that gives no answer, that
//! closes, that writes bytes that are not an answer, or too many of them.
#![cfg(unix)]

mod standin;

use std::time::{Duration, Instant};

use pod_core::Class;
use pod_qga::{Client, ErrorKind, Exec};
use serde_json::Value;
use standin::{Agent, Fault, Reply};

/// A fault for the first `n` requests named `execute`.
fn first(execute: &'static str, n: usize, reply: fn() -> Reply) -> Fault {
    let mut left = n;
    Box::new(move |req| {
        if req["execute"] == execute && left > 0 {
            left -= 1;
            return Some(reply());
        }
        None
    })
}

fn syncs(agent: &Agent) -> usize {
    agent.guest.lock().unwrap().calls().iter().filter(|c| *c == "guest-sync-delimited").count()
}

/// A call with no answer ends at its bound; the next call syncs first,
/// so a late answer is not read as its own.
#[test]
fn a_call_with_no_answer_ends_at_its_bound_and_the_next_call_syncs_again() {
    let agent = Agent::start("silent", b"", first("guest-ping", 1, || Reply::Silent));
    let mut s = agent.settings();
    s.call_timeout = Duration::from_millis(300);
    let mut c = Client::connect(s).expect("connect");
    let t = Instant::now();
    let e = c.ping().expect_err("no answer");
    let took = t.elapsed();
    assert!(matches!(e.kind(), ErrorKind::Timeout { .. }), "{e}");
    assert_eq!(e.class(), Class::Timeout);
    assert!(took >= Duration::from_millis(300) && took < Duration::from_secs(5), "{took:?}");
    c.ping().expect("the next ping");
    assert_eq!(syncs(&agent), 2, "the client did not sync again after the bound");
}

/// An answer that comes after its bound is old: the sync drops it, and
/// the next call gets its own answer.
#[test]
fn a_late_answer_is_not_the_answer_of_the_next_call() {
    // The first exec gets no answer. Its answer comes late, just before
    // the answer of the next request, whichever it is.
    let mut step = 0;
    let fault = move |req: &Value| {
        if req["execute"] == "guest-exec" && step == 0 {
            step = 1;
            return Some(Reply::Silent);
        }
        if step != 1 {
            return None;
        }
        step = 2;
        let mut b = b"{\"return\": {\"pid\": 1}}\n".to_vec();
        if req["execute"] == "guest-sync-delimited" {
            b.push(0xFF);
            b.extend_from_slice(format!("{{\"return\": {}}}\n", req["arguments"]["id"]).as_bytes());
        } else {
            b.extend_from_slice(b"{\"return\": {\"pid\": 9999}}\n");
        }
        Some(Reply::Raw(b))
    };
    let agent = Agent::start("late", b"", Box::new(fault));
    let mut s = agent.settings();
    s.call_timeout = Duration::from_millis(300);
    let mut c = Client::connect(s).expect("connect");
    assert!(matches!(c.exec(&Exec::new("/bin/sh")).expect_err("late").kind(), ErrorKind::Timeout { .. }));
    let pid = c.exec(&Exec::new("/bin/sh")).expect("the next exec");
    assert_ne!(pid, 1, "the next exec read the late answer of the first");
    assert_eq!(syncs(&agent), 2);
}

#[test]
fn a_closed_connection_is_closed() {
    let agent = Agent::start("closed", b"", first("guest-ping", 1, || Reply::Close));
    let mut c = Client::connect(agent.settings()).expect("connect");
    let e = c.ping().expect_err("closed");
    assert!(matches!(e.kind(), ErrorKind::Closed), "{e}");
    assert_eq!(e.class(), Class::Unavailable);
}

/// Bytes that are not an answer: a protocol error, and the next call
/// syncs again.
#[test]
fn a_broken_answer_is_a_protocol_error_and_the_next_call_syncs() {
    let agent = Agent::start("broken", b"", first("guest-ping", 1, || Reply::Raw(b"{\"retur\n".to_vec())));
    let mut c = Client::connect(agent.settings()).expect("connect");
    let e = c.ping().expect_err("broken");
    assert!(matches!(e.kind(), ErrorKind::Protocol(_)), "{e}");
    c.ping().expect("the next ping");
    assert_eq!(syncs(&agent), 2);
}

/// A return of the wrong shape: a protocol error that names the field.
#[test]
fn a_return_of_the_wrong_shape_names_the_field() {
    let agent = Agent::start("shape", b"", first("guest-exec", 1, || Reply::Return(serde_json::json!({"pdi": 1}))));
    let mut c = Client::connect(agent.settings()).expect("connect");
    let e = c.exec(&Exec::new("/bin/sh")).expect_err("no pid");
    assert!(e.to_string().contains("no integer \"pid\""), "{e}");
}

/// An answer over the limit is refused, and the client does not keep it.
#[test]
fn an_answer_over_the_limit_is_refused() {
    let big = || Reply::Raw(format!("{{\"return\": \"{}\"}}\n", "x".repeat(10_000)).into_bytes());
    let agent = Agent::start("big", b"", first("guest-ping", 1, big));
    let mut s = agent.settings();
    s.max_answer = 1000;
    let mut c = Client::connect(s).expect("connect");
    let e = c.ping().expect_err("too long");
    assert!(e.to_string().contains("an answer over 1000 bytes"), "{e}");
    assert_eq!(e.class(), Class::Protocol);
    c.ping().expect("the next ping");
}

/// A sync that never gets its own id ends at its bound.
#[test]
fn a_sync_that_never_gets_its_id_ends_at_its_bound() {
    let fault =
        |req: &Value| (req["execute"] == "guest-sync-delimited").then(|| Reply::Raw(b"\xff{\"return\": 0}\n".to_vec()));
    let agent = Agent::start("noid", b"", Box::new(fault));
    let mut s = agent.settings();
    s.call_timeout = Duration::from_millis(200);
    let e = Client::connect(s).err().expect("no sync");
    assert_eq!(e.call(), "guest-sync-delimited");
    assert!(matches!(e.kind(), ErrorKind::Timeout { .. }), "{e}");
}

/// A command that does not end within the limit of the wait.
#[test]
fn a_wait_ends_at_its_limit_and_the_command_still_runs() {
    let agent = Agent::plain("wait");
    agent.guest.lock().unwrap().runs_for = usize::MAX;
    let mut c = Client::connect(agent.settings()).expect("connect");
    let pid = c.exec(&Exec::new("sleep")).expect("exec");
    let e = c.wait_exec(pid, Duration::from_millis(10), Duration::from_millis(100)).expect_err("still runs");
    assert!(matches!(e.kind(), ErrorKind::NotEnded { pid: p, .. } if *p == pid), "{e}");
    assert_eq!(e.class(), Class::Timeout);
}
