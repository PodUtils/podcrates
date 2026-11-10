//! The blocking face (feature `blocking`): with no runtime of the caller, a
//! direct dial gives the standard `TcpStream`, a dial through a CONNECT
//! proxy and through a proxy at a command gives a stream that carries bytes
//! both ways, and the client gets an answer over a verified handshake.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use pod_proxy::blocking::{Client, Dialer, Stream};
use pod_proxy::resolve::System;
use pod_proxy::{Proxy, ProxyChoice, Settings};
use pod_tls::{Context, TlsRoots, Trust, Words};

// The certificates of the tests of pod-tls, minted in pure Rust.
#[allow(dead_code)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

const LIMIT: Duration = Duration::from_secs(10);

fn dialer(proxy: ProxyChoice) -> Dialer {
    Dialer::new(Settings::new("podvm", proxy), Arc::new(System::new())).expect("a runtime")
}

/// A server on the loopback that gives back each byte of one connection.
fn echo() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("its address");
    thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("a connection");
        let mut far = sock.try_clone().expect("a clone");
        let _ = std::io::copy(&mut sock, &mut far);
    });
    addr
}

/// Write `ping`, read it back.
fn round_trip(stream: &mut impl ReadWrite) {
    stream.write_all(b"ping").expect("the write");
    stream.flush().expect("the flush");
    let mut got = [0u8; 4];
    stream.read_exact(&mut got).expect("the bytes back");
    assert_eq!(&got, b"ping");
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

#[test]
fn a_direct_dial_gives_the_standard_tcp_stream() {
    pod_testkit::need_loopback!();
    let addr = echo();
    let mut tcp = dialer(ProxyChoice::Direct).tcp("127.0.0.1", addr.port(), LIMIT).expect("a connection");
    tcp.set_read_timeout(Some(pod_testkit::host::scaled(LIMIT))).expect("a blocking socket takes a read limit");
    assert_eq!(tcp.peer_addr().expect("its peer"), addr);
    round_trip(&mut tcp);
}

/// A stand-in HTTP proxy that answers 200 to one CONNECT and then gives back
/// each byte: the head that it got.
fn connect_proxy() -> (Proxy, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("its address").port();
    let seen = thread::spawn(move || {
        let (sock, _) = listener.accept().expect("a connection");
        let mut reader = BufReader::new(sock.try_clone().expect("a clone"));
        let mut head = String::new();
        while !head.ends_with("\r\n\r\n") && reader.read_line(&mut head).expect("the head") > 0 {}
        let mut sock = sock;
        sock.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").expect("the answer");
        let mut far = sock.try_clone().expect("a clone");
        let _ = std::io::copy(&mut reader, &mut far);
        head
    });
    (Proxy::parse(&format!("http://127.0.0.1:{port}")).expect("a proxy"), seen)
}

#[test]
fn a_dial_through_a_connect_proxy_carries_bytes_both_ways() {
    pod_testkit::need_loopback!();
    let (proxy, seen) = connect_proxy();
    let (mut stream, _) = dialer(ProxyChoice::Via(proxy)).stream("relay.invalid", 443, LIMIT).expect("a tunnel");
    assert!(matches!(stream, Stream::Tcp(_)), "{stream:?}");
    stream.set_timeouts(Some(LIMIT)).expect("the limits");
    round_trip(&mut stream);
    drop(stream);
    let head = seen.join().expect("the proxy");
    assert!(head.starts_with("CONNECT relay.invalid:443 HTTP/1.1\r\n"), "{head}");
}

/// The command reads the CONNECT head, answers 200, and then is `cat`.
#[cfg(unix)]
#[test]
fn a_dial_through_a_proxy_at_a_command_carries_bytes_both_ways() {
    let command = r#"while IFS= read -r l && [ "$l" != "$(printf '\r')" ]; do :; done; printf 'HTTP/1.1 200 OK\r\n\r\n'; exec cat"#;
    let proxy = Proxy::parse_trusted(&format!("http+exec:{command}")).expect("a proxy at a command");
    let (mut stream, _) = dialer(ProxyChoice::Via(proxy)).stream("relay.invalid", 443, LIMIT).expect("a tunnel");
    assert!(matches!(stream, Stream::Exec(_)), "{stream:?}");
    round_trip(&mut stream);
}

const NAME: &str = "relay.test";

#[test]
fn the_client_gets_an_answer_over_a_verified_handshake() {
    pod_testkit::need_loopback!();
    let (acceptor, roots) = cert::server_for(NAME).expect("a test server");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("its address");
    // The server runs on its own runtime, on its own thread: the test's
    // thread has none, as a synchronous consumer.
    let served = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
        runtime.block_on(async move {
            use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
            listener.set_nonblocking(true).expect("nonblocking");
            let listener = tokio::net::TcpListener::from_std(listener).expect("a tokio listener");
            let (tcp, _) = listener.accept().await.expect("a connection");
            let mut tls = acceptor.accept(tcp).await.expect("a handshake");
            let mut head = Vec::new();
            let mut chunk = [0u8; 1024];
            while !head.ends_with(b"\r\n\r\n") {
                let n = tls.read(&mut chunk).await.expect("the request");
                assert!(n > 0, "the client closed before its request ended");
                head.extend_from_slice(&chunk[..n]);
            }
            tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await.expect("the answer");
            let _ = tls.shutdown().await;
            String::from_utf8(head).expect("a head of text")
        })
    });
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podvm"))));
    let config = context
        .client_config(&TlsRoots { count: roots.len(), roots, source: "the test's own CA".into() })
        .expect("a configuration");
    let trust = Trust::caller(config).expect("no ALPN");
    let pins = System::with_pins(vec![(NAME.into(), IpAddr::from([127, 0, 0, 1]))]);
    let dialer = Dialer::new(Settings::new("podvm", ProxyChoice::Direct), Arc::new(pins)).expect("a runtime");
    let client = Client::new(context, &dialer).with_user_agent("podvm/1.2.3");
    let response = client.https_get(NAME, addr.port(), "/health", 1024, &trust, LIMIT).expect("an answer");
    assert_eq!((response.status, response.body.as_slice()), (200, &b"ok"[..]));
    let head = served.join().expect("the server");
    assert!(head.starts_with("GET /health HTTP/1.1\r\n"), "{head}");
    assert!(head.contains("\r\nUser-Agent: podvm/1.2.3\r\n"), "{head}");
}

