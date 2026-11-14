//! Each call of the client against the stand-in agent.
#![cfg(unix)]

mod standin;

use std::time::Duration;

use pod_core::Class;
use pod_qga::{Client, ErrorKind, Exec, OpenMode, Shutdown};
use serde_json::json;
use standin::{Agent, Reply};

/// The socket holds old answers of an earlier client: an answer, and the
/// delimited answer of an older sync. The connect skips them, and the
/// parse error of the 0xFF too, and the first call gets its own answer.
#[test]
fn the_connect_skips_old_answers_and_the_first_call_gets_its_own() {
    let old = b"{\"return\": {\"pid\": 1}}\n\xff{\"return\": 1}\n{\"return\": 2}\n";
    let agent = Agent::start("old", old, Box::new(|_| None));
    let mut c = Client::connect(agent.settings()).expect("connect");
    c.ping().expect("ping");
    let pid = c.exec(&Exec::new("/bin/true")).expect("exec");
    assert!(pid > 4000, "the pid of an old answer: {pid}");
    assert_eq!(agent.guest.lock().unwrap().calls(), ["guest-sync-delimited", "guest-ping", "guest-exec"]);
}

/// exec gives a pid; the status runs, then gives the code, stdout and
/// stderr; the wait asks until the end.
#[test]
fn exec_gives_a_pid_and_the_status_gives_the_code_and_the_output() {
    let agent = Agent::plain("exec");
    agent.guest.lock().unwrap().runs_for = 2;
    let mut c = Client::connect(agent.settings()).expect("connect");
    let mut e = Exec::new("cmd.exe");
    e.args = vec!["/c".into(), "echo".into()];
    e.env = vec!["A=1".into()];
    e.input = Some(b" in".to_vec());
    let pid = c.exec(&e).expect("exec");
    let first = c.exec_status(pid).expect("status");
    assert!(!first.exited && first.exit_code.is_none(), "{first:?}");
    let done = c.wait_exec(pid, Duration::from_millis(1), Duration::from_secs(10)).expect("wait");
    assert_eq!(done.exit_code, Some(7));
    assert_eq!(done.stdout, b"/c echo in");
    assert_eq!(done.stderr, b"cmd.exe");
    let g = agent.guest.lock().unwrap();
    let req = &g.seen[1];
    assert_eq!(
        req["arguments"],
        json!({"path": "cmd.exe", "arg": ["/c", "echo"], "env": ["A=1"], "input-data": "IGlu", "capture-output": true})
    );
    assert_eq!(g.calls().iter().filter(|c| *c == "guest-exec-status").count(), 3);
}

/// A file goes in and comes back in chunks of the settings, and each
/// handle is closed.
#[test]
fn a_file_goes_in_and_comes_back_in_chunks() {
    let agent = Agent::plain("files");
    let mut s = agent.settings();
    s.chunk = 3;
    let mut c = Client::connect(s).expect("connect");
    let data = b"0123456789\n\x00\xff".to_vec();
    c.write_file("C:\\podvm\\in.bin", &data).expect("write");
    assert_eq!(agent.guest.lock().unwrap().files["C:\\podvm\\in.bin"], data);
    assert_eq!(c.read_file("C:\\podvm\\in.bin", 1 << 20).expect("read"), data);
    c.write_file("empty", b"").expect("an empty file");
    assert_eq!(c.read_file("empty", 0).expect("read"), b"");
    let g = agent.guest.lock().unwrap();
    assert!(g.open.is_empty(), "a handle stayed open: {:?}", g.open);
    let calls = g.calls();
    assert_eq!(calls.iter().filter(|c| *c == "guest-file-write").count(), 5, "{calls:?}");
    assert_eq!(g.seen[1]["arguments"]["mode"], "w");
}

/// A file over the limit is refused, and its handle is closed.
#[test]
fn a_file_over_the_limit_is_refused_and_its_handle_is_closed() {
    let agent = Agent::plain("limit");
    agent.guest.lock().unwrap().files.insert("big".into(), vec![7; 100]);
    let mut c = Client::connect(agent.settings()).expect("connect");
    let e = c.read_file("big", 99).expect_err("over the limit");
    assert!(matches!(e.kind(), ErrorKind::Limit { limit: 99, .. }), "{e}");
    assert_eq!(e.class(), Class::Policy);
    assert!(agent.guest.lock().unwrap().open.is_empty(), "the handle stayed open");
    assert_eq!(c.read_file("big", 100).expect("at the limit").len(), 100);
}

