//! A WARP session as a network, against the stand-in API and the stand-in
//! edge on the loopback: TCP to an address and to a name through the
//! `Dial`, UDP through the `Datagrams`, the order of the doors and the
//! pinned door, a key of the front door that the record does not pin, and
//! a key that the edge did not enroll.

mod common;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::edge::{self, Edge, Route};
use pod_core::{Class, Dial, Stage, Target};
use pod_testkit::stand;
use pod_warp::key::DeviceKey;
use pod_warp::{Door, Routes, Warp};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const LIMIT: Duration = Duration::from_secs(10);
/// The port of the echo service inside the tunnel.
const ECHO_TCP: u16 = 80;

/// The stand-in API whose records name `door_v4` and `door_v6` with the key
/// of `edge`, and the edge with an echo route; the moved dial carries the
/// IPv4 door to `v4_to` and the IPv6 door to the edge.
async fn world(v4_to: SocketAddr) -> (stand::warp::Api, stand::Stand, Edge, Arc<common::Moved>) {
    let echo = edge::echo_server().await;
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    let edge = Edge::start(api.clone(), vec![Route { port: ECHO_TCP, outside: echo }]).await;
    api.serve_key(&edge.pem());
    let served = stand::http(api.handler()).unwrap();
    let moved = common::Moved::new(vec![(edge::door_v4(), v4_to), (edge::door_v6(), edge.addr)]);
    (api, served, edge, moved)
}

/// A port of the loopback where nothing listens.
fn dead() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

async fn echo_through(net: &pod_warp::WarpNet, target: &Target) -> SocketAddr {
    let deadline = tokio::time::Instant::now() + LIMIT;
    let mut connected = net.dial(target, deadline).await.unwrap();
    connected.io.write_all(b"through warp").await.unwrap();
    let mut got = [0u8; 12];
    tokio::time::timeout(LIMIT, connected.io.read_exact(&mut got)).await.expect("an echo").unwrap();
    assert_eq!(&got, b"through warp");
    connected.peer.unwrap()
}

/// A registered device opens a session at the edge and carries TCP to an
/// address and to a name; the name is resolved inside the tunnel. The
/// CONNECT is connect-ip's, with the authority of the facts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_carries_tcp_to_an_address_and_to_a_name() {
    pod_testkit::need_loopback!();
    let (_api, served, edge, moved) = world(dead()).await;
    let (settings, _kept) = common::settings(&format!("http://{}", served.addr()), moved.clone());
    let warp = Warp::register(settings).await.unwrap();
    let net = warp.connect().await.unwrap();
    assert!(net.names_inside());
    assert_eq!(net.label(), "WARP");
    let addr: SocketAddr = "203.0.113.9:80".parse().unwrap();
    assert_eq!(echo_through(&net, &Target::Addr(addr)).await, addr);
    let named = Target::Name { host: "echo.test".into(), port: ECHO_TCP };
    let peer = echo_through(&net, &named).await;
    assert_eq!(peer, SocketAddr::new(IpAddr::V4(edge::NAME_IP), ECHO_TCP));
    assert_eq!(edge.tunnels(), 1);
    assert_eq!(edge.seen.header("cf-connect-proto").as_deref(), Some("cf-connect-ip"));
    let authority = edge.seen.authority.lock().unwrap().clone().unwrap();
    assert!(pod_warp::service::connect_uri().ends_with(&authority), "{authority}");
    assert!(!net.is_closed(), "{:?}", net.close_reason());
}

/// UDP goes through the session as `Datagrams`: a datagram to the echo port
/// of an address comes back from that address.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_carries_udp_as_datagrams() {
    pod_testkit::need_loopback!();
    let (_api, served, _edge, moved) = world(dead()).await;
    let (settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    let net = Warp::register(settings).await.unwrap().connect().await.unwrap();
    let datagrams = net.datagrams().await.unwrap();
    let to: SocketAddr = format!("198.51.100.20:{}", edge::ECHO_PORT).parse().unwrap();
    pod_core::datagrams::send_to(&*datagrams, b"a datagram", to).await.unwrap();
    let mut buf = [0u8; 64];
    let (n, from) = tokio::time::timeout(LIMIT, pod_core::datagrams::recv_from(&*datagrams, &mut buf))
        .await
        .expect("an echo")
        .unwrap();
    assert_eq!((&buf[..n], from), (&b"a datagram"[..], to));
}

