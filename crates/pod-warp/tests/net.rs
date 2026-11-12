//! A WARP session as a network, against the stand-in API and the stand-in
//! edge on the loopback: TCP to an address and to a name through the
//! `Dial`, UDP through the `Datagrams`, the order of the doors and the
//! pinned door, a key of the front door that the record does not pin, a
//! key that the edge did not enroll, the end of the open streams and UDP
//! sockets when the session closes, and the refusal of a UDP datagram to a
//! family that the device lacks.

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

/// An open session with a stream to the echo route after one exchange, and
/// a UDP socket: a read of each waits in a task.
async fn open_flows(
    net: &pod_warp::WarpNet,
) -> (tokio::task::JoinHandle<std::io::Result<usize>>, tokio::task::JoinHandle<std::io::Result<usize>>) {
    let target = Target::Addr("203.0.113.9:80".parse().unwrap());
    let mut connected = net.dial(&target, tokio::time::Instant::now() + LIMIT).await.unwrap();
    connected.io.write_all(b"ping").await.unwrap();
    let mut got = [0u8; 4];
    tokio::time::timeout(LIMIT, connected.io.read_exact(&mut got)).await.expect("an echo").unwrap();
    let datagrams = net.datagrams().await.unwrap();
    let read = tokio::spawn(async move { connected.io.read(&mut [0u8; 16]).await });
    let receive = tokio::spawn(async move {
        let mut buf = [0u8; 16];
        pod_core::datagrams::recv_from(&*datagrams, &mut buf).await.map(|(n, _)| n)
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!read.is_finished() && !receive.is_finished(), "the reads wait");
    (read, receive)
}

/// The read in `task` fails at once with `ConnectionReset` and `why`.
async fn fails_with(task: tokio::task::JoinHandle<std::io::Result<usize>>, why: &str) {
    let err = tokio::time::timeout(LIMIT, task).await.expect("the read waits for ever").unwrap().unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::ConnectionReset, "{err}");
    assert!(err.to_string().contains(why), "{err}");
}

/// A new dial and a new UDP socket of a closed session fail with the cause.
async fn refuses_new_flows(net: &pod_warp::WarpNet, why: &str) {
    let target = Target::Addr("203.0.113.9:80".parse().unwrap());
    let err = net.dial(&target, tokio::time::Instant::now() + LIMIT).await.err().unwrap();
    assert!(err.chain().contains(why), "{}", err.chain());
    let err = net.datagrams().await.err().unwrap();
    assert!(err.chain().contains(why), "{}", err.chain());
}

/// PodUtils/podnet issue 125: the edge ends a session. The read of an open
/// stream and a UDP receive fail at once with the cause, not a wait for
/// ever, and a new flow fails with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_flows_of_a_session_that_the_edge_ends_fail_at_once() {
    pod_testkit::need_loopback!();
    let (_api, served, edge, moved) = world(dead()).await;
    let (settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    let net = Warp::register(settings).await.unwrap().connect().await.unwrap();
    let (read, receive) = open_flows(&net).await;
    edge.end_sessions();
    fails_with(read, "the WARP session is closed").await;
    assert!(net.is_closed());
    let why = net.close_reason().unwrap();
    fails_with(receive, &why).await;
    refuses_new_flows(&net, &why).await;
}

/// PodUtils/podnet issue 185: the caller closes a session. Its flows fail
/// at once with the cause, a new flow fails, and a second close keeps the
/// first cause.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_by_the_caller_fails_the_flows_of_the_session() {
    pod_testkit::need_loopback!();
    let (_api, served, _edge, moved) = world(dead()).await;
    let (settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    let net = Warp::register(settings).await.unwrap().connect().await.unwrap();
    let (read, receive) = open_flows(&net).await;
    net.close("the session is too old");
    net.close("a later cause");
    assert_eq!(net.close_reason().as_deref(), Some("the session is too old"));
    fails_with(read, "the WARP session is closed: the session is too old").await;
    fails_with(receive, "the session is too old").await;
    refuses_new_flows(&net, "the session is too old").await;
}