/// A resolver whose lookup blocks a thread of the runtime for 30 s, as a
/// system lookup that gets no answer does.
struct Stuck;

impl pod_core::Resolve for Stuck {
    fn label(&self) -> String {
        "stuck".into()
    }

    fn resolve_from<'a>(
        &'a self,
        _host: &'a str,
        _port: u16,
        _deadline: tokio::time::Instant,
        _after: Option<pod_core::Source>,
    ) -> pod_core::BoxFuture<'a, Result<pod_core::Found, pod_core::Failure>> {
        Box::pin(async {
            let _ = tokio::task::spawn_blocking(|| thread::sleep(Duration::from_secs(30))).await;
            Err(pod_core::Failure::new(pod_core::Stage::Dns, pod_core::Class::Timeout, "stuck"))
        })
    }
}

/// The time limit of the dial bounds a resolver that does not keep to it, and
/// the lookup that it leaves on a thread of the runtime does not hold the
/// drop of the dialer.
#[test]
fn the_drop_of_a_dialer_does_not_wait_for_a_lookup_that_the_dial_left() {
    let dialer = Dialer::new(Settings::new("podvm", ProxyChoice::Direct), Arc::new(Stuck)).expect("a runtime");
    let dialed = std::time::Instant::now();
    let failed = dialer.tcp("relay.invalid", 443, Duration::from_millis(200));
    assert!(failed.is_err(), "the lookup never answers");
    assert!(
        dialed.elapsed() < pod_testkit::host::scaled(Duration::from_secs(5)),
        "the dial waited {:?}",
        dialed.elapsed()
    );
    let start = std::time::Instant::now();
    drop(dialer);
    assert!(
        start.elapsed() < pod_testkit::host::scaled(Duration::from_secs(5)),
        "the drop waited {:?}",
        start.elapsed()
    );
}

/// The command answers the CONNECT and then sends nothing: a read with a
/// limit ends at the limit with the kind of a socket's limit.
#[cfg(unix)]
#[test]
fn a_read_from_a_proxy_at_a_command_ends_at_its_limit() {
    let command = r#"while IFS= read -r l && [ "$l" != "$(printf '\r')" ]; do :; done; printf 'HTTP/1.1 200 OK\r\n\r\n'; exec sleep 30"#;
    let proxy = Proxy::parse_trusted(&format!("http+exec:{command}")).expect("a proxy at a command");
    let (mut stream, _) = dialer(ProxyChoice::Via(proxy)).stream("relay.invalid", 443, LIMIT).expect("a tunnel");
    stream.set_timeouts(Some(Duration::from_millis(300))).expect("the limit");
    let start = std::time::Instant::now();
    let mut byte = [0u8; 1];
    let failed = stream.read(&mut byte).expect_err("the command sends nothing");
    assert_eq!(failed.kind(), std::io::ErrorKind::WouldBlock, "{failed}");
    assert!(
        start.elapsed() < pod_testkit::host::scaled(Duration::from_secs(5)),
        "the read waited {:?}",
        start.elapsed()
    );
}