/// PodUtils/podnet issue 124: the doors of the record in order. The IPv4
/// door fails, the IPv6 door opens, the event names the next door, and the
/// door that opened goes first next time. With no IPv4 route, the IPv6 door
/// goes first (PodUtils/podnet@T-169): no failed IPv4 connect.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_doors_of_the_record_go_in_order() {
    pod_testkit::need_loopback!();
    let (_api, served, _edge, moved) = world(dead()).await;
    let base = format!("http://{}", served.addr());
    let (settings, kept) = common::settings(&base, moved.clone());
    let warp = Warp::register(settings).await.unwrap();
    warp.open_session().await.unwrap();
    assert_eq!(moved.dials.load(Ordering::SeqCst), 2, "the IPv4 door, then the IPv6 door");
    assert!(kept.kinds().contains(&"warp.door"), "{:?}", kept.kinds());
    assert_eq!(warp.won_door(), Some(edge::door_v6()));
    warp.open_session().await.unwrap();
    assert_eq!(moved.dials.load(Ordering::SeqCst), 3, "the door that opened goes first");
    let (mut settings, _) = common::settings(&base, moved.clone());
    settings.routes = Routes { v4: false, v6: true };
    let ipv6_only = Warp::from_parts(settings, warp.device().clone(), warp.identity().clone());
    ipv6_only.open_session().await.unwrap();
    assert_eq!(moved.dials.load(Ordering::SeqCst), 4, "no IPv4 connect");
}

/// PodUtils/podnet#413, ask 10: a pinned door is the only door; the order of
/// the record does not apply, and a pinned door that fails is the failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_door_is_the_only_door() {
    pod_testkit::need_loopback!();
    let (_api, served, edge, moved) = world(dead()).await;
    let base = format!("http://{}", served.addr());
    let (mut settings, _) = common::settings(&base, moved.clone());
    settings.door = Door::Pinned(Target::Addr(edge.addr));
    let warp = Warp::register(settings).await.unwrap();
    warp.open_session().await.unwrap();
    assert_eq!(moved.dials.load(Ordering::SeqCst), 1);
    assert_eq!(warp.endpoint_in_use(), Some(Target::Addr(edge.addr)));
    let (mut settings, _) = common::settings(&base, moved.clone());
    settings.door = Door::Pinned(Target::Addr(dead()));
    let pinned_dead = Warp::from_parts(settings, warp.device().clone(), warp.identity().clone());
    let err = pinned_dead.open_session().await.err().unwrap();
    assert_eq!(err.stage, Stage::Connect, "{}", err.chain());
    assert_eq!(moved.dials.load(Ordering::SeqCst), 2, "the record's doors are not tried");
}

/// The front door shows a key that the record does not pin: the client
/// reads the record again, and opens with the key that the API gives now. A
/// key that the API did not give is never taken.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_key_of_the_front_door_reads_the_record_again() {
    pod_testkit::need_loopback!();
    let (api, served, edge, moved) = world(dead()).await;
    let (mut settings, kept) = common::settings(&format!("http://{}", served.addr()), moved);
    settings.door = Door::Pinned(Target::Addr(edge.addr));
    let warp = Warp::register(settings).await.unwrap();
    let new = DeviceKey::generate().unwrap();
    edge.show(vec![new]);
    // The API still gives the old key: no session.
    let err = warp.open_session().await.err().unwrap();
    assert!(pod_warp::tls::is_pin_mismatch(&err), "{}", err.chain());
    assert_eq!(err.class, Class::Protocol);
    let reads = api.asked("GET");
    assert!(reads >= 1, "the record was read again");
    // The API gives the new key: the session opens.
    api.serve_key(&edge.pem());
    warp.open_session().await.unwrap();
    assert!(kept.kinds().contains(&"warp.pin"), "{:?}", kept.kinds());
}

/// A device whose key the edge did not enroll is refused at the TLS
/// handshake.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_that_the_edge_did_not_enroll_is_refused() {
    pod_testkit::need_loopback!();
    let (_api, served, edge, moved) = world(dead()).await;
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    settings.door = Door::Pinned(Target::Addr(edge.addr));
    let warp = Warp::register(settings.clone()).await.unwrap();
    let stranger = pod_warp::Identity::generate().unwrap();
    let other = Warp::from_parts(settings, warp.device().clone(), stranger);
    let err = other.open_session().await.err().unwrap();
    assert!(matches!(err.stage, Stage::Tls | Stage::Warp), "{:?}: {}", err.stage, err.chain());
    assert_eq!(edge.tunnels(), 0);
}
