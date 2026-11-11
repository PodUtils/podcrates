//! Mutual TLS with pinned keys (T-165): a client with a pinned key connects;
//! a client with another key, or with no certificate, is refused at the
//! handshake; the server reports the key that the client proved; a
//! rotation pins two keys at once, then one. Each side runs on the pure
//! provider of this crate and on ring.

use std::sync::Arc;
use std::time::Duration;

use pod_tls::mutual::{Pin, Pins, client_config, peer_pin, server_config};
use pod_tls::server::{Served, certified_key};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::sign::CertifiedKey;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[allow(dead_code)]
mod cert {
    include!("common/cert_for_test.rs");
}

const TIMEOUT: Duration = Duration::from_secs(10);

/// A key of `name`: its certified key and its pin.
fn key(name: &str) -> (Arc<CertifiedKey>, Pin) {
    let made = cert::self_signed(name);
    let der = CertificateDer::from(made.der.clone());
    let signing = p256::ecdsa::SigningKey::from_slice(&made.key_der).expect("a P-256 key");
    let pin = Pin::of_certificate(&der).expect("the pin of the certificate");
    (certified_key(vec![der], signing).expect("a certified key"), pin)
}

fn providers() -> Vec<(&'static str, Arc<CryptoProvider>)> {
    vec![("pure", Arc::new(pod_tls::crypto::provider())), ("ring", Arc::new(rustls::crypto::ring::default_provider()))]
}

/// A server on loopback that requires a client pinned in `clients`. For
/// each connection it sends back the pin of the client, then closes.
async fn server(
    provider: Arc<CryptoProvider>,
    clients: Pins,
) -> (std::net::SocketAddr, Pin, tokio::task::JoinHandle<()>) {
    let (own, pin) = key("mint.test");
    let config = server_config(provider, Served::new(Some(own)), clients, &[]).expect("a server configuration");
    let acceptor = tokio_rustls::TlsAcceptor::from(config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(mut tls) = acceptor.accept(stream).await {
                    let proved = peer_pin(tls.get_ref().1.peer_certificates()).map(|p| p.to_string());
                    let _ = tls.write_all(proved.unwrap_or_default().as_bytes()).await;
                    let _ = tls.shutdown().await;
                }
            });
        }
    });
    (addr, pin, task)
}

/// Connect with `config`; the text that the server sent, or the failure.
async fn connect(addr: std::net::SocketAddr, config: Arc<rustls::ClientConfig>) -> Result<String, String> {
    let run = async {
        let tcp = tokio::net::TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
        let name = ServerName::try_from("any.name.test").unwrap();
        let mut tls = tokio_rustls::TlsConnector::from(config).connect(name, tcp).await.map_err(|e| e.to_string())?;
        let mut text = String::new();
        tls.read_to_string(&mut text).await.map_err(|e| e.to_string())?;
        if text.is_empty() { Err("the server closed with no answer".to_string()) } else { Ok(text) }
    };
    tokio::time::timeout(TIMEOUT, run).await.map_err(|_| "no answer in time".to_string())?
}

#[tokio::test]
async fn a_pinned_client_connects_and_the_server_reports_its_key() {
    pod_testkit::need_loopback!();
    for (label, provider) in providers() {
        let (client, client_pin) = key("unit-a.test");
        let (addr, server_pin, task) = server(provider.clone(), Pins::new([client_pin])).await;
        let config = client_config(provider, client, Pins::new([server_pin]), &[]).unwrap();
        let proved = connect(addr, config).await.unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(proved, client_pin.to_string(), "{label}");
        task.abort();
    }
}

#[tokio::test]
async fn a_client_with_another_key_or_no_certificate_is_refused() {
    pod_testkit::need_loopback!();
    for (label, provider) in providers() {
        let (_, pinned) = key("unit-a.test");
        let (other, _) = key("unit-b.test");
        let (addr, server_pin, task) = server(provider.clone(), Pins::new([pinned])).await;
        let config = client_config(provider.clone(), other, Pins::new([server_pin]), &[]).unwrap();
        assert!(connect(addr, config).await.is_err(), "{label}: a client of another key connected");
        // A client with no certificate: the default configuration.
        let mut roots = rustls::RootCertStore::empty();
        let _ = &mut roots;
        let plain = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyServer(provider.signature_verification_algorithms)))
            .with_no_client_auth();
        assert!(connect(addr, Arc::new(plain)).await.is_err(), "{label}: a client with no certificate connected");
        task.abort();
    }
}

#[tokio::test]
async fn a_client_refuses_a_server_whose_key_it_does_not_pin() {
    pod_testkit::need_loopback!();
    let provider: Arc<CryptoProvider> = Arc::new(pod_tls::crypto::provider());
    let (client, client_pin) = key("unit-a.test");
    let (addr, _server_pin, task) = server(provider.clone(), Pins::new([client_pin])).await;
    let (_, stranger) = key("stranger.test");
    let config = client_config(provider, client, Pins::new([stranger]), &[]).unwrap();
    assert!(connect(addr, config).await.is_err(), "a client took a server of another key");
    task.abort();
}

#[tokio::test]
async fn a_rotation_pins_two_keys_then_one() {
    pod_testkit::need_loopback!();
    let provider: Arc<CryptoProvider> = Arc::new(pod_tls::crypto::provider());
    let (old, old_pin) = key("unit-old.test");
    let (new, new_pin) = key("unit-new.test");
    let clients = Pins::new([old_pin, new_pin]);
    let (addr, server_pin, task) = server(provider.clone(), clients.clone()).await;
    let old_config = client_config(provider.clone(), old, Pins::new([server_pin]), &[]).unwrap();
    let new_config = client_config(provider, new, Pins::new([server_pin]), &[]).unwrap();
    assert_eq!(connect(addr, old_config.clone()).await, Ok(old_pin.to_string()));
    assert_eq!(connect(addr, new_config.clone()).await, Ok(new_pin.to_string()));
    // The server that runs takes the new set at the next handshake.
    clients.set([new_pin]);
    assert!(connect(addr, old_config).await.is_err(), "the old key connected after the rotation");
    assert_eq!(connect(addr, new_config).await, Ok(new_pin.to_string()));
    task.abort();
}

#[test]
fn a_pin_is_written_and_read_as_sha256_and_base64() {
    let (_, pin) = key("unit-a.test");
    let text = pin.to_string();
    assert!(text.starts_with("sha256/") && text.len() == 7 + 44, "{text}");
    assert_eq!(Pin::parse(&text).unwrap(), pin);
    assert!(Pin::parse("sha1/AAAA").is_err());
    assert!(Pin::parse("sha256/AAAA").is_err());
    assert!(peer_pin(None).is_none());
}

/// A client verifier that takes any server, for the client with no
/// certificate: the refusal must come from the server.
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
