//! The proxy into WARP (podnet's `src/warp/proxy.rs`, T-338) against the
//! stand-in API and the stand-in edge on the loopback: the resolvers on UDP
//! and over TLS, the resend of a lost question, the DNS forwarder, the UDP
//! associations, and the rules of the proxy; the forward and the UDP pipe
//! of nc (`src/warp/forward.rs`, `src/warp/nc.rs`).

mod common;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::edge::{self, Edge, NAME_IP, Route};
use pod_core::{Dial, Report};
use pod_dns::wire;
use pod_testkit::stand;
use pod_warp::forward::{Forward, Rule};
use pod_warp::nc;
use pod_warp::proxy::{Fixed, Names, Proxy};
use pod_warp::resolver::{Resolver, Resolvers};
use pod_warp::rules::{Action, NoLint, Rules, Table};
use pod_warp::{DataPathCheck, Warp, WarpNet};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, DuplexStream};

const LIMIT: Duration = Duration::from_secs(20);
/// The port of the echo service inside the tunnel.
const ECHO_TCP: u16 = 80;
/// The address of a resolver inside the tunnel: the far end answers DNS at
/// each address.
const PEER_DNS: &str = "198.51.100.7:53";
/// The name that the certificate of the resolver over TLS has.
const DOT_NAME: &str = "dns.test";

/// The stand-in API, the edge with an echo route and a route of DNS over
/// TLS, and a session of a registered device; with `v6` false the device
/// has no IPv6 address.
async fn session(v6: bool) -> (Edge, WarpNet, Arc<rustls::ClientConfig>, stand::Stand) {
    let echo = edge::echo_server().await;
    let (dot, tls) = dot_server().await;
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    let routes = vec![Route { port: ECHO_TCP, outside: echo }, Route { port: 853, outside: dot }];
    let edge = Edge::start(api.clone(), routes).await;
    api.serve_key(&edge.pem());
    let served = stand::http(api.handler()).unwrap();
    let moved = common::Moved::new(vec![(edge::door_v4(), edge.addr), (edge::door_v6(), edge.addr)]);
    let (settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    let warp = Warp::register(settings.clone()).await.unwrap();
    let mut device = warp.device().clone();
    if !v6 {
        device.config.interface.addresses.v6.clear();
    }
    let warp = Warp::from_parts(settings, device, warp.identity().clone());
    (edge, warp.connect().await.unwrap(), tls, served)
}

/// A server of DNS over TLS on the loopback (RFC 7858) with a key of its
/// own, and the TLS client that pins that key.
async fn dot_server() -> (SocketAddr, Arc<rustls::ClientConfig>) {
    let key = pod_warp::key::DeviceKey::generate().unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let certified = Arc::new(key.certified(now - 60));
    let config = rustls::ServerConfig::builder_with_provider(common::provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(One(certified)));
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else { return };
                let mut len = [0u8; 2];
                while tls.read_exact(&mut len).await.is_ok() {
                    let mut query = vec![0u8; usize::from(u16::from_be_bytes(len))];
                    if tls.read_exact(&mut query).await.is_err() {
                        return;
                    }
                    let answer = edge::dns_answer(&query);
                    let mut framed = (answer.len() as u16).to_be_bytes().to_vec();
                    framed.extend_from_slice(&answer);
                    if tls.write_all(&framed).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    let identity = pod_warp::Identity::generate().unwrap();
    let client = pod_warp::tls::client_config(common::provider(), &identity, key.spki(), b"dot").unwrap();
    (addr, client)
}

/// One certificate for each handshake.
#[derive(Debug)]
struct One(Arc<rustls::sign::CertifiedKey>);

impl rustls::server::ResolvesServerCert for One {
    fn resolve(&self, _hello: rustls::server::ClientHello<'_>) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(self.0.clone())
    }
}

fn question(id: u16) -> Vec<u8> {
    let mut query = wire::query("example.com", wire::A).unwrap();
    query[..2].copy_from_slice(&id.to_be_bytes());
    query
}

/// PodUtils/podnet issue 145: a list of resolvers: the first that answers
/// wins.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_resolver_that_answers_wins() {
    pod_testkit::need_loopback!();
    let (_edge, net, _, _served) = session(true).await;
    let list = vec![Resolver::Udp("198.51.100.99:5353".parse().unwrap()), Resolver::Udp(PEER_DNS.parse().unwrap())];
    let resolvers = Resolvers { list, limit: Duration::from_millis(600), ..Resolvers::default() };
    let answer = resolvers.exchange(&net, &question(0x2468)).await.unwrap();
    assert_eq!(&answer[..2], &[0x24, 0x68]);
    let none = Resolvers { list: vec![Resolver::Udp("198.51.100.99:5353".parse().unwrap())], ..resolvers };
    let failure = none.exchange(&net, &question(0x2469)).await.unwrap_err();
    assert!(
        failure.chain().contains("no DNS answer through WARP (198.51.100.99:5353: no answer"),
        "{}",
        failure.chain()
    );
}

/// PodUtils/podnet issue 114: a lost DNS datagram is sent again; before, it
/// cost the full time limit (5 s) and gave no answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_dns_query_is_sent_again() {
    pod_testkit::need_loopback!();
    let (edge, net, _, _served) = session(true).await;
    let before = edge.watch.dns_questions.load(Ordering::SeqCst);
    edge.watch.dns_drop.store(1, Ordering::SeqCst);
    let start = std::time::Instant::now();
    let resolvers = Resolvers::new(vec![Resolver::Udp(PEER_DNS.parse().unwrap())]);
    let ips = resolvers.query(&net, "example.com", wire::A).await.unwrap();
    assert_eq!(ips, vec![IpAddr::V4(NAME_IP)]);
    assert_eq!(edge.watch.dns_questions.load(Ordering::SeqCst) - before, 2);
    assert!(start.elapsed() < pod_warp::resolver::DNS_TIMEOUT, "{:?}", start.elapsed());
}

