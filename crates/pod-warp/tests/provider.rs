//! The provider of an enrolment (PodUtils/podnet#486, QUIC-4 and QUIC-5):
//! the adapter of Cloudflare against the stand-in API and the in-process
//! edge, the local/static adapter against the same edge, both on the same
//! session test, and the IPv6-only rule of the front door, which makes no
//! IPv4 attempt and names the reason before any socket.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::h3door::{self, FrontDoor};
use pod_core::{Class, Target};
use pod_testkit::stand;
use pod_warp::door::NO_FAMILY;
use pod_warp::provider::Enrolment as _;
use pod_warp::{Cloudflare, Device, Door, Routes, StaticEnrolment, Warp, Way};

/// A way over HTTP/3 on the UDP of the system.
fn h3_way(dial: Arc<dyn pod_core::Dial>) -> Way {
    let mut way = Way::new(dial, common::provider());
    way.h3 = true;
    way.udp = Some(Arc::new(pod_warp::SystemUdp));
    way.timeout = pod_testkit::host::scaled(Duration::from_secs(10));
    way
}

/// A packet crosses a session of `warp`: a CONNECT-IP packet comes back
/// from the edge.
async fn the_session_test(warp: &Warp) {
    let session = warp.open_session().await.unwrap();
    assert!(session.is_h3());
    let (mut tx, mut rx) = session.split();
    let packet = vec![0x45u8; 200];
    tx.send_packets(std::slice::from_ref(&packet)).await.unwrap();
    let back = tokio::time::timeout(Duration::from_secs(10), rx.recv_packet())
        .await
        .expect("a packet back")
        .unwrap()
        .expect("a packet");
    assert_eq!(back.to_vec(), packet);
    tx.close().unwrap();
}

/// The adapter of Cloudflare registers with the stand-in API, pins the key
/// of the record and opens the session at the edge of that record.
async fn cloudflare(front: &FrontDoor) -> Warp {
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    api.serve_key(&pod_warp::der::pem(&front.pin, "PUBLIC KEY"));
    let served = stand::http(api.handler()).unwrap();
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), common::Moved::new(vec![]));
    settings.way = h3_way(common::Moved::new(vec![]));
    settings.door = Door::Pinned(Target::Addr(front.addr));
    Warp::register(settings).await.unwrap()
}

/// The local adapter of the same edge: its door and its pinned key, and no
/// call of the WARP API at all.
async fn local(front: &FrontDoor) -> Warp {
    let (mut settings, _) = common::settings("http://127.0.0.1:1", common::Moved::new(vec![]));
    settings.way = h3_way(common::Moved::new(vec![]));
    let provider = StaticEnrolment::at("local-edge", front.addr, front.pin.clone()).unwrap();
    Warp::enrol(settings, &provider).await.unwrap()
}

/// A second provider passes the same session test as the first: the client
/// is bound to no provider.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn both_providers_pass_the_same_session_test() {
    pod_testkit::need_loopback!();
    let front = h3door::ip_echo_door().await;
    let cloudflare = cloudflare(&front).await;
    assert_eq!(cloudflare.provider(), Some("cloudflare"));
    the_session_test(&cloudflare).await;
    let local = local(&front).await;
    assert_eq!(local.provider(), Some("local-edge"));
    assert!(local.device().id.is_empty(), "a local provider keeps no record");
    assert_eq!(local.account().await.unwrap_err().class, Class::Usage);
    the_session_test(&local).await;
    let protocols = front.gate.protocols.lock().unwrap().clone();
    assert_eq!(protocols, ["cf-connect-ip", "cf-connect-ip"], "{protocols:?}");
    assert_eq!(front.gate.sessions.load(Ordering::SeqCst), 2);
}

