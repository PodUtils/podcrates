//! The L4 door (PodUtils/podnet issue 140) against a stand-in door of
//! HTTP/3 on the loopback: a flow is one CONNECT stream, a closed
//! connection opens again, and a proxy over the door decides its rules with
//! no lookup of a name.

mod common;

use common::edge::{self, Edge, Route};

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_core::{Class, Dial, Report, Target};
use pod_quic::noq;
use pod_testkit::stand;
use pod_warp::h3::{self, Frames};
use pod_warp::l4::{Form, L4, connect_headers_form};
use pod_warp::proxy::Proxy;
use pod_warp::rules::{Action, NoLint, Rules, Table};
use pod_warp::{Door, Identity, SystemUdp, Warp};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, DuplexStream};

/// The bound of each wait: a guard of a hang, five times longer under an
/// emulator (T-532).
fn limit() -> Duration {
    pod_testkit::host::scaled(Duration::from_secs(10))
}
/// The size of the one DATA frame that the door sends to `large.test`:
/// larger than the limit of a frame of the HTTP/3 client.
const LARGE: usize = (1 << 20) + 10;

/// The certificate of the door, made for each handshake.
#[derive(Debug)]
struct Shown(Identity);

impl ResolvesServerCert for Shown {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        Some(Arc::new(self.0.certified(now)))
    }
}

/// What the stand-in door saw: its connections and the field section of
/// each CONNECT.
#[derive(Default)]
struct Seen {
    connections: AtomicUsize,
    requests: Mutex<Vec<Vec<u8>>>,
}

impl Seen {
    fn saw(&self, authority: &str) -> bool {
        let want = connect_headers_form(authority, Form::Plain);
        self.requests.lock().unwrap().contains(&want)
    }
}

/// A stand-in L4 door on the loopback: each CONNECT to `refused.test` gets
/// 403, to `large.test` one DATA frame of [`LARGE`] bytes, and each other
/// one 200 and an echo of its DATA frames. It gives its address and its
/// pinned key.
async fn l4_door() -> (SocketAddr, Vec<u8>, Arc<Seen>) {
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
    let seen = Arc::new(Seen::default());
    let kept = seen.clone();
    tokio::spawn(async move {
        while let Some(incoming) = server.accept().await {
            let Ok(conn) = incoming.await else { continue };
            kept.connections.fetch_add(1, Ordering::SeqCst);
            let seen = kept.clone();
            tokio::spawn(async move {
                while let Ok((send, recv)) = conn.accept_bi().await {
                    tokio::spawn(stream(send, recv, seen.clone()));
                }
            });
        }
    });
    (addr, pin, seen)
}

/// One CONNECT stream of the stand-in door.
async fn stream(mut send: noq::SendStream, mut recv: noq::RecvStream, seen: Arc<Seen>) {
    let mut frames = Frames::default();
    loop {
        while let Ok(Some((kind, payload))) = frames.next_frame() {
            if kind == h3::FRAME_HEADERS {
                seen.requests.lock().unwrap().push(payload.clone());
                if payload.windows(12).any(|w| w == b"refused.test") {
                    // :status 403 (static 68).
                    let _ = send.write_all(&h3::frame(h3::FRAME_HEADERS, &[0, 0, 0xff, 68 - 63])).await;
                    let _ = send.finish();
                    return;
                }
                let _ = send.write_all(&h3::frame(h3::FRAME_HEADERS, &[0, 0, 0xc0 | 25])).await;
                if payload.windows(10).any(|w| w == b"large.test") {
                    let _ = send.write_all(&h3::frame(h3::FRAME_DATA, &vec![9u8; LARGE])).await;
                    let _ = send.finish();
                    return;
                }
            } else if kind == h3::FRAME_DATA {
                let _ = send.write_all(&h3::frame(h3::FRAME_DATA, &payload)).await;
            }
        }
        match recv.read_chunk(65536).await {
            Ok(Some(chunk)) => frames.push(&chunk),
            _ => break,
        }
    }
    let _ = send.finish();
}

/// The L4 door at `addr` with the key `pin`, on the UDP of the system.
fn door_at(addr: SocketAddr, pin: Vec<u8>) -> L4 {
    let client_id = Identity::generate().unwrap();
    let tls = pod_warp::tls::client_config(common::provider(), &client_id, pin, h3::ALPN).unwrap();
    L4::new(addr, Arc::new(SystemUdp), tls)
}

