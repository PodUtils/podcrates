//! The class of a failure with no answer (call C153): the way out gives it
//! where the failure is made (a connect that was refused, a TLS handshake
//! that ended before the first byte of the server), and the runner holds
//! the first one of a scheduled run and fails the second.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pod_services_watch::Finding;

use crate::common::{NAME, endpoint, file, one, raw, text};

const SSH_BANNER: &str = "[[watch]]\nkind = \"ssh-banner\"\nendpoint = \"ssh\"\n";
const TLS_NAME: &str = "[[watch]]\nkind = \"tls-name\"\nendpoint = \"api\"\n";

/// A port of the loopback where nothing listens: the connect is refused.
fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    listener.local_addr().expect("its address").port()
}

#[tokio::test]
async fn a_refused_connect_is_no_answer() {
    pod_testkit::need_loopback!();
    let ep = endpoint("ssh", "ssh", "127.0.0.1", closed_port(), "");
    let report = one(&file("silent-1", &(ep + SSH_BANNER))).await;
    assert!(matches!(report.outcome.findings.as_slice(), [Finding::NoAnswer(_)]), "{}", text(&report));
    assert!(text(&report).starts_with("FAILED silent-1 ssh-banner: "), "{}", text(&report));
}

/// The server takes the connection and ends it before a byte: the
/// handshake has no answer.
#[tokio::test]
async fn a_handshake_that_ends_before_a_byte_is_no_answer() {
    pod_testkit::need_loopback!();
    let stand = raw(|_| {});
    let ep = endpoint("api", "https", NAME, stand.port(), "");
    let report = one(&file("silent-2", &(ep + TLS_NAME))).await;
    assert!(matches!(report.outcome.findings.as_slice(), [Finding::NoAnswer(_)]), "{}", text(&report));
}

/// The server sends the head of a TLS record and closes: the network ended
/// the handshake after a byte of the server, so the host answered.
#[tokio::test]
async fn a_handshake_cut_after_a_byte_of_the_server_is_an_answer() {
    pod_testkit::need_loopback!();
    let stand = raw(|io| {
        let _ = io.write_all(&[0x16, 0x03, 0x03, 0x00, 0x40]);
        let _ = io.flush();
    });
    let ep = endpoint("api", "https", NAME, stand.port(), "");
    let report = one(&file("silent-3", &(ep + TLS_NAME))).await;
    assert!(matches!(report.outcome.findings.as_slice(), [Finding::Failed(_)]), "{}", text(&report));
}

/// The server answers with bytes of another protocol: a failure of another
/// kind.
#[tokio::test]
async fn a_handshake_with_another_protocol_is_an_answer() {
    pod_testkit::need_loopback!();
    let stand = raw(|io| {
        let _ = io.write_all(b"SSH-2.0-not-tls\r\n");
        let _ = io.flush();
    });
    let ep = endpoint("api", "https", NAME, stand.port(), "");
    let report = one(&file("silent-4", &(ep + TLS_NAME))).await;
    assert!(matches!(report.outcome.findings.as_slice(), [Finding::Failed(_)]), "{}", text(&report));
}

/// A directory of the test, empty.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-services-watch-answer-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// The runner with `args`, with no variable of the environment: no proxy,
/// and no token.
fn runner(args: &[&Path]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_pod-services-watch"))
        .args(args)
        .env_clear()
        .output()
        .expect("the runner starts");
    eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    output
}

/// A watch of a closed port of the loopback: with no state, the run passes
/// with the line `first no answer` and lists the watch; with the state that
/// names it, the run fails; a run by hand (no state) fails.
#[test]
fn a_second_scheduled_run_with_no_answer_fails() {
    pod_testkit::need_loopback!();
    let dir = scratch("runner");
    let service = dir.join("a-silent-node.toml");
    let body = endpoint("ssh", "ssh", "127.0.0.1", closed_port(), "") + SSH_BANNER;
    std::fs::write(&service, file("a-silent-node", &body)).expect("the service file");
    let (was, now) = (dir.join("was.txt"), dir.join("now.txt"));
    let id = Path::new("a-silent-node");
    let scheduled = || {
        let flag = Path::new;
        runner(&[
            flag("--file"),
            &service,
            flag("--service"),
            id,
            flag("--no-answer-was"),
            &was,
            flag("--no-answer-now"),
            &now,
        ])
    };

    let first = scheduled();
    let said = String::from_utf8_lossy(&first.stdout).to_string();
    assert!(first.status.success(), "{said}");
    assert!(said.contains("FAILED (first no answer) a-silent-node ssh-banner: "), "{said}");
    assert!(said.contains(", 1 first with no answer, 0 moved or failed"), "{said}");
    let listed = std::fs::read_to_string(&now).expect("the list of the run");
    assert!(listed.lines().any(|l| l == "a-silent-node ssh-banner 1"), "{listed}");

    std::fs::rename(&now, &was).expect("the state of the next run");
    let second = scheduled();
    let said = String::from_utf8_lossy(&second.stdout).to_string();
    assert_eq!(second.status.code(), Some(1), "{said}");
    assert!(said.contains("FAILED a-silent-node ssh-banner: "), "{said}");
    assert!(!said.contains("first no answer) a-silent-node"), "{said}");

    let by_hand = runner(&[Path::new("--file"), &service, Path::new("--service"), id]);
    assert_eq!(by_hand.status.code(), Some(1), "a run by hand fails on each finding");
    let _ = std::fs::remove_dir_all(&dir);
}
