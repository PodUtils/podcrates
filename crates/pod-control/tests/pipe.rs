//! The server on a Windows named pipe, with a client: the methods, the
//! conformance check and the deadline of a line (the jobs of Windows in
//! nightly.yml run it).

#![cfg(windows)]

mod common;

use std::io::Write;
use std::time::{Duration, Instant};

use pod_control::{Client, ClientSettings, conformance, pipe};

fn name(test: &str) -> String {
    format!(r"\\.\pipe\pod-control-{test}-{}", std::process::id())
}

fn start(test: &str, settings: pod_control::Settings) -> (pipe::Server, String) {
    let name = name(test);
    let (server, incoming) = pipe::serve(&name, settings, common::catalogue()).unwrap();
    let mut host = common::Recorder { log: common::Log::default() };
    std::thread::spawn(move || incoming.serve(&mut host));
    (server, name)
}

#[test]
fn a_pipe_answers_each_method_and_conforms() {
    let (_server, name) = start("each", common::settings());
    let mut c = Client::connect_pipe(&name, ClientSettings::default()).unwrap();
    assert_eq!(c.version().unwrap().program, "pod-control-test");
    assert_eq!(c.status().unwrap()["slots"][0]["principal"], "shared");
    let mut connect = || Client::connect_pipe(&name, ClientSettings::default());
    let found = conformance::check(&mut connect, &conformance::Options::default());
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn a_second_server_of_one_name_is_refused() {
    let (_server, name) = start("twice", common::settings());
    let again = pipe::serve(&name, common::settings(), common::catalogue());
    assert_eq!(again.err().map(|e| e.kind()), Some(std::io::ErrorKind::AddrInUse));
}

#[test]
fn a_slow_client_of_a_pipe_is_cut_at_the_read_timeout() {
    let mut settings = common::settings();
    settings.read_timeout = Duration::from_millis(300);
    let (_server, name) = start("slow", settings);
    let mut s = std::fs::OpenOptions::new().read(true).write(true).open(&name).unwrap();
    let started = Instant::now();
    let mut closed = false;
    while started.elapsed() < Duration::from_secs(5) {
        if s.write_all(b" ").is_err() {
            closed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(closed, "the connection was not cut");
}
