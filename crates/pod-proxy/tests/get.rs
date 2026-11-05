//! The GET client: one request over the caller's dial and a verified TLS
//! handshake, with the user agent and the type that the caller asks for;
//! a name that TLS cannot check is refused before a connection, and a
//! failure keeps its stage: the dial's, the handshake's, the exchange's.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Stage};
use pod_proxy::resolve::System;
use pod_proxy::{Client, Dialer, ProxyChoice, Settings};
use pod_tls::{Context, TlsRoots, Trust, Words};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// The certificates of the tests of pod-tls, minted in pure Rust.
#[allow(dead_code)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

const NAME: &str = "relay.test";
const LIMIT: Duration = Duration::from_secs(10);

/// A TLS server for [`NAME`] on the loopback that answers one request with
/// `answer`, and gives the head of the request; the trust of its test CA.
async fn server(answer: &'static [u8]) -> (SocketAddr, Trust, Arc<Context>, tokio::task::JoinHandle<String>) {
    let (acceptor, roots) = cert::server_for(NAME).expect("a test server");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("an address");
    let seen = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("a connection");
        let mut tls = acceptor.accept(tcp).await.expect("a handshake");
        let mut head = Vec::new();
        let mut chunk = [0u8; 1024];
        while !head.ends_with(b"\r\n\r\n") {
            let n = tls.read(&mut chunk).await.expect("the request");
            assert!(n > 0, "the client closed before its request ended");
            head.extend_from_slice(&chunk[..n]);
        }
        tls.write_all(answer).await.expect("the answer");
        let _ = tls.shutdown().await;
        String::from_utf8(head).expect("a head of text")
    });
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podssh"))));
    let config = context
        .client_config(&TlsRoots { count: roots.len(), roots, source: "the test's own CA".into() })
        .expect("a configuration");
    (addr, Trust::caller(config).expect("no ALPN"), context, seen)
}

/// The client of podssh over a direct dial whose resolver pins [`NAME`] to
/// the loopback.
fn client(context: Arc<Context>) -> Client {
    let pins = System::with_pins(vec![(NAME.into(), IpAddr::from([127, 0, 0, 1]))]);
    let dialer = Dialer::new(Settings::new("podssh", ProxyChoice::Direct), Arc::new(pins));
    Client::new(context, Arc::new(dialer)).with_user_agent("podssh/1.2.3")
}

#[tokio::test]
async fn a_get_goes_over_the_dial_and_a_verified_handshake() {
    pod_testkit::need_loopback!();
    let (addr, trust, context, seen) =
        server(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await;
    let response =
        client(context).https_get(NAME, addr.port(), "/health", 1024, &trust, LIMIT).await.expect("an answer");
    assert_eq!((response.status, response.body.as_slice()), (200, &b"{\"ok\":true}"[..]));
    assert_eq!(response.header("content-type"), Some("application/json"));
    let head = seen.await.unwrap();
    assert!(head.starts_with("GET /health HTTP/1.1\r\n"), "{head}");
    for line in
        [format!("Host: {NAME}:{}", addr.port()), "User-Agent: podssh/1.2.3".into(), "Accept: application/json".into()]
    {
        assert!(head.contains(&format!("\r\n{line}\r\n")), "{line} is not in {head}");
    }
}

#[tokio::test]
async fn a_get_accepting_asks_for_its_type() {
    pod_testkit::need_loopback!();
    let (addr, trust, context, seen) = server(b"HTTP/1.1 204 No Content\r\n\r\n").await;
    let response = client(context)
        .https_get_accepting(NAME, addr.port(), "/dns-query?name=a", "application/dns-json", 1024, &trust, LIMIT)
        .await
        .expect("an answer");
    assert_eq!(response.status, 204);
    assert!(seen.await.unwrap().contains("\r\nAccept: application/dns-json\r\n"));
}

/// A body past the bound is the exchange's failure; a name that TLS cannot
/// check is refused before anything connects; a host that does not answer
/// TLS is the handshake's failure; a dial that fails is the dial's.
#[tokio::test]
async fn each_failure_keeps_its_stage() {
    pod_testkit::need_loopback!();
    let (addr, trust, context, _seen) = server(b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\n\r\n").await;
    let big = client(context.clone()).https_get(NAME, addr.port(), "/", 16, &trust, LIMIT).await.unwrap_err();
    assert_eq!((big.stage, big.class), (Stage::Connect, Class::Protocol), "{big}");
    assert!(big.text().starts_with("the HTTPS request failed: "), "{big}");

    let name = client(context.clone()).https_get("not a name", 443, "/", 16, &trust, LIMIT).await.unwrap_err();
    assert_eq!((name.stage, name.class), (Stage::Tls, Class::Usage), "{name}");

    let plain = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = plain.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut tcp, _) = plain.accept().await.unwrap();
        let _ = tcp.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
    });
    let tls = client(context.clone()).https_get(NAME, port, "/", 16, &trust, LIMIT).await.unwrap_err();
    assert_eq!(tls.stage, Stage::Tls, "{tls}");

    let gone = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let dial = client(context).https_get(NAME, gone, "/", 16, &trust, LIMIT).await.unwrap_err();
    assert_eq!((dial.stage, pod_proxy::Cut::of_failure(&dial)), (Stage::Connect, Some(pod_proxy::Cut::Refused)));
}
