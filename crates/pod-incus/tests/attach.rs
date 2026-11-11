//! exec with its websockets on the Unix socket: stdin, stdout and stderr
//! apart, the barrier at the end of each stream, the control socket, the
//! exit code; and no secret in an error.
#![cfg(unix)]

mod common;

use common::{Plan, Stand};
use pod_incus::{Client, Exec, NewInstance, Source};

async fn running(stand: &Stand, name: &str) -> Client {
    let client = stand.client();
    let mut new = NewInstance::container(name, Source::None);
    new.start = true;
    client.create_instance(&new).await.expect("a running instance");
    client
}

#[tokio::test]
async fn a_command_with_its_websockets_gives_its_streams_apart() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = running(&stand, "web").await;
    stand.state().plan =
        Plan { exit: 0, stdout: b"hello ".to_vec(), stderr: b"warn".to_vec(), echo_stdin: true, ..Plan::default() };
    let attached = client.exec_attached("web", &Exec::new(["cat"])).await.expect("attached");
    assert_eq!(attached.operation.class, "websocket");
    let output = attached.output(b"from stdin", 1 << 20).await.expect("the output");
    assert_eq!(
        (output.exit_code, output.stdout.as_slice(), output.stderr.as_slice()),
        (0, &b"hello from stdin"[..], &b"warn"[..])
    );
    assert_eq!(stand.state().stdin_seen, b"from stdin");

    let post: serde_json::Value =
        serde_json::from_slice(&stand.seen("POST", "/1.0/instances/web/exec")[0].body).unwrap();
    assert_eq!((post["wait-for-websocket"].as_bool(), post["record-output"].as_bool()), (Some(true), Some(false)));
    assert_eq!(post["interactive"], false);
    let sockets = stand.seen("GET", "/1.0/operations/");
    let upgrades: Vec<_> = sockets.iter().filter(|s| s.path.ends_with("/websocket")).collect();
    assert_eq!(upgrades.len(), 4, "control, stdin, stdout and stderr");
    for upgrade in upgrades {
        assert_eq!(upgrade.header("upgrade"), Some("websocket"));
        assert!(upgrade.query.contains_key("secret"));
    }
}

#[tokio::test]
async fn the_streams_are_read_one_by_one() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = running(&stand, "web").await;
    stand.state().plan = Plan { exit: 5, stdout: b"0123456789".to_vec(), stderr: b"e".to_vec(), ..Plan::default() };
    let mut attached = client.exec_attached("web", &Exec::new(["sh"])).await.unwrap();
    attached.stdin.close().await.unwrap();
    let mut out = Vec::new();
    while let Some(bytes) = attached.stdout.read().await.unwrap() {
        out.push(bytes);
    }
    assert_eq!(out, [b"01234".to_vec(), b"56789".to_vec()], "two messages, two reads");
    assert_eq!(attached.stdout.read().await.unwrap(), None, "the end stays the end");
    assert_eq!(attached.stderr.read_to_end(16).await.unwrap(), b"e");
    assert_eq!(attached.wait().await.unwrap(), 5);
}

#[tokio::test]
async fn a_signal_goes_by_the_control_socket() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = running(&stand, "web").await;
    stand.state().plan = Plan { until_signal: true, ..Plan::default() };
    let mut attached = client.exec_attached("web", &Exec::new(["sleep", "infinity"])).await.unwrap();
    attached.control.signal(15).await.unwrap();
    let (out, err) = tokio::join!(attached.stdout.read_to_end(16), attached.stderr.read_to_end(16));
    assert_eq!((out.unwrap(), err.unwrap()), (Vec::new(), Vec::new()));
    assert_eq!(attached.wait().await.unwrap(), 143);
    assert_eq!(stand.state().signals, [15]);
}

#[tokio::test]
async fn an_output_over_its_bound_is_refused() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = running(&stand, "web").await;
    stand.state().plan = Plan { stdout: vec![b'x'; 64], ..Plan::default() };
    let attached = client.exec_attached("web", &Exec::new(["yes"])).await.unwrap();
    let e = attached.output(b"", 16).await.unwrap_err();
    assert_eq!(e.class(), pod_core::Class::Usage);
    assert!(e.to_string().ends_with("the output is longer than 16 bytes"), "{e}");
    assert!(!e.to_string().contains("secret"), "{e}");
}