/// PodUtils/podnet issues 144 and 145: a resolver over TLS answers through
/// the tunnel, and the check of the data path can ask it; with no TLS
/// client of the caller, it is no answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_check_of_the_data_path_asks_a_resolver_over_tls() {
    pod_testkit::need_loopback!();
    let (edge, net, tls, _served) = session(true).await;
    let dot = Resolver::parse(&format!("tls://198.51.100.53#{DOT_NAME}")).unwrap();
    let resolvers = Resolvers { list: vec![dot.clone()], tls: Some(tls), ..Resolvers::default() };
    let udp_before = edge.watch.dns_questions.load(Ordering::SeqCst);
    let ips = resolvers.query(&net, "example.com", wire::A).await.unwrap();
    assert_eq!(ips, vec![IpAddr::V4(NAME_IP)]);
    net.check_data_path(&DataPathCheck::of(&resolvers)).await.unwrap();
    assert_eq!(edge.watch.dns_questions.load(Ordering::SeqCst), udp_before, "no question on UDP");
    let check = DataPathCheck { servers: vec![dot], limit: Duration::from_secs(2), ..DataPathCheck::default() };
    let failure = net.check_data_path(&check).await.unwrap_err();
    assert!(failure.chain().contains("DNS over TLS needs the TLS settings of the caller"), "{}", failure.chain());
}

/// PodUtils/podnet issue 117: the DNS forwarder gives the answer of the
/// resolver through the tunnel, with the id of the question; a datagram
/// that is no DNS message gets nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_dns_forwarder_gives_the_answer() {
    pod_testkit::need_loopback!();
    let (_edge, net, _, _served) = session(true).await;
    let mut proxy = Proxy::new(Arc::new(Fixed(Arc::new(net.clone()))), Arc::new(net.clone()));
    proxy.names.resolvers = Resolvers::new(vec![Resolver::Udp(PEER_DNS.parse().unwrap())]);
    let proxy = Arc::new(proxy);
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let at = socket.local_addr().unwrap();
    let _forwarder = proxy.serve_dns(socket);
    let client = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client.send_to(b"short", at).await.unwrap();
    client.send_to(&question(0x7777), at).await.unwrap();
    let mut buf = [0u8; 512];
    let (n, _) = tokio::time::timeout(LIMIT, client.recv_from(&mut buf)).await.expect("an answer").unwrap();
    assert_eq!(&buf[..2], &[0x77, 0x77]);
    assert_eq!(wire::addrs(&buf[..n], "example.com", wire::A).unwrap(), vec![IpAddr::V4(NAME_IP)]);
    let short = Resolvers::default().exchange_with(&net, &Resolver::Udp(PEER_DNS.parse().unwrap()), b"short").await;
    assert!(short.is_err());
}

