//! The background operation: the wait to its end, the wait at its limit,
//! a failure with the words of incus, the state and the cancel.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::Stand;
use pod_core::Class;
use pod_incus::{Client, ErrorKind, NewInstance, Source, status};

#[tokio::test]
async fn an_async_call_waits_for_its_operation() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    stand.state().hold = true;
    let client = stand.client();
    let new = NewInstance::container("web", Source::Alias("podvm/alpine".into()));
    let create = tokio::spawn(async move { client.create_instance(&new).await });
    stand.arrival("GET", "/1.0/operations/").await;
    assert!(!create.is_finished(), "the call waits while the operation runs");
    let wait = stand.seen("GET", "/1.0/operations/").remove(0);
    assert!(wait.path.ends_with("/wait"), "{}", wait.path);
    assert_eq!(wait.query.get("timeout").map(String::as_str), Some("-1"));
    let id = wait.path.trim_start_matches("/1.0/operations/").trim_end_matches("/wait").to_string();
    stand.end_op(&id, serde_json::json!({}));
    let done = create.await.unwrap().expect("the operation ended");
    assert_eq!((done.id.as_str(), done.status_code, done.class.as_str()), (id.as_str(), status::SUCCESS, "task"));
    assert!(done.is_final() && done.succeeded());
}

#[tokio::test]
async fn a_wait_ends_at_its_limit_with_the_operation_running() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    stand.state().hold = true;
    let mut settings = stand.settings();
    settings.wait = Some(Duration::ZERO);
    let new = NewInstance::container("web", Source::None);
    let e = Client::new(settings).create_instance(&new).await.unwrap_err();
    let ErrorKind::Timeout { id, status } = e.kind() else { panic!("{e:?}") };
    assert_eq!(status, "Running (103)");
    assert_eq!(e.class(), Class::Timeout);
    assert_eq!(e.call(), "POST /1.0/instances");
    assert_eq!(stand.seen("GET", &format!("/1.0/operations/{id}/wait"))[0].query["timeout"], "0");
    // A part of a second goes up to the next second.
    let mut settings = stand.settings();
    settings.wait = Some(Duration::from_millis(1));
    let operation = stand.client().operation(id).await.unwrap();
    assert_eq!(operation.status_code, status::RUNNING);
    let e = Client::new(settings).wait(id, Some(Duration::from_millis(1))).await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::Timeout { .. }), "{e:?}");
    let waits = stand.seen("GET", &format!("/1.0/operations/{id}/wait"));
    assert_eq!(waits.last().unwrap().query["timeout"], "1");
}

#[tokio::test]
async fn a_failed_operation_gives_the_words_of_incus() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let words = "Failed creating instance record: Instance \"web\" already exists";
    stand.state().fail = Some(words.into());
    let e = stand.client().create_instance(&NewInstance::container("web", Source::None)).await.unwrap_err();
    let ErrorKind::Operation { code, err, .. } = e.kind() else { panic!("{e:?}") };
    assert_eq!((*code, err.as_str(), e.incus_error()), (status::FAILURE, words, Some(words)));
    assert_eq!(e.class(), Class::Unavailable);
    assert!(e.to_string().starts_with("POST /1.0/instances: the operation "), "{e}");
    assert!(e.to_string().ends_with(&format!("failed (400): {words}")), "{e}");
}

#[tokio::test]
async fn an_operation_is_read_and_canceled() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    stand.state().hold = true;
    let id = stand.state().new_op("task", serde_json::json!({"progress": "50%"}));
    let client = stand.client();
    let operation = client.operation(&id).await.unwrap();
    assert_eq!((operation.text("progress"), operation.is_final()), (Some("50%"), false));
    client.cancel(&id).await.unwrap();
    assert_eq!(stand.seen("DELETE", "/1.0/operations/").len(), 1);
    let canceled = client.wait(&id, None).await.expect("a canceled operation is final");
    assert_eq!(canceled.status_code, status::CANCELED);
    let e = client.wait_success(&id, None).await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::Operation { code: 401, .. }), "{e:?}");
    assert_eq!(e.class(), Class::TempFail);
    assert!(e.to_string().contains("was canceled (401): Operation canceled"), "{e}");
    let e = client.operation("no-such-operation").await.unwrap_err();
    assert_eq!((e.status(), e.incus_error()), (Some(404), Some("Operation not found")));
}
