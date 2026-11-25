//! The instances: create, start, list, get, stop, delete, the console log,
//! the PATCH of the config on the condition of an ETag, and the errors of
//! incus with their words.
#![cfg(unix)]

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use common::Stand;
use pod_core::Class;
use pod_incus::{ETag, ErrorKind, Kind, NewInstance, Source};
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

/// The state of an instance, the default profile with the pool of its root
/// disk, and the OOM kills of the metrics; a missing profile keeps the
/// words of incus.
#[tokio::test]
async fn the_state_the_profile_and_the_oom_kills_of_an_instance() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = stand.client();
    client.create_instance(&NewInstance::container("web", Source::Alias("podvm/alpine".into()))).await.unwrap();
    client.start("web").await.unwrap();
    let state = client.instance_state("web").await.expect("the state");
    assert_eq!((state.status.as_str(), state.processes), ("Running", 7));
    assert_eq!((state.cpu.usage, state.memory.usage_peak), (1_500_000_000, 20_971_520));
    assert_eq!(state.disk["root"].usage, 1_048_576);
    assert_eq!(stand.seen("GET", "/1.0/instances/web/state").len(), 1);
    let e = client.instance_state("nowhere").await.expect_err("no instance");
    assert!(e.to_string().contains("Instance not found"), "{e}");

    let profile = client.profile("default").await.expect("the profile");
    assert_eq!(profile.devices["root"].get("pool").map(String::as_str), Some("fast"));
    let e = client.profile("missing").await.expect_err("no profile");
    assert_eq!(e.class(), Class::NotFound, "{e}");
    assert!(e.to_string().contains("Profile not found"), "{e}");

    assert_eq!(client.oom_kills("web").await.unwrap(), Some(2));
    assert_eq!(client.oom_kills("other").await.unwrap(), None);
}

fn config(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// A PATCH of the config alone sends `{"config": ...}` with `If-Match`:
/// the keys sent change, each other key stays, and the ETag changes.
#[tokio::test]
async fn a_patch_of_the_config_changes_only_the_keys_sent() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = stand.client();
    client.create_instance(&NewInstance::container("web", Source::None)).await.unwrap();
    client.start("web").await.unwrap();
    let (before, etag) = client.instance_with_etag("web").await.expect("the instance and its ETag");
    let etag = etag.expect("incus gives the ETag of an instance");
    assert_eq!(before, client.instance("web").await.unwrap());
    assert!(etag.as_str().starts_with('"'), "the quotes stay: {etag}");

    let limits = config(&[("limits.memory", "512MiB"), ("limits.cpu", "2")]);
    client.patch_instance_config("web", &limits, Some(&etag)).await.expect("the PATCH");
    let patch = &stand.seen("PATCH", "/1.0/instances/web")[0];
    assert_eq!(patch.header("if-match"), Some(etag.as_str()));
    assert_eq!(patch.header("content-type"), Some("application/json"));
    assert_eq!(body(patch), json!({"config": {"limits.memory": "512MiB", "limits.cpu": "2"}}));

    let (after, changed) = client.instance_with_etag("web").await.unwrap();
    let mut want = before.config.clone();
    want.extend(limits);
    assert_eq!(after.config, want);
    assert_eq!(after.config.get("volatile.base_image").map(String::as_str), Some("ab"), "a key not sent stays");
    assert_eq!((after.status.as_str(), after.profiles.clone()), ("Running", before.profiles.clone()));
    assert_ne!(changed.as_ref(), Some(&etag), "a change of the config changes the ETag");

    // With no condition, no If-Match goes out.
    client.patch_instance_config("web", &config(&[("limits.cpu", "4")]), None).await.unwrap();
    assert_eq!(stand.seen("PATCH", "/1.0/instances/web")[1].header("if-match"), None);
    let after = client.instance("web").await.unwrap();
    assert_eq!((after.config["limits.cpu"].as_str(), after.config["limits.memory"].as_str()), ("4", "512MiB"));
}

/// A refusal of incus keeps its kind and its class.
#[tokio::test]
async fn a_refusal_of_a_patch_keeps_the_class_of_incus() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = stand.client();
    let limits = config(&[("limits.memory", "lots")]);
    let e = client.patch_instance_config("nope", &limits, None).await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::Incus { status: 404, .. }), "{e}");
    assert_eq!((e.class(), e.call()), (Class::NotFound, "PATCH /1.0/instances/nope"));

    client.create_instance(&NewInstance::container("web", Source::None)).await.unwrap();
    let words = "Invalid config: Invalid value for limits.memory";
    stand.state().refuse = Some(("PATCH".into(), "/1.0/instances/web".into(), 400, words.into()));
    let e = client.patch_instance_config("web", &limits, None).await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::Incus { status: 400, .. }), "{e}");
    assert_eq!((e.class(), e.incus_error()), (Class::Usage, Some(words)));
    stand.state().refuse = Some(("PATCH".into(), "/1.0/instances/web".into(), 403, "not authorized".into()));
    let e = client.patch_instance_config("web", &limits, None).await.unwrap_err();
    assert_eq!((e.status(), e.class()), (Some(403), Class::Refused));
}

/// A PATCH on the ETag of a read before a change is refused with 412, as
/// its own kind, and changes nothing.
#[tokio::test]
async fn a_patch_on_a_stale_etag_is_an_etag_mismatch() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = stand.client();
    client.create_instance(&NewInstance::container("web", Source::None)).await.unwrap();
    let (_, read) = client.instance_with_etag("web").await.unwrap();
    let read = read.expect("an ETag");
    client.patch_instance_config("web", &config(&[("limits.cpu", "2")]), Some(&read)).await.unwrap();

    let e = client.patch_instance_config("web", &config(&[("limits.cpu", "8")]), Some(&read)).await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::EtagMismatch { .. }), "{e:?}");
    assert_eq!((e.status(), e.class()), (Some(412), Class::TempFail));
    assert!(e.incus_error().is_some_and(|words| words.starts_with("ETag doesn't match")), "{e}");
    assert!(e.to_string().starts_with("PATCH /1.0/instances/web: the instance changed after the read"), "{e}");
    assert_eq!(client.instance("web").await.unwrap().config["limits.cpu"], "2", "a refused PATCH changes nothing");

    let e = client.patch_instance_config("web", &config(&[]), Some(&ETag::new("\"made-up\""))).await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::EtagMismatch { .. }), "{e:?}");
}