/// A proxy over `net` whose direct dial is `direct`, with `rules`.
fn proxy_of(net: &WarpNet, direct: Arc<dyn Dial>, rules: Rules) -> (Arc<Proxy>, Arc<common::Kept>) {
    let kept = Arc::new(common::Kept::default());
    let mut proxy = Proxy::new(Arc::new(Fixed(Arc::new(net.clone()))), direct);
    proxy.rules = Arc::new(rules);
    proxy.names.resolvers = Resolvers::new(vec![Resolver::Udp(PEER_DNS.parse().unwrap())]);
    proxy.report = Report { events: kept.clone(), observer: kept.clone() };
    (Arc::new(proxy), kept)
}

/// A SOCKS5 client of `proxy` on a pipe: the greeting, then `command` to
/// `request` (the address part of RFC 1928); the reply.
async fn socks5(proxy: &Arc<Proxy>, command: u8, request: &[u8]) -> (DuplexStream, [u8; 10]) {
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    assert!(proxy.client(server, Some(("127.0.0.1".parse().unwrap(), "127.0.0.1".parse().unwrap()))));
    client.write_all(&[5, 1, 0]).await.unwrap();
    let mut reply = [0u8; 2];
    client.read_exact(&mut reply).await.unwrap();
    let mut head = vec![5, command, 0];
    head.extend_from_slice(request);
    client.write_all(&head).await.unwrap();
    let mut reply = [0u8; 10];
    tokio::time::timeout(LIMIT, client.read_exact(&mut reply)).await.expect("a reply").unwrap();
    (client, reply)
}

fn by_name(name: &str, port: u16) -> Vec<u8> {
    let mut out = vec![3, name.len() as u8];
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&port.to_be_bytes());
    out
}

fn by_ip(ip: [u8; 4], port: u16) -> Vec<u8> {
    let mut out = vec![1];
    out.extend_from_slice(&ip);
    out.extend_from_slice(&port.to_be_bytes());
    out
}

async fn echoes(client: &mut DuplexStream) {
    client.write_all(b"ping").await.unwrap();
    let mut echo = [0u8; 4];
    tokio::time::timeout(LIMIT, client.read_exact(&mut echo)).await.expect("the echo").unwrap();
    assert_eq!(&echo, b"ping");
}

/// PodUtils/podnet issues 132 and 136: each destination takes the action of
/// its rule: a name that a domain rule refuses gets the reply 2; a name
/// whose address (resolved inside the tunnel) a cidr rule refuses too; an
/// address of a `direct` rule goes to the direct dial; the rest goes
/// through the tunnel. Each decision counts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_rules_of_the_proxy_route_each_destination() {
    pod_testkit::need_loopback!();
    let (_edge, net, _, _served) = session(true).await;
    let echo = edge::echo_server().await;
    let direct = common::Moved::new(vec![("203.0.113.5:80".parse().unwrap(), echo)]);
    let specs: Vec<String> =
        ["domain:blocked.test=reject", "cidr:198.51.100.0/24=reject", "cidr:203.0.113.0/24=direct"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    let (table, _) = Table::build(&specs, None, Action::Tunnel, &NoLint, false).unwrap();
    let (proxy, kept) = proxy_of(&net, direct.clone(), Rules::new(table));
    let (_c, reply) = socks5(&proxy, 1, &by_name("blocked.test", 80)).await;
    assert_eq!(reply[1], 2, "a domain rule refuses");
    let (_c, reply) = socks5(&proxy, 1, &by_name("named.test", 80)).await;
    assert_eq!(reply[1], 2, "the address of the name is in a refused block");
    let (mut client, reply) = socks5(&proxy, 1, &by_ip([203, 0, 113, 5], 80)).await;
    assert_eq!(reply[1], 0);
    echoes(&mut client).await;
    assert_eq!(direct.dials.load(Ordering::SeqCst), 1, "the direct dial took it");
    let (mut client, reply) = socks5(&proxy, 1, &by_ip([192, 0, 2, 80], ECHO_TCP)).await;
    assert_eq!(reply[1], 0);
    echoes(&mut client).await;
    assert_eq!(direct.dials.load(Ordering::SeqCst), 1, "the tunnel took it");
    let health = proxy.health();
    assert_eq!((health.rejected, health.direct, health.tunnel), (2, 1, 1));
    let decisions = kept.decisions.lock().unwrap().clone();
    assert!(decisions.contains(&("warp.rule".into(), "reject".into())), "{decisions:?}");
    assert!(decisions.contains(&("warp.dispatch".into(), "socks_connect".into())), "{decisions:?}");
}

