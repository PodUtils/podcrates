//! A proxy at a command that ends before it answers (PodUtils/podnet#754):
//! the failure names the command, its exit and the last line of its stderr;
//! the stderr never reaches the stderr of the process; and a command that
//! the shell does not find is not started again for each dial.
#![cfg(unix)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use pod_core::{Class, Failure, Stage};
use pod_proxy::resolve::System;
use pod_proxy::{ChildIo, DialError, Dialer, Proxy, ProxyChoice, STDERR_KEPT, Settings};
use tokio::io::AsyncReadExt;

const LIMIT: Duration = Duration::from_secs(10);

/// The child run of [`the_stderr_of_a_command_does_not_reach_the_process`].
const CHILD: &str = "POD_PROXY_EXEC_EXIT_CHILD";

async fn dial(spec: &str) -> DialError {
    let proxy = Proxy::parse_trusted(spec).expect("a proxy at a command");
    let dialer = Dialer::new(Settings::new("test", ProxyChoice::Via(proxy)), Arc::new(System::new()));
    let limit = pod_testkit::host::scaled(LIMIT);
    dialer.stream("relay.invalid", 443, limit).await.expect_err("a command that exits gives no tunnel")
}

/// The ask of the issue: one failure with the cause, at the stage and in the
/// class of a proxy that is not there.
#[tokio::test]
async fn a_command_that_is_not_found_gives_its_cause() {
    let error = dial("http+exec:/nonexist/prog").await;
    let DialError::ProxyExited { word, status, said } = &error else { panic!("{error:?}") };
    assert_eq!((word.as_str(), status.as_str()), ("/nonexist/prog", "127 (not found)"));
    // dash says "not found", bash "No such file or directory".
    assert!(said.contains("/nonexist/prog"), "{said}");
    let text = error.to_string();
    assert!(text.starts_with("the proxy command /nonexist/prog exited 127 (not found) before it answered: "), "{text}");
    let failure = Failure::from(error.clone());
    assert_eq!((failure.stage, failure.class), (Stage::Proxy, Class::Unavailable));
    // The next dial gets the same failure.
    assert_eq!(dial("http+exec:/nonexist/prog").await, error);
}

/// A SOCKS5 proxy at a command gives the same cause.
#[tokio::test]
async fn a_socks5_command_that_is_not_found_gives_its_cause() {
    let error = dial("socks5h+exec:/nonexist/socks").await;
    assert!(
        error.to_string().starts_with("the proxy command /nonexist/socks exited 127 (not found) before it answered"),
        "{error}"
    );
}

/// A command that exits with another code: no hold, and the last line of a
/// long stderr.
#[tokio::test]
async fn a_command_that_exits_gives_the_last_line_of_its_stderr() {
    let spec = "http+exec:i=0; while [ $i -lt 300 ]; do echo line $i >&2; i=$((i+1)); done; exit 3";
    let error = dial(spec).await;
    assert_eq!(
        error,
        DialError::ProxyExited { word: "while".into(), status: "3".into(), said: "line 299".into() },
        "{error}"
    );
}

/// 20 dials through a command that the shell does not find start it one
/// time (`EXEC_HOLD`).
#[tokio::test]
async fn a_command_that_is_not_found_starts_once_for_many_dials() {
    let dir = std::env::temp_dir().join(format!("pod-proxy-exec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    let count = dir.join("starts");
    let spec = format!("http+exec:printf x >> '{}'; /nonexist/counted", count.display());
    let started = Instant::now();
    let first = dial(&spec).await;
    for _ in 0..19 {
        assert_eq!(dial(&spec).await, first);
    }
    let starts = std::fs::read(&count).map(|bytes| bytes.len()).unwrap_or(0);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(starts, 1, "{first}");
    assert!(started.elapsed() < pod_proxy::EXEC_HOLD, "the test ran past the hold");
}

/// The test binary runs itself again with only [`exec_exit_child`], and
/// reads the stderr of that run: it holds no line of a command.
#[test]
fn the_stderr_of_a_command_does_not_reach_the_process() {
    let run = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args(["exec_exit_child", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .expect("a run of the test binary");
    let (stdout, stderr) = (String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    assert!(run.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("exec_exit_child ... ok"), "{stdout}");
    assert!(!stderr.contains("nonexist") && !stderr.contains("noise"), "the stderr of the process: {stderr}");
}

/// The dials of the child run: they pass in any run, and write their
/// commands' stderr only when it reaches the process.
#[tokio::test]
async fn exec_exit_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    for _ in 0..3 {
        assert!(matches!(dial("http+exec:/nonexist/child").await, DialError::ProxyExited { .. }));
        assert!(matches!(dial("http+exec:echo noise >&2; exit 4").await, DialError::ProxyExited { .. }));
    }
}

/// A [`ChildIo`] keeps the last [`STDERR_KEPT`] bytes of the stderr of its
/// command, apart from its stdout.
#[tokio::test]
async fn a_child_keeps_the_last_bytes_of_its_stderr() {
    let mut child =
        ChildIo::spawn("i=0; while [ $i -lt 100 ]; do echo stderr line $i >&2; i=$((i+1)); done; printf out")
            .expect("the command");
    let mut out = String::new();
    child.read_to_string(&mut out).await.expect("the stdout");
    assert_eq!(out, "out");
    assert!(child.answered());
    let until = Instant::now() + pod_testkit::host::scaled(LIMIT);
    while !child.stderr_tail().ends_with(b"stderr line 99\n") && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let tail = child.stderr_tail();
    assert_eq!(tail.len(), STDERR_KEPT, "{}", String::from_utf8_lossy(&tail));
    assert!(tail.ends_with(b"stderr line 99\n"));
}