/// A device of the stand-in whose record keeps the addresses of
/// `families` (IPv4, IPv6), a session with a check that asks a server of
/// each family, and a UDP socket of the session.
async fn one_family(families: (bool, bool)) -> (Edge, pod_warp::WarpNet, Arc<dyn pod_core::Datagrams>, Vec<IpAddr>) {
    let (_api, served, edge, moved) = world(dead()).await;
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    let servers = pod_warp::resolver::parse_resolvers(&["[2606:4700:4700::1111]:53,1.1.1.1:53".into()]).unwrap();
    settings.net.check = Some(pod_warp::DataPathCheck { servers, ..pod_warp::DataPathCheck::default() });
    let warp = Warp::register(settings.clone()).await.unwrap();
    let mut device = warp.device().clone();
    if !families.0 {
        device.config.interface.addresses.v4.clear();
    }
    if !families.1 {
        device.config.interface.addresses.v6.clear();
    }
    let warp = Warp::from_parts(settings, device, warp.identity().clone());
    let net = warp.connect().await.unwrap();
    let datagrams = net.datagrams().await.unwrap();
    let ours = net.addresses();
    (edge, net, datagrams, ours)
}

/// A datagram to the echo port of `to` comes back from `to`, and the edge
/// saw it come from `source`.
async fn echoes(edge: &Edge, datagrams: &dyn pod_core::Datagrams, to: SocketAddr, source: IpAddr) {
    pod_core::datagrams::send_to(datagrams, b"query", to).await.unwrap();
    let mut buf = [0u8; 64];
    let (n, from) = tokio::time::timeout(LIMIT, pod_core::datagrams::recv_from(datagrams, &mut buf))
        .await
        .expect("an echo")
        .unwrap();
    assert_eq!((&buf[..n], from), (&b"query"[..], to));
    let seen = edge.watch.udp.lock().unwrap().clone();
    assert!(seen.contains(&(source, to)), "{to} from {source}: {seen:?}");
}

/// A send to `to` is refused with `InvalidInput` and `words`, and no packet
/// leaves into the session.
async fn refused(net: &pod_warp::WarpNet, datagrams: &dyn pod_core::Datagrams, to: SocketAddr, words: &str) {
    let before = net.link_stats().tx_packets;
    let err = pod_core::datagrams::send_to(datagrams, b"query", to).await.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{err}");
    assert!(err.to_string().contains(words), "{err}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(net.link_stats().tx_packets, before, "a packet left");
}

fn echo_v4() -> SocketAddr {
    format!("198.51.100.20:{}", edge::ECHO_PORT).parse().unwrap()
}

fn echo_v6() -> SocketAddr {
    format!("[2001:db8:7::20]:{}", edge::ECHO_PORT).parse().unwrap()
}

/// PodUtils/podnet issue 112: a datagram to IPv6 on a tunnel with an IPv4
/// address only is refused, and the check of the data path asks no IPv6
/// server: no IPv6 packet leaves. IPv4 still works, from the address of the
/// device.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_to_ipv6_on_an_ipv4_tunnel_is_refused() {
    pod_testkit::need_loopback!();
    let (edge, net, datagrams, ours) = one_family((true, false)).await;
    refused(&net, &*datagrams, "[2606:4700:4700::1111]:53".parse().unwrap(), "no IPv6 address").await;
    echoes(&edge, &*datagrams, echo_v4(), ours[0]).await;
    assert_eq!(edge.watch.families[1].load(Ordering::SeqCst), 0, "an IPv6 packet left");
}

/// PodUtils/podnet issue 112: on a tunnel with both families, each datagram
/// has the source of the family of its destination.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_on_a_dual_stack_tunnel_takes_the_source_of_each_family() {
    pod_testkit::need_loopback!();
    let (edge, _net, datagrams, ours) = one_family((true, true)).await;
    echoes(&edge, &*datagrams, echo_v6(), ours[1]).await;
    echoes(&edge, &*datagrams, echo_v4(), ours[0]).await;
}

/// PodUtils/podnet issue 112: a datagram to IPv4 on a tunnel with an IPv6
/// address only is refused, and no IPv4 packet leaves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_to_ipv4_on_an_ipv6_tunnel_is_refused() {
    pod_testkit::need_loopback!();
    let (edge, net, datagrams, ours) = one_family((false, true)).await;
    refused(&net, &*datagrams, "1.1.1.1:53".parse().unwrap(), "no IPv4 address").await;
    echoes(&edge, &*datagrams, echo_v6(), ours[0]).await;
    assert_eq!(edge.watch.families[0].load(Ordering::SeqCst), 0, "an IPv4 packet left");
}
