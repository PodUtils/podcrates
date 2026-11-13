//! The refusals of the tcp carrier (T-396): a listener of another key is
//! refused by the dialer; a client of another key is refused by the
//! listener, which stays open; each refusal names the key that the peer
//! proved; a peer of TLS 1.2 is refused on each side.

#![cfg(feature = "tcp")]

mod common;

use std::sync::Arc;

use common::limit;
use common::tcp::{Direct, at, bound, carrier, id, ring, rustls_error};
use pod_link::tcp::{ALPN, Refusal, refusal};
use pod_link::{Carrier, CarrierErrorKind};
use pod_tls::server::Served;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::sign::CertifiedKey;
use tokio::time::timeout;

#[tokio::test]
async fn the_dialer_refuses_a_listener_of_another_key_and_names_it() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(3));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let dialing = carrier(2, Arc::new(Direct::default()), &[]);
    let door = bound(&listening).await;
    let e = dialing.dial(&at(&stranger.public(), &door)).await.err().expect("a dial took a listener of another key");
    assert_eq!(e.kind, CarrierErrorKind::Refused, "{e}");
    assert_eq!(e.class().exit_code(), 77);
    assert_eq!(refusal(&e), Some(Refusal::Unpinned(server.public())), "{e}");
    assert!(e.to_string().contains(&server.public().fingerprint()), "the error names the key seen: {e}");
}

#[tokio::test]
async fn the_listener_refuses_a_client_of_another_key_and_stays_open() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(4));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);

    let other = carrier(4, Arc::new(Direct::default()), &[]);
    let (near, far) = tokio::join!(other.dial(&address), door.accept_key());
    let near = near.err().expect("a client of another key dialed");
    assert_eq!(near.kind, CarrierErrorKind::Refused, "the dialer learns of the refusal at the dial: {near}");
    assert!(near.to_string().contains(&stranger.public().fingerprint()), "{near}");
    let far = far.err().expect("the listener took a client of another key");
    assert_eq!(far.kind, CarrierErrorKind::Refused, "{far}");
    assert_eq!(refusal(&far), Some(Refusal::Unpinned(stranger.public())), "{far}");
    assert!(far.to_string().contains(&stranger.public().fingerprint()), "the error names the key seen: {far}");

    let good = carrier(2, Arc::new(Direct::default()), &[]);
    let (near, far) = tokio::join!(good.dial(&address), door.accept_key());
    near.expect("the pinned client after a refusal");
    assert_eq!(far.expect("the listener stays open").1, client.public());
}

/// A client that presents `key` at each request of the server.
#[derive(Debug)]
struct Presents(Arc<CertifiedKey>);

impl rustls::client::ResolvesClientCert for Presents {
    fn resolve(&self, _: &[&[u8]], _: &[rustls::SignatureScheme]) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }

    fn has_certs(&self) -> bool {
        true
    }
}

/// A client verifier that takes any server: the refusal must come from
/// the listener.
#[derive(Debug)]
struct AnyServer(rustls::crypto::WebPkiSupportedAlgorithms);

impl rustls::client::danger::ServerCertVerifier for AnyServer {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.supported_schemes()
    }
}

#[tokio::test]
async fn a_client_of_tls_1_2_is_refused_by_the_listener() {
    pod_testkit::need_loopback!();
    let client = id(2);
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let mut door = bound(&listening).await;
    // A pinned client key and the ALPN of the link: only the version is
    // wrong.
    let provider = ring();
    let mut config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyServer(provider.signature_verification_algorithms)))
        .with_client_cert_resolver(Arc::new(Presents(pod_tls::peer::certified_key(client))));
    config.alpn_protocols = vec![ALPN.to_vec()];
    let local = door.local_addr();
    let old = async {
        let tcp = tokio::net::TcpStream::connect(local).await.unwrap();
        let name = ServerName::IpAddress(local.ip().into());
        tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await
    };
    let (near, far) = tokio::join!(old, door.accept_key());
    assert!(near.is_err(), "a client of TLS 1.2 shook hands");
    let far = far.err().expect("the listener took a client of TLS 1.2");
    let cause = rustls_error(&far);
    assert!(matches!(cause, Some(rustls::Error::PeerIncompatible(_))), "{far}: {cause:?}");
}

#[tokio::test]
async fn a_listener_of_tls_1_2_is_refused_by_the_dialer() {
    pod_testkit::need_loopback!();
    let server = id(1);
    // The certificate of the pinned key on a server of TLS 1.2 alone.
    let mut config = rustls::ServerConfig::builder_with_provider(ring())
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Served::new(Some(pod_tls::peer::certified_key(server.clone()))));
    config.alpn_protocols = vec![ALPN.to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = listener.local_addr().unwrap();
    let serve = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        acceptor.accept(tcp).await.map(drop)
    });
    let dialing = carrier(2, Arc::new(Direct::default()), &[]);
    let e = dialing.dial(&format!("{}@{local}", server.public().hex())).await.err().expect("a dial took TLS 1.2");
    let cause = rustls_error(&e);
    assert_eq!(cause, Some(rustls::Error::AlertReceived(rustls::AlertDescription::ProtocolVersion)), "{e}");
    assert!(timeout(limit(), serve).await.unwrap().unwrap().is_err(), "the server of TLS 1.2 shook hands");
}
