//! A session that comes back, its TCP probe and its exit (podnet's
//! `src/warp/live.rs` and `src/warp/exit.rs`, T-339) against the stand-in
//! API and the stand-in edge on the loopback.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use common::edge::{self, Edge, Route};
use pod_core::{Dial, Target};
use pod_testkit::stand;
use pod_warp::live::{Closable, Live, LiveSettings};
use pod_warp::proxy::Tunnel;
use pod_warp::{Warp, WarpNet};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const LIMIT: Duration = Duration::from_secs(10);
/// The port of the echo service inside the tunnel.
const ECHO_TCP: u16 = 80;
/// The body of the stand-in trace.
const TRACE: &str = "fl=1\nh=one.one.one.one\nip=104.28.0.9\ncolo=AMS\nloc=NL\nwarp=on\n";

/// The stand-in API, the edge with an echo route and a trace on port 443,
/// a registered device, and the TLS client that pins the key of the trace.
async fn world() -> (Edge, Arc<Warp>, Arc<rustls::ClientConfig>, stand::Stand) {
    let echo = edge::echo_server().await;
    let (trace, tls) = trace_server().await;
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    let routes = vec![Route { port: ECHO_TCP, outside: echo }, Route { port: 443, outside: trace }];
    let edge = Edge::start(api.clone(), routes).await;
    api.serve_key(&edge.pem());
    let served = stand::http(api.handler()).unwrap();
    let moved = common::Moved::new(vec![(edge::door_v4(), edge.addr), (edge::door_v6(), edge.addr)]);
    let (settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    let warp = Warp::register(settings).await.unwrap();
    (edge, Arc::new(warp), tls, served)
}

/// An HTTPS server of the trace on the loopback with a key of its own, and
/// the TLS client that pins that key.
async fn trace_server() -> (SocketAddr, Arc<rustls::ClientConfig>) {
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
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && tls.read_exact(&mut byte).await.is_ok() {
                    head.push(byte[0]);
                }
                let text = String::from_utf8_lossy(&head);
                let body = match text.starts_with("GET /cdn-cgi/trace HTTP/1.0\r\n") {
                    true if text.contains("\r\nHost: one.one.one.one\r\n") => TRACE,
                    _ => "not the trace\n",
                };
                let answer = format!("HTTP/1.0 200 OK\r\ncontent-type: text/plain\r\n\r\n{body}");
                let _ = tls.write_all(answer.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    let identity = pod_warp::Identity::generate().unwrap();
    let client = pod_warp::tls::client_config(common::provider(), &identity, key.spki(), b"h2").unwrap();
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

async fn echo_through(net: &dyn Dial) {
    let target = Target::Addr("203.0.113.9:80".parse().unwrap());
    let mut connected = net.dial(&target, tokio::time::Instant::now() + LIMIT).await.unwrap();
    connected.io.write_all(b"live").await.unwrap();
    let mut got = [0u8; 4];
    tokio::time::timeout(LIMIT, connected.io.read_exact(&mut got)).await.expect("an echo").unwrap();
    assert_eq!(&got, b"live");
}

/// PodUtils/podnet issue 75: the edge ends the session; `Live` opens a new
/// one, and the proxy's tunnel gives it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_session_comes_back_after_the_edge_ends_it() {
    pod_testkit::need_loopback!();
    let (edge, warp, _, _served) = world().await;
    let settings = LiveSettings { base: Duration::from_millis(50), probe: None, ..LiveSettings::default() };
    let live: Arc<Live<WarpNet>> = warp.connect_live(&settings).await.unwrap();
    let first = live.get().await.unwrap();
    echo_through(first.as_ref()).await;
    // A new session changes the report of the data path.
    let mut changes = live.report();
    edge.end_sessions();
    tokio::time::timeout(LIMIT, changes.changed()).await.expect("a new session").unwrap();
    let tunnel: Arc<dyn Tunnel> = live.clone();
    let next = tokio::time::timeout(LIMIT, tunnel.session()).await.expect("a new session").unwrap();
    echo_through(next.as_ref()).await;
    assert!(first.is_closed());
    assert_eq!(live.reconnects(), 1);
    assert_eq!(edge.tunnels(), 2);
}

/// PodUtils/podnet@T-068: the TCP probe of a session reaches its target
/// through the tunnel, and fails on a closed session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tcp_probe_of_a_session_reaches_its_target() {
    pod_testkit::need_loopback!();
    let (_edge, warp, _, _served) = world().await;
    let net = warp.connect().await.unwrap();
    let target: SocketAddr = "203.0.113.9:80".parse().unwrap();
    assert!(Closable::probe_tcp(&net, target, LIMIT).await);
    Closable::close(&net, "the test closes it");
    assert_eq!(Closable::reason(&net).as_deref(), Some("the test closes it"));
    assert!(!Closable::probe_tcp(&net, target, LIMIT).await);
}

/// PodUtils/podnet issue 147: the exit of a session is the trace of
/// 1.1.1.1 over HTTPS through the tunnel, with the name of the trace.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_of_a_session_is_read_from_the_trace() {
    pod_testkit::need_loopback!();
    let (_edge, warp, tls, _served) = world().await;
    let net = warp.connect().await.unwrap();
    let exit = pod_warp::exit::trace(&net, &tls).await.unwrap();
    assert_eq!(exit.colo, "AMS");
    assert_eq!(exit.loc.as_deref(), Some("NL"));
    assert_eq!(exit.ip.as_deref(), Some("104.28.0.9"));
    assert_eq!(exit.warp.as_deref(), Some("on"));
}