/// An HTTP CONNECT client reaches the echo through the tunnel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http_connect_goes_through_the_tunnel() {
    pod_testkit::need_loopback!();
    let (_edge, net, _, _served) = session(true).await;
    let (proxy, _) = proxy_of(&net, Arc::new(net.clone()), Rules::default());
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    proxy.client(server, None);
    client.write_all(b"CONNECT 192.0.2.80:80 HTTP/1.1\r\nHost: 192.0.2.80:80\r\n\r\n").await.unwrap();
    let mut head = [0u8; 39];
    tokio::time::timeout(LIMIT, client.read_exact(&mut head)).await.expect("an answer").unwrap();
    assert!(head.starts_with(b"HTTP/1.1 200"), "{}", String::from_utf8_lossy(&head));
    echoes(&mut client).await;
}

/// PodUtils/podnet#278: a second local socket cannot take the replies of
/// the client of a UDP association.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_local_sender_can_not_take_the_udp_association() {
    pod_testkit::need_loopback!();
    let (_edge, net, _, _served) = session(true).await;
    let (proxy, _) = proxy_of(&net, Arc::new(net.clone()), Rules::default());
    let (_control, reply) = socks5(&proxy, 3, &by_ip([0, 0, 0, 0], 0)).await;
    assert_eq!(reply[1], 0);
    let relay = SocketAddr::new(
        IpAddr::from([reply[4], reply[5], reply[6], reply[7]]),
        u16::from_be_bytes([reply[8], reply[9]]),
    );
    let owner = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let other = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let v4 = pod_proxy::udp::wrap(&question(0x4242), "198.51.100.7".parse().unwrap(), 53);
    owner.send_to(&v4, relay).await.unwrap();
    let mut buf = [0u8; 1024];
    tokio::time::timeout(LIMIT, owner.recv_from(&mut buf)).await.expect("the owner gets its answer").unwrap();
    // The other socket sends: it gets no answer, and the owner keeps the
    // association.
    let v4 = pod_proxy::udp::wrap(&question(0x5151), "198.51.100.7".parse().unwrap(), 53);
    other.send_to(&v4, relay).await.unwrap();
    let got = tokio::time::timeout(Duration::from_millis(500), other.recv_from(&mut buf)).await;
    assert!(got.is_err(), "another sender gets no answer");
    let got = tokio::time::timeout(Duration::from_millis(500), owner.recv_from(&mut buf)).await;
    assert!(got.is_err(), "the datagram of the other sender went nowhere");
}

/// PodUtils/podnet issues 112 and 114: through a UDP ASSOCIATE, a datagram
/// to IPv6 on a tunnel with no IPv6 address does not stop the stack. A
/// datagram to IPv4 after it gets its answer, and a second association
/// works; a name in a datagram is resolved inside the tunnel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_udp_association_survives_a_datagram_to_ipv6() {
    pod_testkit::need_loopback!();
    let (_edge, net, _, _served) = session(false).await;
    let (proxy, _) = proxy_of(&net, Arc::new(net.clone()), Rules::default());
    for round in 0..2 {
        let (control, reply) = socks5(&proxy, 3, &by_ip([0, 0, 0, 0], 0)).await;
        assert_eq!(reply[1], 0, "round {round}: {reply:?}");
        let relay = SocketAddr::new(
            IpAddr::from([reply[4], reply[5], reply[6], reply[7]]),
            u16::from_be_bytes([reply[8], reply[9]]),
        );
        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let v6 = pod_proxy::udp::wrap(&question(0x4242), "2606:4700:4700::1111".parse().unwrap(), 53);
        udp.send_to(&v6, relay).await.unwrap();
        let v4 = pod_proxy::udp::wrap_name(&question(0x4242), "dns.example", 53).unwrap();
        udp.send_to(&v4, relay).await.unwrap();
        let mut buf = [0u8; 1024];
        let (n, _) = tokio::time::timeout(LIMIT, udp.recv_from(&mut buf)).await.expect("an answer").unwrap();
        assert_eq!(&buf[..10], &[0, 0, 0, 1, 198, 51, 100, 7, 0, 53], "round {round}: from the address of the name");
        assert_eq!(&buf[10..12], &[0x42, 0x42], "round {round}");
        assert!(n > 12);
        assert!(!net.is_closed());
        drop(control);
    }
}

