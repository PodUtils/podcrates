//! The instances: create, start, list, get, stop, delete, the console log,
//! and the errors of incus with their words.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::Stand;
use pod_core::Class;
use pod_incus::{Kind, NewInstance, Source};
use serde_json::{Value, json};

fn body(seen: &common::Seen) -> Value {
    serde_json::from_slice(&seen.body).expect("a JSON body")
}

#[tokio::test]
async fn an_instance_is_created_started_listed_stopped_and_deleted() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = stand.client();
    let mut new = NewInstance::container("web", Source::Alias("podvm/alpine".into()));
    new.config.insert("raw.lxc".into(), "lxc.init.cmd = /podvm/init".into());
    new.devices.insert("root".into(), [("type".to_string(), "disk".to_string()), ("path".into(), "/".into())].into());
    client.create_instance(&new).await.expect("created");
    let post = &stand.seen("POST", "/1.0/instances")[0];
    assert_eq!(post.header("content-type"), Some("application/json"));
    let sent = body(post);
    assert_eq!(sent["source"], json!({"type": "image", "alias": "podvm/alpine"}));
    assert_eq!(sent["config"]["raw.lxc"], "lxc.init.cmd = /podvm/init");
    assert_eq!(sent["devices"]["root"], json!({"type": "disk", "path": "/"}));
    assert_eq!((sent["type"].as_str(), sent["start"].as_bool()), (Some("container"), Some(false)));

    client.start("web").await.expect("started");
    let put = &stand.seen("PUT", "/1.0/instances/web/state")[0];
    assert_eq!(body(put), json!({"action": "start", "timeout": -1, "force": false, "stateful": false}));

    let listed = client.instances().await.expect("the list");
    assert_eq!(listed.len(), 1);
    let web = &listed[0];
    assert_eq!(
        (web.name.as_str(), web.status.as_str(), web.status_code, web.kind.as_str()),
        ("web", "Running", 103, "container")
    );
    assert_eq!(web.config.get("volatile.base_image").map(String::as_str), Some("ab"));
    assert!(web.devices.is_empty(), "null is an empty map");
    assert_eq!(client.instance("web").await.unwrap(), *web);

    client.stop("web", false, Some(Duration::from_secs(30))).await.expect("stopped");
    let put = &stand.seen("PUT", "/1.0/instances/web/state")[1];
    assert_eq!(body(put), json!({"action": "stop", "timeout": 30, "force": false, "stateful": false}));
    client.stop("web", true, None).await.expect("stopped at once");
    assert_eq!(body(&stand.seen("PUT", "/1.0/instances/web/state")[2])["force"], true);

    client.delete_instance("web").await.expect("deleted");
    assert_eq!(stand.seen("DELETE", "/1.0/instances/web").len(), 1);
    assert!(client.instances().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_virtual_machine_from_a_fingerprint_that_starts_at_once() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let mut new = NewInstance::container("vm1", Source::Fingerprint("ab12".into()));
    new.kind = Kind::VirtualMachine;
    new.start = true;
    new.profiles = Some(Vec::new());
    stand.client().create_instance(&new).await.unwrap();
    let sent = body(&stand.seen("POST", "/1.0/instances")[0]);
    assert_eq!(sent["source"], json!({"type": "image", "fingerprint": "ab12"}));
    assert_eq!((sent["type"].as_str(), sent["start"].as_bool()), (Some("virtual-machine"), Some(true)));
    assert_eq!(sent["profiles"], json!([]));
    assert_eq!(stand.client().instance("vm1").await.unwrap().status, "Running");
}

#[tokio::test]
async fn a_running_instance_is_not_deleted_and_the_words_of_incus_come_back() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = stand.client();
    let mut new = NewInstance::container("web", Source::None);
    new.start = true;
    client.create_instance(&new).await.unwrap();
    let e = client.delete_instance("web").await.unwrap_err();
    assert_eq!((e.status(), e.incus_error(), e.class()), (Some(400), Some("Instance is running"), Class::Usage));
    assert_eq!(e.call(), "DELETE /1.0/instances/web");

    let e = client.instance("nope").await.unwrap_err();
    assert_eq!((e.status(), e.class()), (Some(404), Class::NotFound));
    let e = client.start("nope").await.unwrap_err();
    assert_eq!(
        e.to_string(),
        "PUT /1.0/instances/nope/state: incus refused the call (404, error_code 404): Instance not found"
    );
}

#[tokio::test]
async fn a_name_goes_escaped_into_the_path() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let e = stand.client().instance("a/../b c").await.unwrap_err();
    assert_eq!(e.call(), "GET /1.0/instances/a%2F..%2Fb%20c");
    assert_eq!(stand.state().requests[0].path, "/1.0/instances/a%2F..%2Fb%20c");
}

#[tokio::test]
async fn the_console_log_comes_as_bytes() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    stand.state().console = b"boot\r\n\x1b[0m\xffend".to_vec();
    let log = stand.client().console_log("web").await.expect("the console log");
    assert_eq!(log, b"boot\r\n\x1b[0m\xffend");
    stand.state().refuse =
        Some(("GET".into(), "/1.0/instances/web/console".into(), 400, "Instance is not running".into()));
    let e = stand.client().console_log("web").await.unwrap_err();
    assert_eq!(e.incus_error(), Some("Instance is not running"));
}
