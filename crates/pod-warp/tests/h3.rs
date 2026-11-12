//! MASQUE over HTTP/3 (PodUtils/podnet issue 157) against a front door of
//! HTTP/3 on the loopback: datagrams and capsules go both ways, a wrong key
//! and a way with no UDP are refused, and a session over HTTP/3 takes no
//! relay.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use pod_core::{Class, Target};
use pod_quic::noq;
use pod_testkit::stand;
use pod_warp::h3::{self, Frames};
use pod_warp::leg::Relay;
use pod_warp::tls::is_pin_mismatch;
use pod_warp::{Door, Identity, MasqueSession, SystemUdp, Warp, Way};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;

/// The certificate of the front door, made for each handshake.
#[derive(Debug)]
struct Shown(Identity);

impl ResolvesServerCert for Shown {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        Some(Arc::new(self.0.certified(now)))
    }
}

/// A front door of HTTP/3 on the loopback: it checks the extended CONNECT,
/// answers 200, and sends each datagram and each DATA frame back. It gives
/// its address and its pinned key.
async fn h3_front_door() -> (SocketAddr, Vec<u8>, tokio::task::JoinHandle<()>) {
    let server_id = Identity::generate().unwrap();
    let pin = server_id.spki_der().to_vec();
    let mut tls = rustls::ServerConfig::builder_with_provider(common::provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Shown(server_id)));
    tls.alpn_protocols = vec![h3::ALPN.to_vec()];
    let crypto = noq::crypto::rustls::QuicServerConfig::try_from(Arc::new(tls)).unwrap();
    let config = pod_quic::endpoint::server_config(Arc::new(crypto)).unwrap();
    let server = pod_quic::endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = server.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let conn = server.accept().await.unwrap().await.unwrap();
        let (mut send, mut recv) = conn.accept_bi().await.unwrap();
        let mut frames = Frames::default();
        let headers = loop {
            if let Some((kind, payload)) = frames.next_frame().unwrap() {
                if kind == h3::FRAME_HEADERS {
                    break payload;
                }
                continue;
            }
            let chunk = recv.read_chunk(65536).await.unwrap().unwrap();
            frames.push(&chunk);
        };
        assert!(headers.windows(13).any(|w| w == b"cf-connect-ip"));
        assert_eq!(&headers[2..5], &[0xcf, 0xd7, 0xc1]);
        send.write_all(&h3::frame(h3::FRAME_HEADERS, &[0, 0, 0xc0 | 25])).await.unwrap();
        let echo = conn.clone();
        tokio::spawn(async move {
            while let Ok(d) = echo.read_datagram().await {
                let _ = echo.send_datagram(d);
            }
        });
        loop {
            while let Some((kind, payload)) = frames.next_frame().unwrap() {
                if kind == h3::FRAME_DATA {
                    send.write_all(&h3::frame(h3::FRAME_DATA, &payload)).await.unwrap();
                }
            }
            match recv.read_chunk(65536).await {
                Ok(Some(chunk)) => frames.push(&chunk),
                _ => break,
            }
        }
    });
    (addr, pin, task)
}

/// A way over HTTP/3 on the UDP of the system; its dial is never used.
fn h3_way() -> Way {
    let mut way = Way::new(common::Moved::new(vec![]), common::provider());
    way.h3 = true;
    way.udp = Some(Arc::new(SystemUdp));
    way.timeout = Duration::from_secs(10);
    way
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_over_http3_carries_datagrams_and_capsules() {
    pod_testkit::need_loopback!();
    let (addr, pin, server) = h3_front_door().await;
    let client_id = Identity::generate().unwrap();
    let session = MasqueSession::connect(&Target::Addr(addr), &client_id, pin, &h3_way()).await.unwrap();
    assert!(session.is_h3());
    let (mut tx, mut rx) = session.split();
    assert!(tx.keepalive(&rx).is_some());
    assert!(tx.keepalive(&rx).is_none(), "one check for each session");
    // A small packet (a datagram) and one larger than a datagram (a DATA
    // capsule on the stream).
    let small = vec![0x45u8; 200];
    let large = vec![0x60u8; 3000];
    tx.send_packets(&[small.clone(), large.clone()]).await.unwrap();
    let mut got = Vec::new();
    for _ in 0..2 {
        let packet = tokio::time::timeout(Duration::from_secs(5), rx.recv_packet())
            .await
            .expect("a packet back")
            .unwrap()
            .unwrap();
        got.push(packet.to_vec());
    }
    got.sort_by_key(|p| p.len());
    assert_eq!(got, vec![small, large]);
    assert!(rx.progress().load(Ordering::Relaxed) >= 2);
    tx.close().unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http3_refuses_a_proxy_and_a_wrong_key() {
    pod_testkit::need_loopback!();
    let (addr, _pin, server) = h3_front_door().await;
    let client_id = Identity::generate().unwrap();
    let other = Identity::generate().unwrap();
    let door = Target::Addr(addr);
    let err = MasqueSession::connect(&door, &client_id, other.spki_der().to_vec(), &h3_way())
        .await
        .err()
        .expect("a wrong key fails");
    assert!(err.chain().contains("pinned"), "{}", err.chain());
    assert!(is_pin_mismatch(&err), "the record is read again: {}", err.chain());
    // A way with no UDP is a proxy or a relay: it carries TCP only.
    let proxy = Way { udp: None, ..h3_way() };
    let err = MasqueSession::connect(&door, &client_id, vec![], &proxy).await.err().expect("a proxy fails");
    assert!(err.chain().contains("UDP") && err.chain().contains("TCP only"), "{}", err.chain());
    assert_eq!(err.class, Class::Usage);
    // A name is not an address of a door.
    let name = Target::Name { host: "door.test".into(), port: 443 };
    let err = MasqueSession::connect(&name, &client_id, vec![], &h3_way()).await.err().expect("a name fails");
    assert_eq!(err.class, Class::Usage);
    server.abort();
}

/// With a relay in the settings, a session over HTTP/3 goes on the UDP of
/// the way alone: the relay carries TCP, and its dial is not used.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_over_http3_takes_no_relay() {
    pod_testkit::need_loopback!();
    let (addr, pin, server) = h3_front_door().await;
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    api.serve_key(&pod_warp::der::pem(&pin, "PUBLIC KEY"));
    let served = stand::http(api.handler()).unwrap();
    let (mut settings, kept) = common::settings(&format!("http://{}", served.addr()), common::Moved::new(vec![]));
    settings.way = h3_way();
    settings.door = Door::Pinned(Target::Addr(addr));
    let relay = common::Moved::new(vec![]);
    settings.relay = Some(Relay::new(relay.clone(), "wss://relay.test/"));
    let warp = Warp::register(settings).await.unwrap();
    let session = warp.open_session().await.unwrap();
    assert!(session.is_h3());
    assert_eq!(relay.dials.load(Ordering::SeqCst), 0, "the relay is not dialed");
    let decisions = kept.decisions.lock().unwrap().clone();
    assert!(decisions.contains(&("warp.leg".into(), "no-relay".into())), "{decisions:?}");
    server.abort();
}