async fn echoes(pipe: &mut DuplexStream, text: &[u8]) {
    pipe.write_all(text).await.unwrap();
    let mut back = vec![0u8; text.len()];
    tokio::time::timeout(limit(), pipe.read_exact(&mut back)).await.expect("an echo").unwrap();
    assert_eq!(back, text);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_flow_through_an_l4_door_on_the_loopback() {
    pod_testkit::need_loopback!();
    let (addr, pin, seen) = l4_door().await;
    let door = door_at(addr, pin);
    let mut pipe = door.open(&Target::new("example.com", 443)).await.unwrap();
    echoes(&mut pipe, b"ping through L4").await;
    assert!(seen.saw("example.com:443"), "the plain CONNECT");
    // The Dial of the door: an IPv6 address goes in brackets.
    let deadline = tokio::time::Instant::now() + limit();
    let connected = door.dial(&Target::new("2001:db8::1", 22), deadline).await.unwrap();
    let mut io = connected.io;
    io.write_all(b"ssh").await.unwrap();
    let mut back = [0u8; 3];
    tokio::time::timeout(limit(), io.read_exact(&mut back)).await.expect("an echo").unwrap();
    assert_eq!(&back, b"ssh");
    assert!(seen.saw("[2001:db8::1]:22"));
    assert_eq!(seen.connections.load(Ordering::SeqCst), 1, "one QUIC connection for each flow");
    // A DATA frame larger than the limit of a frame of the client.
    let mut pipe = door.open(&Target::new("large.test", 443)).await.unwrap();
    let mut all = Vec::new();
    tokio::time::timeout(limit(), pipe.read_to_end(&mut all)).await.expect("the end of the flow").unwrap();
    assert!(all.len() == LARGE && all.iter().all(|b| *b == 9), "{} bytes", all.len());
    // A refusal of the door.
    let err = door.open(&Target::new("refused.test", 443)).await.unwrap_err();
    assert_eq!(err.class, Class::Refused);
    assert!(err.chain().contains("403"), "{}", err.chain());
}

/// The door opens a new QUIC connection when the one of now has closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_l4_door_opens_a_new_connection_after_a_close() {
    pod_testkit::need_loopback!();
    let (addr, pin, seen) = l4_door().await;
    let door = door_at(addr, pin);
    let first = door.connection().await.unwrap();
    assert_eq!(door.connection().await.unwrap().stable_id(), first.stable_id(), "the open connection is kept");
    first.close(0u32.into(), b"closed by the test");
    let mut pipe = door.open(&Target::new("example.com", 443)).await.unwrap();
    echoes(&mut pipe, b"after the close").await;
    assert_eq!(seen.connections.load(Ordering::SeqCst), 2);
}

/// A SOCKS5 client of `proxy` on a pipe: the greeting, then `command` to
/// the name `host`; the reply.
async fn socks5(proxy: &Arc<Proxy>, command: u8, host: &str, port: u16) -> (DuplexStream, [u8; 10]) {
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    assert!(proxy.client(server, Some(("127.0.0.1".parse().unwrap(), "127.0.0.1".parse().unwrap()))));
    client.write_all(&[5, 1, 0]).await.unwrap();
    let mut reply = [0u8; 2];
    client.read_exact(&mut reply).await.unwrap();
    let mut head = vec![5, command, 0];
    match host.parse::<std::net::Ipv4Addr>() {
        Ok(ip) => {
            head.push(1);
            head.extend_from_slice(&ip.octets());
        }
        Err(_) => {
            head.extend_from_slice(&[3, host.len() as u8]);
            head.extend_from_slice(host.as_bytes());
        }
    }
    head.extend_from_slice(&port.to_be_bytes());
    client.write_all(&head).await.unwrap();
    let mut reply = [0u8; 10];
    tokio::time::timeout(limit(), client.read_exact(&mut reply)).await.expect("a reply").unwrap();
    (client, reply)
}

/// PodUtils/podnet issue 140: the rules of a proxy over the L4 door apply
/// with no lookup of a name. A domain rule refuses its name, an address
/// rule refuses its address, a name that only an address rule could
/// decide takes the default, a `direct` name goes to the direct dial, and
/// a UDP command gets the reply 7.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proxy_over_the_l4_door_decides_with_no_lookup() {
    pod_testkit::need_loopback!();
    let (addr, pin, seen) = l4_door().await;
    let echo = common::edge::echo_server().await;
    let direct = common::Moved::new(vec![]);
    let specs: Vec<String> = ["domain:blocked.test=reject", "cidr:198.51.100.0/24=reject", "domain:direct.test=direct"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let (table, _) = Table::build(&specs, None, Action::Tunnel, &NoLint, false).unwrap();
    let kept = Arc::new(common::Kept::default());
    let mut proxy = Proxy::over_l4(Arc::new(door_at(addr, pin)), direct.clone());
    proxy.rules = Arc::new(Rules::new(table));
    proxy.report = Report { events: kept.clone(), observer: kept.clone() };
    let proxy = Arc::new(proxy);
    let (_c, reply) = socks5(&proxy, 1, "blocked.test", 443).await;
    assert_eq!(reply[1], 2, "a domain rule refuses");
    let (_c, reply) = socks5(&proxy, 1, "198.51.100.7", 443).await;
    assert_eq!(reply[1], 2, "an address rule refuses");
    let (mut client, reply) = socks5(&proxy, 1, "named.test", 443).await;
    assert_eq!(reply[1], 0, "no lookup: the default");
    echoes(&mut client, b"ping").await;
    assert!(seen.saw("named.test:443"), "the door takes the name");
    let (mut client, reply) = socks5(&proxy, 1, "direct.test", echo.port()).await;
    assert_eq!(reply[1], 0);
    echoes(&mut client, b"ping").await;
    assert_eq!(direct.dials.load(Ordering::SeqCst), 1, "the direct dial took it");
    let (_c, reply) = socks5(&proxy, 3, "0.0.0.0", 0).await;
    assert_eq!(reply[1], 7, "the door carries no UDP");
    assert_eq!(seen.requests.lock().unwrap().len(), 1, "the door saw one CONNECT");
    let health = proxy.health();
    assert_eq!((health.rejected, health.direct, health.tunnel), (2, 1, 1));
    let decisions = kept.decisions.lock().unwrap().clone();
    assert!(decisions.contains(&("warp.rule".into(), "l4".into())), "{decisions:?}");
}

