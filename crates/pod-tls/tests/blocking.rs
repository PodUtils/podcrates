//! The blocking handshake: rustls's own synchronous stream over a standard
//! `TcpStream`, with no runtime on either side. A verified handshake carries
//! bytes both ways; a certificate for another name is refused with the words
//! of a failure, at the stage of TLS.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use pod_core::Stage;
use pod_tls::{Context, Settings, TlsRoots, Trust, Words};

// Shared test support; this file uses part of it.
#[allow(dead_code)]
mod cert {
    include!("common/cert_for_test.rs");
}

/// A server on the loopback, on a thread with no runtime, that presents a
/// certificate for `name` and gives back what one connection sends; the
/// trust of its test CA in a context of `podvm`.
fn server(name: &str) -> (u16, Arc<Context>, Trust) {
    let (acceptor, roots) = cert::server_for(name).expect("a test server");
    let config = acceptor.config().clone();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("its address").port();
    thread::spawn(move || {
        let (tcp, _) = listener.accept().expect("a connection");
        tcp.set_read_timeout(Some(pod_testkit::host::scaled(Duration::from_secs(10)))).expect("a limit");
        let tls = rustls::ServerConnection::new(config).expect("a server connection");
        let mut stream = rustls::StreamOwned::new(tls, tcp);
        let mut got = [0u8; 4];
        if stream.read_exact(&mut got).is_ok() {
            let _ = stream.write_all(&got);
            let _ = stream.flush();
        }
    });
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(Context::new(provider, Settings::new(Words::new("podvm"))));
    let config = context
        .client_config(&TlsRoots { count: roots.len(), roots, source: "the test's own CA".into() })
        .expect("a configuration");
    (port, context, Trust::caller(config).expect("no ALPN"))
}

fn tcp(port: u16) -> TcpStream {
    let tcp = TcpStream::connect(("127.0.0.1", port)).expect("a connection");
    tcp.set_read_timeout(Some(pod_testkit::host::scaled(Duration::from_secs(10)))).expect("a limit");
    tcp
}

#[test]
fn a_blocking_handshake_carries_bytes_both_ways() {
    pod_testkit::need_loopback!();
    let (port, context, trust) = server("relay.test");
    let mut tls = context.tls_over_blocking(tcp(port), "relay.test", &trust).expect("a verified handshake");
    tls.write_all(b"ping").expect("the write");
    tls.flush().expect("the flush");
    let mut got = [0u8; 4];
    tls.read_exact(&mut got).expect("the bytes back");
    assert_eq!(&got, b"ping");
}

#[test]
fn a_blocking_handshake_refuses_a_certificate_for_another_name() {
    pod_testkit::need_loopback!();
    let (port, context, trust) = server("wrong.example");
    let failure = match context.tls_over_blocking(tcp(port), "relay.test", &trust) {
        Ok(_) => panic!("a certificate for another name was taken"),
        Err(failure) => failure,
    };
    assert_eq!(failure.stage, Stage::Tls, "{failure}");
    assert!(failure.to_string().contains("not valid for name"), "{failure}");
}