/// A refusal of the agent names its call and its class, and the client
/// stays in step: the next call needs no new sync.
#[test]
fn a_refusal_names_its_class_and_the_client_stays_in_step() {
    let agent = Agent::plain("refuse");
    let mut c = Client::connect(agent.settings()).expect("connect");
    let e = c.file_open("C:\\missing", OpenMode::Read).expect_err("a missing file");
    assert_eq!((e.call(), e.agent_class(), e.class()), ("guest-file-open", Some("GenericError"), Class::Unavailable));
    assert!(e.to_string().contains("failed to open file"), "{e}");
    let e = c.exec_status(1).expect_err("no such command");
    assert!(e.to_string().contains("Invalid parameter"), "{e}");
    c.ping().expect("ping after the refusals");
    let syncs = agent.guest.lock().unwrap().calls().iter().filter(|c| *c == "guest-sync-delimited").count();
    assert_eq!(syncs, 1, "a refusal made the client sync again");
}

/// An agent with no such command: the class says unsupported.
#[test]
fn a_command_that_the_agent_does_not_have_is_unsupported() {
    let fault = |req: &serde_json::Value| {
        (req["execute"] == "guest-exec")
            .then(|| Reply::Refuse("CommandNotFound", "The command guest-exec has not been found".into()))
    };
    let agent = Agent::start("nocmd", b"", Box::new(fault));
    let mut c = Client::connect(agent.settings()).expect("connect");
    let e = c.exec(&Exec::new("/bin/sh")).expect_err("no exec");
    assert_eq!(e.class(), Class::Unsupported);
}

/// The agent gives no answer when it obeys a shutdown, and closes; a
/// refusal is an error.
#[test]
fn a_shutdown_with_no_answer_is_done_and_a_refusal_is_an_error() {
    let agent = Agent::plain("down");
    let mut c = Client::connect(agent.settings()).expect("connect");
    c.shutdown(Shutdown::Powerdown).expect("shutdown");
    assert_eq!(agent.guest.lock().unwrap().shutdown.as_deref(), Some("powerdown"));

    let fault = |req: &serde_json::Value| {
        (req["execute"] == "guest-shutdown").then(|| Reply::Refuse("GenericError", "shutdown is disabled".into()))
    };
    let agent = Agent::start("nodown", b"", Box::new(fault));
    let mut c = Client::connect(agent.settings()).expect("connect");
    let e = c.shutdown(Shutdown::Reboot).expect_err("a refused shutdown");
    assert!(e.to_string().contains("shutdown is disabled"), "{e}");
    c.ping().expect("the client syncs again after a shutdown");
}

#[test]
fn no_agent_at_the_path_is_named() {
    let path = pod_testkit::host::socket_path(&format!("pod-qga-none-{}.sock", std::process::id()));
    let e = Client::connect(pod_qga::Settings::new(&path)).err().expect("no agent");
    assert!(matches!(e.kind(), ErrorKind::NoAgent { .. }), "{e}");
    assert_eq!((e.call(), e.class()), ("connect", Class::Unavailable));
    assert!(e.to_string().contains("no guest agent answers at"), "{e}");
}

/// A value that cannot go into a request is refused before it is sent.
#[test]
fn an_empty_path_and_a_read_of_no_byte_are_refused_before_a_request() {
    let agent = Agent::plain("usage");
    let mut c = Client::connect(agent.settings()).expect("connect");
    assert_eq!(c.exec(&Exec::new("")).expect_err("no path").class(), Class::Usage);
    assert_eq!(c.file_open("", OpenMode::Write).expect_err("no path").class(), Class::Usage);
    assert_eq!(c.file_read(1, 0).expect_err("no byte").class(), Class::Usage);
    assert_eq!(agent.guest.lock().unwrap().calls(), ["guest-sync-delimited"]);
}
