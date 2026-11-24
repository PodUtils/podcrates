//! exec with `record-output`: stdout and stderr apart from their logs, the
//! exit code from `metadata.return`, the logs deleted after.
#![cfg(unix)]

mod common;

use common::{Plan, Stand};
use pod_core::Class;
use pod_incus::{Client, Exec, NewInstance, Source};
use serde_json::Value;

async fn running(stand: &Stand, name: &str) -> Client {
    let client = stand.client();
    let mut new = NewInstance::container(name, Source::None);
    new.start = true;
    client.create_instance(&new).await.expect("a running instance");
    client
}

#[tokio::test]
async fn a_command_with_recorded_output_gives_stdout_stderr_and_the_exit_code() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = running(&stand, "web").await;
    stand.state().plan = Plan { exit: 3, stdout: b"out\n\xff".to_vec(), stderr: b"err\n".to_vec(), ..Plan::default() };
    let mut exec = Exec::new(["sh", "-c", "echo out; echo err >&2; exit 3"]);
    exec.environment.insert("CI".into(), "true".into());
    exec.cwd = "/work".into();
    let output = client.exec_recorded("web", &exec).await.expect("the output");
    assert_eq!(
        (output.exit_code, output.stdout.as_slice(), output.stderr.as_slice()),
        (3, &b"out\n\xff"[..], &b"err\n"[..])
    );

    let post: Value = serde_json::from_slice(&stand.seen("POST", "/1.0/instances/web/exec")[0].body).unwrap();
    assert_eq!((post["record-output"].as_bool(), post["wait-for-websocket"].as_bool()), (Some(true), Some(false)));
    assert_eq!((post["environment"]["CI"].as_str(), post["cwd"].as_str()), (Some("true"), Some("/work")));
    let gets = stand.seen("GET", "/1.0/instances/web/logs/exec-output/");
    let deletes = stand.seen("DELETE", "/1.0/instances/web/logs/exec-output/");
    assert_eq!((gets.len(), deletes.len()), (2, 2));
    assert!(gets[0].path.ends_with(".stdout") && gets[1].path.ends_with(".stderr"), "{gets:?}");
    assert!(stand.state().logs.is_empty(), "each log is deleted");
}

#[tokio::test]
async fn exec_in_a_stopped_instance_is_refused_with_the_words_of_incus() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = stand.client();
    client.create_instance(&NewInstance::container("web", Source::None)).await.unwrap();
    let e = client.exec_recorded("web", &Exec::new(["true"])).await.unwrap_err();
    assert_eq!((e.status(), e.incus_error(), e.class()), (Some(400), Some("Instance is not running"), Class::Usage));
    assert_eq!(e.call(), "POST /1.0/instances/web/exec");
}

#[tokio::test]
async fn a_command_with_no_program_is_refused_before_the_call() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let e = stand.client().exec_recorded("web", &Exec::default()).await.unwrap_err();
    assert_eq!(e.class(), Class::Usage);
    assert!(stand.state().requests.is_empty());
}

#[tokio::test]
async fn a_command_that_fails_to_start_gives_the_err_of_its_operation() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = running(&stand, "web").await;
    stand.state().fail = Some("Command not found".into());
    let e = client.exec_recorded("web", &Exec::new(["nope"])).await.unwrap_err();
    assert_eq!(e.incus_error(), Some("Command not found"));
}

/// incusd 6.0 ends an exec of 127 or 126 as a failure, and its wait answers
/// an error envelope (PodUtils/podvm#653): the exit code and the logs come
/// from the operation.
async fn recorded_end(exit: i64) {
    let stand = Stand::start();
    let client = running(&stand, "web").await;
    stand.state().plan = Plan { exit, stdout: b"out\n".to_vec(), stderr: b"err\n".to_vec(), ..Plan::default() };
    let exec = Exec::new(["sh", "-c", &format!("echo out; echo err >&2; exit {exit}")]);
    let output = client.exec_recorded("web", &exec).await.expect("the output");
    assert_eq!(
        (output.exit_code, output.stdout.as_slice(), output.stderr.as_slice()),
        (exit as i32, &b"out\n"[..], &b"err\n"[..])
    );
    assert_eq!(stand.seen("DELETE", "/1.0/instances/web/logs/exec-output/").len(), 2);
    assert!(stand.state().logs.is_empty(), "each log is deleted");
}

#[tokio::test]
async fn a_command_that_ends_with_127_gives_its_code_and_its_logs() {
    pod_testkit::need_loopback!();
    recorded_end(127).await;
}

#[tokio::test]
async fn a_command_that_ends_with_126_gives_its_code_and_its_logs() {
    pod_testkit::need_loopback!();
    recorded_end(126).await;
}

#[tokio::test]
async fn an_operation_that_is_gone_gives_the_code_of_the_words() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = running(&stand, "web").await;
    stand.state().plan = Plan { exit: 127, stdout: b"out\n".to_vec(), ..Plan::default() };
    stand.state().forget = true;
    let output = client.exec_recorded("web", &Exec::new(["nope"])).await.expect("the output");
    assert_eq!((output.exit_code, output.stdout.as_slice(), output.stderr.as_slice()), (127, &b""[..], &b""[..]));
}

#[tokio::test]
async fn a_failed_read_of_the_operation_stays_the_error_of_that_read() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = running(&stand, "web").await;
    stand.state().plan = Plan { exit: 126, ..Plan::default() };
    stand.state().op_read = Some((500, "database is locked".into()));
    let e = client.exec_recorded("web", &Exec::new(["/etc/hosts"])).await.unwrap_err();
    assert_eq!((e.status(), e.incus_error()), (Some(500), Some("database is locked")));
    assert!(e.call().starts_with("GET /1.0/operations/"), "{e}");
}

#[tokio::test]
async fn another_error_of_the_wait_stays_an_error_that_names_the_operation() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = running(&stand, "web").await;
    stand.state().plan = Plan { exit: 127, ..Plan::default() };
    stand.state().refuse = Some(("GET".into(), "/1.0/operations/".into(), 400, "Instance is not running".into()));
    let e = client.exec_recorded("web", &Exec::new(["nope"])).await.unwrap_err();
    assert_eq!((e.status(), e.incus_error()), (Some(400), Some("Instance is not running")));
    assert_eq!(e.call(), "POST /1.0/instances/web/exec");
    let id = stand.state().ops.keys().last().cloned().expect("the operation of exec");
    assert_eq!(e.operation(), Some(id.as_str()));
    assert!(stand.seen("GET", &format!("/1.0/operations/{id}")).iter().all(|s| s.path.ends_with("/wait")));
}
