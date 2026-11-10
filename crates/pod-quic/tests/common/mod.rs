//! The stand-in server of the tests: a QUIC endpoint on the loopback, TLS
//! 1.3 on ring with a certificate for [`NAME`] (pod-tls
//! `tests/common/cert_for_test.rs`, minted in pure Rust) and the ALPN
//! [`ALPN`]; the roots that trust it; and the TLS of a client on ring.

#![allow(dead_code)]

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use pod_quic::noq::Endpoint;
use pod_quic::noq::crypto::rustls::QuicServerConfig;

mod cert {
    include!("../../../pod-tls/tests/common/cert_for_test.rs");
}

/// The name of the stand-in's certificate.
pub const NAME: &str = "quic.example";
/// The ALPN of the stand-in.
pub const ALPN: &[u8] = b"pod-quic-test";
/// The limit of each wait of a test.
pub const LIMIT: Duration = Duration::from_secs(30);

/// The provider of the tests.
pub fn ring() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The stand-in on `ip`, its address, and the roots that trust it. It must
/// be made in a runtime.
pub fn server_on(ip: Ipv4Addr) -> (Endpoint, SocketAddr, Arc<rustls::RootCertStore>) {
    let (acceptor, roots) = cert::server_for(NAME).expect("the stand-in's certificate");
    let mut tls = (**acceptor.config()).clone();
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = QuicServerConfig::try_from(Arc::new(tls)).expect("QUIC's TLS of the stand-in");
    let server = pod_quic::endpoint::server_config(Arc::new(crypto)).expect("the stand-in's configuration");
    let socket = UdpSocket::bind((ip, 0)).unwrap();
    let addr = socket.local_addr().unwrap();
    let endpoint = pod_quic::endpoint::on_socket(socket, Some(server)).expect("the stand-in's endpoint");
    (endpoint, addr, Arc::new(roots))
}

/// The stand-in on 127.0.0.1.
pub fn server() -> (Endpoint, SocketAddr, Arc<rustls::RootCertStore>) {
    server_on(Ipv4Addr::LOCALHOST)
}

/// The TLS of a client that trusts `roots`, with the stand-in's ALPN.
pub fn tls(roots: &Arc<rustls::RootCertStore>) -> Arc<rustls::ClientConfig> {
    pod_quic::connect::tls(ring(), roots.clone(), &[ALPN]).expect("the client's TLS")
}

/// Roots that trust another CA, not the stand-in's.
pub fn other_roots() -> Arc<rustls::RootCertStore> {
    Arc::new(cert::server_for("other.example").expect("another CA").1)
}