/// PodUtils/podnet#366: an IPv6-only host sends no packet to an IPv4 door.
/// A pinned IPv4 door is a named failure before any socket, the report
/// names it, and the dial is never used.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ipv6_only_host_makes_no_ipv4_attempt() {
    let v4 = "162.159.198.2:443".parse::<SocketAddr>().unwrap();
    let v6 = "[2606:4700:103::2]:443".parse::<SocketAddr>().unwrap();
    let record = device(&[v4.to_string()], &[v6.to_string()]);
    let moved = common::Moved::new(vec![]);
    let (mut settings, kept) = common::settings("http://127.0.0.1:1", moved.clone() as Arc<dyn pod_core::Dial>);
    settings.routes = Routes { v4: false, v6: true };
    settings.door = Door::Pinned(Target::Addr(v4));
    let warp = Warp::from_parts(settings, record.clone(), pod_warp::Identity::generate().unwrap());
    let err = warp.open_session().await.err().expect("the pinned IPv4 door fails");
    assert_eq!(err.class, Class::Usage);
    assert!(err.chain().contains(NO_FAMILY), "{}", err.chain());
    assert!(err.chain().contains(&v4.to_string()), "{}", err.chain());
    assert_eq!(moved.dials.load(Ordering::SeqCst), 0, "no dial of any kind");
    let events = kept.events.lock().unwrap().clone();
    assert!(events.iter().any(|e| e.kind == "warp.door" && e.text.contains(NO_FAMILY)), "{events:?}");
    // A record of an IPv4 door alone is the same failure, with no attempt.
    let (mut settings, kept) = common::settings("http://127.0.0.1:1", moved.clone() as Arc<dyn pod_core::Dial>);
    settings.routes = Routes { v4: false, v6: true };
    let ipv4_only = device(&[v4.to_string()], &[]);
    let warp = Warp::from_parts(settings, ipv4_only, pod_warp::Identity::generate().unwrap());
    let err = warp.open_session().await.err().expect("an IPv4 record fails");
    assert!(err.chain().contains(NO_FAMILY), "{}", err.chain());
    assert_eq!(moved.dials.load(Ordering::SeqCst), 0);
    assert!(kept.events.lock().unwrap().iter().any(|e| e.kind == "warp.door"), "{:?}", kept.kinds());
    // The IPv6 door of the record opens, and it is the only attempt.
    let (mut settings, _) = common::settings("http://127.0.0.1:1", moved.clone() as Arc<dyn pod_core::Dial>);
    settings.routes = Routes { v4: false, v6: true };
    let warp = Warp::from_parts(settings, record, pod_warp::Identity::generate().unwrap());
    let err = warp.open_session().await.err().expect("no edge on the loopback");
    assert!(!err.chain().contains(NO_FAMILY), "the IPv6 door was tried: {}", err.chain());
    assert_eq!(moved.dials.load(Ordering::SeqCst), 1, "the IPv6 door alone");
}

/// A device record with the given door addresses, and a peer key.
fn device(v4: &[String], v6: &[String]) -> Device {
    let peer = |v4: &[String], v6: &[String]| {
        let v4 = v4.first().cloned().unwrap_or_default();
        let v6 = v6.first().cloned().unwrap_or_default();
        let key = pod_warp::Identity::generate().unwrap();
        let pem = pod_warp::der::pem(key.spki_der(), "PUBLIC KEY");
        serde_json::json!({"public_key": pem, "endpoint": {"v4": v4, "v6": v6}})
    };
    let json = serde_json::json!({
        "id": "record-of-the-test",
        "token": "token-of-the-test",
        "account": {"id": "account-of-the-test"},
        "config": {"peers": [peer(v4, v6)]}
    });
    serde_json::from_value(json).unwrap()
}

/// The enrolment failure of a provider is not a transport failure: a
/// refused registration names the provider's status, and the failure of the
/// session type keeps that name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_enrolment_is_named_and_not_a_transport_failure() {
    pod_testkit::need_loopback!();
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    api.fail_post(Some(403));
    let served = stand::http(api.handler()).unwrap();
    let (settings, _) = common::settings(&format!("http://{}", served.addr()), common::Moved::new(vec![]));
    let provider = Cloudflare::new(settings.api.clone());
    let identity = pod_warp::Identity::generate().unwrap();
    let err = provider.enrol(&identity, &settings.hints).await.expect_err("a refused enrolment");
    assert_eq!(err.class(), Class::Refused);
    assert!(err.text().contains("HTTP 403"), "{}", err.text());
    let failure = err.failure();
    assert_eq!(failure.class, Class::Refused);
    assert!(failure.chain().contains("HTTP 403"), "{}", failure.chain());
    for word in ["QUIC", "TCP", "UDP"] {
        assert!(!failure.chain().contains(word), "a transport stage in {}", failure.chain());
    }
}