/// `Warp::l4` takes the pinned door, the UDP of the way, the certificate
/// of the device and the pinned key of the record; a way with no UDP is
/// refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_l4_door_of_a_device_takes_its_door_and_its_key() {
    pod_testkit::need_loopback!();
    let (addr, pin, seen) = l4_door().await;
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    api.serve_key(&pod_warp::der::pem(&pin, "PUBLIC KEY"));
    let served = stand::http(api.handler()).unwrap();
    let (mut settings, _kept) = common::settings(&format!("http://{}", served.addr()), common::Moved::new(vec![]));
    settings.way.udp = Some(Arc::new(SystemUdp));
    settings.door = Door::Pinned(Target::Addr(addr));
    let warp = Warp::register(settings).await.unwrap();
    let door = warp.l4().unwrap().with_sni("l4.test");
    assert_eq!(door.door(), addr);
    let mut pipe = door.open(&Target::new("example.com", 443)).await.unwrap();
    echoes(&mut pipe, b"ping").await;
    assert!(seen.saw("example.com:443"));
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), common::Moved::new(vec![]));
    settings.door = Door::Pinned(Target::Name { host: "door.test".into(), port: 443 });
    settings.way.udp = Some(Arc::new(SystemUdp));
    let named = Warp::from_parts(settings, warp.device().clone(), warp.identity().clone());
    assert_eq!(named.l4().unwrap_err().class, Class::Usage);
    let (settings, _) = common::settings(&format!("http://{}", served.addr()), common::Moved::new(vec![]));
    let sealed = Warp::from_parts(settings, warp.device().clone(), warp.identity().clone());
    let err = sealed.l4().unwrap_err();
    assert_eq!(err.class, Class::Usage);
    assert!(err.chain().contains("TCP only"), "{}", err.chain());
}

/// The UDP port inside the tunnel that the stand-in edge carries to the
/// QUIC server of the loopback.
const QUIC_INSIDE: u16 = 4433;

/// PodUtils/podnet#524: a QUIC handshake through the datagrams of a WARP
/// session reaches an IPv4 peer. The socket of the session binds `[::]`, so
/// noq sends to the IPv4-mapped form of the peer; the datagrams of the
/// session take it as IPv4, and give the answers back in the same form.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quic_handshake_through_the_datagrams_of_a_session_reaches_an_ipv4_peer() {
    pod_testkit::need_loopback!();
    let (addr, pin, _seen) = l4_door().await;
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    let udp = vec![Route { port: QUIC_INSIDE, outside: addr }];
    let edge = Edge::start_with_udp(api.clone(), vec![], udp).await;
    api.serve_key(&edge.pem());
    let served = stand::http(api.handler()).unwrap();
    let moved = common::Moved::new(vec![(edge::door_v4(), edge.addr), (edge::door_v6(), edge.addr)]);
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    settings.net.check = None;
    let net = Warp::register(settings).await.unwrap().connect().await.unwrap();
    let endpoint = pod_quic::endpoint::over(net.datagrams().await.unwrap(), None).unwrap();
    let client_id = Identity::generate().unwrap();
    let tls = pod_warp::tls::client_config(common::provider(), &client_id, pin, h3::ALPN).unwrap();
    let crypto = noq::crypto::rustls::QuicClientConfig::try_from(tls).unwrap();
    let to: SocketAddr = format!("198.51.100.30:{QUIC_INSIDE}").parse().unwrap();
    let connecting = endpoint.connect_with(noq::ClientConfig::new(Arc::new(crypto)), to, "warp.test").unwrap();
    let conn = tokio::time::timeout(limit(), connecting).await.expect("a QUIC handshake in time").unwrap();
    conn.close(0u32.into(), b"done");
    // The packets to the peer are IPv4 packets inside the tunnel: WARP's
    // network drops an IPv6 packet to `::ffff:a.b.c.d`.
    let seen = edge.watch.udp.lock().unwrap().clone();
    let to_peer: Vec<_> = seen.iter().filter(|(_, at)| at.port() == QUIC_INSIDE).collect();
    assert!(!to_peer.is_empty(), "no packet to the peer: {seen:?}");
    assert!(to_peer.iter().all(|(_, at)| *at == to), "{to_peer:?}");
}
