//! `GET /1.0`, the refusal by name where no incus answers, the error of
//! incus with its words, and the project of each call.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::Stand;
use pod_core::Class;
use pod_incus::{AnswerPhase, Client, ErrorKind, Settings};

#[tokio::test]
async fn the_facts_of_the_server_are_read() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let server = stand.client().server().await.expect("the facts");
    assert_eq!((server.api_version.as_str(), server.auth.as_str()), ("1.0", "trusted"));
    assert_eq!(server.environment.server_version, "6.0.0");
    assert_eq!(server.environment.architectures, ["x86_64", "i686"]);
    assert!(server.environment.addresses.is_empty(), "null is an empty list");
    assert!(server.has("container_exec_recording") && !server.has("image_import_alias"));
    server.check().expect("incus 6.0 has each extension that the crate needs");
    let seen = stand.seen("GET", "/1.0");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].header("host"), Some("unix.socket"));
    assert_eq!(seen[0].header("user-agent"), Some("pod-incus-test"));
    assert!(seen[0].query.is_empty(), "{:?}", seen[0].query);
}

#[tokio::test]
async fn a_server_with_no_needed_extension_is_refused_by_name() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    stand.state().extensions = Some(vec!["instances".into(), "console".into()]);
    let server = stand.client().server().await.unwrap();
    let e = server.check().unwrap_err();
    assert_eq!(e.class(), Class::Unsupported);
    assert_eq!(e.to_string(), "GET /1.0: the incus server does not have the API extension container_exec_recording");
}

#[tokio::test]
async fn a_socket_with_no_server_is_refused_by_name() {
    let path = common::socket_path("absent");
    let e = Client::new(Settings::new(&path)).server().await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::NoServer { socket, .. } if *socket == path), "{e:?}");
    assert_eq!(e.class(), Class::Unavailable);
    assert_eq!(e.call(), "GET /1.0");
    assert!(e.to_string().starts_with(&format!("GET /1.0: no incus answers at {}: ", path.display())), "{e}");
    let failure = pod_core::Failure::from(e);
    assert_eq!((failure.stage, failure.class), (pod_core::Stage::Host, Class::Unavailable));
}

#[tokio::test]
async fn a_socket_with_no_listener_is_refused_by_name() {
    pod_testkit::need_loopback!();
    let path = common::socket_path("closed");
    drop(std::os::unix::net::UnixListener::bind(&path).expect("bind"));
    let e = Client::new(Settings::new(&path)).instances().await.unwrap_err();
    let ErrorKind::NoServer { cause, .. } = e.kind() else { panic!("{e:?}") };
    assert_eq!(cause.kind(), std::io::ErrorKind::ConnectionRefused);
    assert_eq!(e.call(), "GET /1.0/instances");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn an_error_of_incus_becomes_the_error_of_the_crate_with_its_words() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    stand.state().refuse = Some(("GET".into(), "/1.0".into(), 403, "not authorized".into()));
    let e = stand.client().server().await.unwrap_err();
    assert_eq!((e.status(), e.incus_error(), e.class()), (Some(403), Some("not authorized"), Class::Refused));
    assert_eq!(e.to_string(), "GET /1.0: incus refused the call (403, error_code 403): not authorized");
}

#[tokio::test]
async fn the_project_goes_with_each_call() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let mut settings = stand.settings();
    settings.project = Some("user-1000".into());
    let client = Client::new(settings);
    client.server().await.unwrap();
    client.instances().await.unwrap();
    for seen in stand.state().requests.iter() {
        assert_eq!(seen.query.get("project").map(String::as_str), Some("user-1000"), "{seen:?}");
    }
    assert_eq!(stand.seen("GET", "/1.0/instances")[0].query.get("recursion").map(String::as_str), Some("1"));
}

/// A socket that takes the connection and never answers holds the caller
/// only to `Settings::answer_timeout` (30 s by default), PodUtils/podvm#653.
#[tokio::test(start_paused = true)]
async fn a_server_that_never_answers_gives_answer_timeout_in_the_head() {
    pod_testkit::need_loopback!();
    let raw = common::raw::Raw::start(Vec::new(), common::raw::After::Hold);
    let started = tokio::time::Instant::now();
    let client = raw.client();
    let call = tokio::time::timeout(Duration::from_secs(600), client.server());
    let e = call.await.expect("the call ends at the bound").unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::AnswerTimeout { phase: AnswerPhase::Head, bound } if *bound == Duration::from_secs(30)),
        "{e:?}"
    );
    assert_eq!(started.elapsed().as_secs(), 30);
    assert_eq!(e.class(), Class::Timeout);
    assert_eq!(e.to_string(), "GET /1.0: incus sent no byte of the head of its answer in 30000 ms");
}

#[tokio::test(start_paused = true)]
async fn a_body_that_stalls_gives_answer_timeout_in_the_body() {
    pod_testkit::need_loopback!();
    let head = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{\"type\":".to_vec();
    let raw = common::raw::Raw::start(head, common::raw::After::Hold);
    let mut settings = raw.settings();
    settings.answer_timeout = Some(Duration::from_secs(2));
    let started = tokio::time::Instant::now();
    let call = tokio::time::timeout(Duration::from_secs(600), async { Client::new(settings).instances().await });
    let e = call.await.expect("the call ends at the bound").unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::AnswerTimeout { phase: AnswerPhase::Body, .. }), "{e:?}");
    assert_eq!(started.elapsed().as_secs(), 2);
    assert_eq!(e.call(), "GET /1.0/instances");
}

#[tokio::test(start_paused = true)]
async fn a_wait_past_the_answer_bound_still_waits() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    stand.state().hold = true;
    let id = stand.state().new_op("task", serde_json::json!({}));
    let mut settings = stand.settings();
    settings.answer_timeout = Some(Duration::from_secs(1));
    let started = tokio::time::Instant::now();
    let e = Client::new(settings).wait(&id, Some(Duration::from_secs(60))).await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::Timeout { .. }), "the limit of the wait, not the answer bound: {e:?}");
    assert_eq!(started.elapsed().as_secs(), 60);
}

#[tokio::test(start_paused = true)]
async fn no_answer_bound_waits() {
    pod_testkit::need_loopback!();
    let raw = common::raw::Raw::start(Vec::new(), common::raw::After::Hold);
    let mut settings = raw.settings();
    settings.answer_timeout = None;
    let call = tokio::time::timeout(Duration::from_secs(3600), async { Client::new(settings).server().await });
    assert!(call.await.is_err(), "the call still waits after an hour");
    assert_eq!(Settings::new("/x").answer_timeout, Some(Duration::from_secs(30)));
}
