//! WireGuard for the WARP tunnel (PodUtils/podnet issues 152, 154 and 155)
//! against the stand-in WireGuard front door of `pod-wg` on the loopback: a
//! tunnel carries packets both ways, a way with no UDP and a door with no
//! answer fail, `Warp::connect` gives a network over WireGuard, and a
//! device carried inside a session of the stand-in edge gives a network.

mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use common::edge::{self, Edge, Route};
use pod_core::{Class, Dial, Target};
use pod_testkit::stand;
use pod_warp::wg::Peer;
use pod_warp::{MasqueSession, Settings, SystemUdp, Warp, WarpNet, Way};
use pod_wg::standin::{self, DataPath};

const LIMIT: Duration = Duration::from_secs(10);
/// The address of the device of the stand-in of `pod-wg`.
const DEVICE: Ipv4Addr = Ipv4Addr::new(172, 16, 0, 2);
/// The address of the carried front door inside the tunnel of the edge.
const CARRIED_DOOR: &str = "203.0.113.50:2408";

/// A way on the UDP of the system; its dial is never used.
fn udp_way() -> Way {
    let mut way = Way::new(common::Moved::new(vec![]), common::provider());
    way.udp = Some(Arc::new(SystemUdp));
    way.timeout = LIMIT;
    way
}

/// A stand-in front door of WireGuard on the loopback and its peer.
async fn front_door(reserved: [u8; 3], path: DataPath) -> Peer {
    let (keys, endpoint) = standin::start(reserved, path).await;
    Peer { keys, endpoint, v4: Some(DEVICE), v6: None }
}

/// An IPv4 UDP packet from the device, with no checksum.
fn udp_packet(payload: &[u8]) -> Vec<u8> {
    let len = 28 + payload.len();
    let mut p = vec![0x45, 0, (len >> 8) as u8, len as u8, 0, 0, 0x40, 0, 64, 17, 0, 0];
    p.extend_from_slice(&DEVICE.octets());
    p.extend_from_slice(&[198, 51, 100, 7]);
    p.extend_from_slice(&[0x30, 0x39, 0, 7, 0, (8 + payload.len()) as u8, 0, 0]);
    p.extend_from_slice(payload);
    p
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wireguard_tunnel_carries_packets_both_ways() {
    pod_testkit::need_loopback!();
    // WARP reads the reserved bytes of each message: the stand-in drops a
    // message without them.
    let peer = front_door([1, 2, 3], DataPath::NeedsReserved).await;
    let session = MasqueSession::connect_wg(&peer, &udp_way()).await.unwrap();
    assert!(session.is_wg() && !session.is_h3());
    let (mut tx, mut rx) = session.split();
    assert!(tx.keepalive(&rx).is_some());
    assert!(tx.keepalive(&rx).is_none(), "one check for each session");
    let packets = [udp_packet(b"one"), udp_packet(b"two")];
    tx.send_packets(&packets).await.unwrap();
    for sent in &packets {
        let packet = tokio::time::timeout(LIMIT, rx.recv_packet()).await.expect("a packet back").unwrap().unwrap();
        assert_eq!(&packet[..], &sent[..]);
    }
    tx.close().unwrap();
    assert!(tx.send_packet(&packets[0]).await.is_err(), "a closed session sends nothing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wireguard_refuses_a_way_with_no_udp() {
    pod_testkit::need_loopback!();
    let peer = front_door([0; 3], DataPath::All).await;
    let way = Way { udp: None, ..udp_way() };
    let err = MasqueSession::connect_wg(&peer, &way).await.err().expect("a way with no UDP fails");
    assert!(err.chain().contains("UDP"), "{}", err.chain());
    assert_eq!(err.class, Class::Usage);
}

#[tokio::test]
async fn no_answer_is_a_failure_that_names_the_door() {
    pod_testkit::need_loopback!();
    let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (keys, _) = standin::keys([0; 3]);
    let peer = Peer { keys, endpoint: silent.local_addr().unwrap(), v4: None, v6: None };
    tokio::time::pause();
    let task = tokio::spawn(async move { MasqueSession::connect_wg(&peer, &udp_way()).await.err() });
    let err = task.await.unwrap().expect("no answer fails");
    assert!(err.chain().contains("no WireGuard handshake answer"), "{}", err.chain());
    assert!(err.chain().contains(&silent.local_addr().unwrap().to_string()), "{}", err.chain());
    assert_eq!(err.class, Class::Timeout);
}

/// The stand-in answers a TCP SYN with a SYN-ACK: a dial through the
/// network opens.
async fn opens(net: &WarpNet) {
    let target = Target::Addr("198.51.100.7:80".parse().unwrap());
    net.dial(&target, tokio::time::Instant::now() + LIMIT).await.unwrap();
}

/// A registered device of the stand-in API, with no check of the data path.
async fn device(dial: Arc<common::Moved>) -> (Settings, Arc<common::Kept>, stand::Stand) {
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    let served = stand::http(api.handler()).unwrap();
    let (mut settings, kept) = common::settings(&format!("http://{}", served.addr()), dial);
    settings.net.check = None;
    (settings, kept, served)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn warp_connect_over_wireguard_gives_a_network() {
    pod_testkit::need_loopback!();
    let peer = front_door([0; 3], DataPath::All).await;
    let dial = common::Moved::new(vec![]);
    let (mut settings, kept, _served) = device(dial.clone()).await;
    settings.way = udp_way();
    settings.wireguard = Some(peer.clone());
    let warp = Warp::register(settings).await.unwrap();
    let net = warp.connect().await.unwrap();
    assert_eq!(net.addresses(), vec![std::net::IpAddr::V4(DEVICE)]);
    opens(&net).await;
    assert_eq!(warp.endpoint_in_use(), Some(Target::Addr(peer.endpoint)));
    assert_eq!(dial.dials.load(std::sync::atomic::Ordering::SeqCst), 0, "no TCP to a front door");
    let decisions = kept.decisions.lock().unwrap().clone();
    assert!(decisions.contains(&("warp.leg".into(), "wireguard".into())), "{decisions:?}");
}

/// A second device on WireGuard inside a session of the stand-in edge: the
/// edge carries the UDP of the tunnel to the stand-in front door of
/// WireGuard, and the network is the stack of the carried device.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_carried_device_gives_a_network_inside_the_session() {
    pod_testkit::need_loopback!();
    let peer = front_door([0; 3], DataPath::All).await;
    let door: SocketAddr = CARRIED_DOOR.parse().unwrap();
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    let udp = vec![Route { port: door.port(), outside: peer.endpoint }];
    let edge = Edge::start_with_udp(api.clone(), vec![], udp).await;
    api.serve_key(&edge.pem());
    let served = stand::http(api.handler()).unwrap();
    let moved = common::Moved::new(vec![(edge::door_v4(), edge.addr), (edge::door_v6(), edge.addr)]);
    let (mut settings, kept) = common::settings(&format!("http://{}", served.addr()), moved);
    settings.net.check = None;
    settings.carry = Some(Peer { endpoint: door, ..peer });
    let warp = Warp::register(settings).await.unwrap();
    let net = warp.connect().await.unwrap();
    assert_eq!(net.addresses(), vec![std::net::IpAddr::V4(DEVICE)]);
    opens(&net).await;
    assert_eq!(edge.tunnels(), 1);
    let decisions = kept.decisions.lock().unwrap().clone();
    assert!(decisions.contains(&("warp.carry".into(), "carried".into())), "{decisions:?}");
}
