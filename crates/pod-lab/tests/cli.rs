//! The binary `podlab`: its help and its manual come from the model of
//! pod-manual, and `podlab man --no-pager` prints the manual.
#![cfg(target_os = "linux")]

use std::process::{Command, Output};

fn podlab(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_podlab")).args(args).env_remove("PAGER").output().expect("podlab runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).expect("UTF-8")
}

#[test]
fn man_with_no_pager_prints_the_manual() {
    let o = podlab(&["man", "--no-pager"]);
    assert_eq!(o.status.code(), Some(0));
    let text = stdout(&o);
    for want in
        ["\nSYNOPSIS\n", "\npodlab check\n", "\nENVIRONMENT\n", "\nPROFILES\n", "\nEXIT STATUS\n", "125 (refused)"]
    {
        assert!(text.contains(want), "no {want:?} in\n{text}");
    }
    assert!(o.stderr.is_empty());
}

#[test]
fn one_section_the_json_and_the_roff_page() {
    let o = podlab(&["man", "--no-pager", "exit-status"]);
    assert!(stdout(&o).starts_with("EXIT STATUS\n"), "{}", stdout(&o));
    let o = podlab(&["man", "--json"]);
    let json = stdout(&o);
    let m = pod_manual::json::read(&json).expect("the JSON reads back");
    assert_eq!(m.name, "podlab");
    assert!(m.facts.exits.iter().any(|e| e.code == pod_manual::Code::Value(125)));
    let o = podlab(&["man", "--roff", "check"]);
    assert!(stdout(&o).starts_with(".TH PODLAB\\-CHECK 1 "), "{}", stdout(&o));
}

#[test]
fn an_unknown_section_is_a_usage_error() {
    let o = podlab(&["man", "nonsense"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.starts_with("nonsense: no such section; fix: "), "{err}");
}

/// `--help`, `COMMAND --help` and `help COMMAND` print the help of the
/// model; `--help --json` prints the command as JSON.
#[test]
fn the_help_of_each_command_comes_from_the_model() {
    let root = podlab(&["--help"]);
    assert_eq!(root.status.code(), Some(0));
    assert!(stdout(&root).contains("Usage: podlab COMMAND [OPTIONS]"), "{}", stdout(&root));
    for args in [&["run", "--help"][..], &["help", "run"][..], &["run", "-h"][..]] {
        let o = podlab(args);
        assert_eq!(o.status.code(), Some(0), "{args:?}");
        assert!(stdout(&o).contains("Usage: podlab run [OPTIONS] PROFILE CMD..."), "{args:?}: {}", stdout(&o));
    }
    let o = podlab(&["check", "--help", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).expect("JSON");
    assert_eq!(v["command"]["path"], serde_json::json!(["check"]));
    // After `--`, `--help` is a word of the command.
    let o = podlab(&["run", "no-such-profile", "--", "true", "--help"]);
    assert_eq!(o.status.code(), Some(2), "{}", String::from_utf8_lossy(&o.stderr));
}

/// The targets of the rows that send a packet out: a target with no
/// `--network`, or a `--network` that is not `NAME:PORT`, is a usage error;
/// a check with no `--network` runs no such row.
#[test]
fn the_targets_of_the_rows_that_send_a_packet_out() {
    let o = podlab(&["check", "proxy-only", "--network-other", "other.example"]);
    assert_eq!(o.status.code(), Some(2), "{}", String::from_utf8_lossy(&o.stderr));
    for bad in ["example.org", "example.org:http", "a b:443", ":443"] {
        let o = podlab(&["check", "proxy-only", "--network", bad]);
        assert_eq!(o.status.code(), Some(2), "{bad}: {}", String::from_utf8_lossy(&o.stderr));
    }
    let o = podlab(&["check", "proxy-only"]);
    let text = stdout(&o);
    assert!(text.contains("Settings::network is not set"), "{text}");
}

/// A served port of the stand-ins, `--serve` in a lab with no network,
/// `--serve-ca` with no `--serve` and a file of roots that does not read
/// are usage errors: the lab does not start.
#[test]
fn a_served_port_and_its_roots_are_checked_before_the_lab() {
    let missing = std::env::temp_dir().join(format!("pod-lab-cli-no-roots-{}.pem", std::process::id()));
    let missing = missing.to_str().expect("a UTF-8 path");
    for args in [
        &["run", "--serve", "443", "proxy-only", "--", "true"][..],
        &["run", "--serve", "3128", "proxy-only", "--", "true"][..],
        &["run", "--serve", "0", "proxy-only", "--", "true"][..],
        &["run", "--serve", "9443", "small-tmp", "--", "true"][..],
        &["run", "--serve-ca", missing, "proxy-only", "--", "true"][..],
        &["run", "--serve", "9443", "--serve-ca", missing, "proxy-only", "--", "true"][..],
    ] {
        let o = podlab(args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
    }
}

/// A row of a failure: `--expect-exit` and `--expect-stderr` give 0 where
/// the command ends as they say, and 4 with one line for each difference.
/// The error output of the command still comes to the caller.
#[test]
fn a_row_of_a_failure_wants_its_code_and_its_line() {
    let run = |want: &[&str]| {
        let mut args = vec!["run"];
        args.extend_from_slice(want);
        args.extend_from_slice(&["inherited-fds", "--", "sh", "-c", "echo the relay could not reach it >&2; exit 69"]);
        podlab(&args)
    };
    let o = run(&[]);
    if o.status.code() == Some(125) {
        return pod_testkit::host::skip_line("the lab inherited-fds does not hold here");
    }
    assert_eq!(o.status.code(), Some(69), "with no wanted end, the code of the command");
    let err = |o: &Output| String::from_utf8_lossy(&o.stderr).into_owned();
    let o = run(&["--expect-exit", "69", "--expect-stderr", "could not reach"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(err(&o).contains("the relay could not reach it"), "the error output comes to the caller: {}", err(&o));
    let o = run(&["--expect-exit", "0"]);
    assert_eq!(o.status.code(), Some(4), "{}", err(&o));
    assert!(err(&o).contains("the command exited 69; --expect-exit wants 0"), "{}", err(&o));
    let o = run(&["--expect-stderr", "connected"]);
    assert_eq!(o.status.code(), Some(4), "{}", err(&o));
    assert!(err(&o).contains("does not hold \"connected\""), "{}", err(&o));
    let o = podlab(&["run", "--expect-exit", "256", "inherited-fds", "--", "true"]);
    assert_eq!(o.status.code(), Some(2), "a code past 255 is a usage error: {}", err(&o));
}