/// PodUtils/podnet issue 84: the UDP association in the stream (0x83)
/// carries a datagram to the echo port of the far end and back, with no UDP
/// socket of the host.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_udp_association_in_the_stream_reaches_the_echo() {
    pod_testkit::need_loopback!();
    let (_edge, net, _, _served) = session(true).await;
    let (proxy, _) = proxy_of(&net, Arc::new(net.clone()), Rules::default());
    let (mut control, reply) = socks5(&proxy, 0x83, &by_ip([0, 0, 0, 0], 0)).await;
    assert_eq!(reply[1], 0);
    let datagram = pod_proxy::udp::wrap(b"hello", "203.0.113.7".parse().unwrap(), edge::ECHO_PORT);
    pod_proxy::udp::write_frame(&mut control, &datagram).await.unwrap();
    let back = tokio::time::timeout(LIMIT, pod_proxy::udp::read_frame(&mut control)).await.expect("the echo");
    let back = back.unwrap().unwrap();
    let (from, payload) = pod_proxy::udp::unwrap(&back).unwrap();
    assert_eq!(from.to_string(), format!("203.0.113.7:{}", edge::ECHO_PORT));
    assert_eq!(payload, b"hello");
}

/// PodUtils/podnet issue 143: each connection of a forward goes through the
/// tunnel to its target, a name resolved inside; a connection over the
/// limit is closed at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_forward_carries_each_connection_through_the_tunnel() {
    pod_testkit::need_loopback!();
    let (_edge, net, _, _served) = session(true).await;
    let rule = Rule::parse(&format!("1=tunnel:echo.test:{ECHO_TCP}")).unwrap();
    let names = Names { resolvers: Resolvers::new(vec![Resolver::Udp(PEER_DNS.parse().unwrap())]), pins: None };
    let forward = Arc::new(Forward::new(rule, Arc::new(Fixed(Arc::new(net.clone()))), names, 1));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    let _task = forward.run_tcp(listener);
    let mut client = tokio::net::TcpStream::connect(at).await.unwrap();
    client.write_all(b"ping").await.unwrap();
    let mut echo = [0u8; 4];
    tokio::time::timeout(LIMIT, client.read_exact(&mut echo)).await.expect("the echo").unwrap();
    assert_eq!(&echo, b"ping");
    let (second, _far) = tokio::io::duplex(64);
    assert!(!forward.connection(second), "the limit of 1 is held");
}

/// PodUtils/podnet issue 115: the UDP pipe of nc through a session reaches
/// the echo of the far end, and through the association in the stream of
/// the proxy too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_udp_pipe_of_nc_reaches_the_echo() {
    pod_testkit::need_loopback!();
    let (_edge, net, _, _served) = session(true).await;
    let (proxy, _) = proxy_of(&net, Arc::new(net.clone()), Rules::default());
    let (out, back) =
        nc::udp_via_net(Arc::new(net.clone()), &proxy.names, "203.0.113.7", edge::ECHO_PORT).await.unwrap();
    let mut output = Vec::new();
    nc::udp_pipe_with(out, back, &b"one"[..], &mut output, Duration::from_millis(500)).await.unwrap();
    assert_eq!(output, b"one");
    let (ours, server) = tokio::io::duplex(64 * 1024);
    proxy.client(server, None);
    let socks = pod_proxy::Proxy::parse("socks5h://127.0.0.1:1080").unwrap();
    let opened = pod_proxy::udp::open_in_stream(ours, "podnet", &socks).await.unwrap();
    let (out, back) = nc::udp_via_stream(opened, "echo.test", edge::ECHO_PORT).unwrap();
    let mut output = Vec::new();
    nc::udp_pipe_with(out, back, &b"two"[..], &mut output, Duration::from_millis(500)).await.unwrap();
    assert_eq!(output, b"two");
}
