//! TLS between peers that pin each other's Ed25519 key (T-304): the pinned
//! key passes on each side and each side learns the key of the other; a
//! key that is not pinned fails the handshake with a `Refusal` that names
//! it; a certificate of another form passes when its key is pinned; a
//! certificate of no Ed25519 key, a certificate signed by another key and a
//! client with no certificate are refused. Each test runs on the pure
//! provider of this crate and on ring.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer::{Keys, Refusal, certificate, client_config, peer_key, refusal, server_config, signing_key};
use pod_tls::server::Served;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::sign::CertifiedKey;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

#[allow(dead_code)]
mod cert {
    include!("common/cert_for_test.rs");
}

const TIMEOUT: Duration = Duration::from_secs(10);
const ALPN: &[u8] = b"test/1";

/// A made-up identity: the seed is `n` in each byte.
fn id(n: u8) -> Arc<Identity> {
    Arc::new(Identity::from_seed(&[n; 32]))
}

fn named(key: PublicKey) -> KeyName {
    KeyName::Key(key)
}

fn providers() -> Vec<(&'static str, Arc<CryptoProvider>)> {
    vec![("pure", Arc::new(pod_tls::crypto::provider())), ("ring", Arc::new(rustls::crypto::ring::default_provider()))]
}

/// A server on loopback with `config`. For each connection it sends the
/// fingerprint of the client key, then closes; the result of each
/// handshake goes to the channel.
async fn serve(
    config: Arc<rustls::ServerConfig>,
) -> (SocketAddr, mpsc::UnboundedReceiver<Result<Option<PublicKey>, io::Error>>, tokio::task::JoinHandle<()>) {
    let acceptor = tokio_rustls::TlsAcceptor::from(config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (acceptor, tx) = (acceptor.clone(), tx.clone());
            tokio::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(mut tls) => {
                        let client = peer_key(tls.get_ref().1.peer_certificates());
                        let _ = tx.send(Ok(client));
                        let text = client.map(|k| k.fingerprint()).unwrap_or_default();
                        let _ = tls.write_all(text.as_bytes()).await;
                        let _ = tls.shutdown().await;
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                    }
                }
            });
        }
    });
    (addr, rx, task)
}

/// Connect with `config`: the key of the server and the text that it sent,
/// or the failure.
async fn connect(
    addr: SocketAddr,
    config: Arc<rustls::ClientConfig>,
) -> Result<(Option<PublicKey>, String), io::Error> {
    let run = async {
        let tcp = tokio::net::TcpStream::connect(addr).await?;
        let name = ServerName::try_from("any.name.test").unwrap();
        let mut tls = tokio_rustls::TlsConnector::from(config).connect(name, tcp).await?;
        let server = peer_key(tls.get_ref().1.peer_certificates());
        let mut text = String::new();
        tls.read_to_string(&mut text).await?;
        Ok((server, text))
    };
    tokio::time::timeout(TIMEOUT, run).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "no answer"))?
}

/// The result of the next handshake of the server.
async fn next(
    rx: &mut mpsc::UnboundedReceiver<Result<Option<PublicKey>, io::Error>>,
) -> Result<Option<PublicKey>, io::Error> {
    tokio::time::timeout(TIMEOUT, rx.recv()).await.expect("a handshake of the server").expect("the server runs")
}

/// A server of TLS 1.3 that presents `certified` and asks no client
/// certificate: for the checks of the client.
fn plain_server(provider: Arc<CryptoProvider>, certified: Arc<CertifiedKey>) -> Arc<rustls::ServerConfig> {
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Served::new(Some(certified)));
    config.alpn_protocols = vec![ALPN.to_vec()];
    Arc::new(config)
}

#[tokio::test]
async fn a_pinned_key_passes_on_each_side_and_each_side_learns_the_other() {
    pod_testkit::need_loopback!();
    for (label, provider) in providers() {
        let (server, client) = (id(1), id(2));
        let config = server_config(provider.clone(), server.clone(), Keys::new([named(client.public())]), &[ALPN]);
        let (addr, mut rx, task) = serve(config.unwrap()).await;
        let config = client_config(provider, client.clone(), named(server.public()), &[ALPN]).unwrap();
        let (seen, text) = connect(addr, config).await.unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(seen, Some(server.public()), "{label}: the client learns the server key");
        assert_eq!(text, client.public().fingerprint(), "{label}");
        assert_eq!(next(&mut rx).await.unwrap(), Some(client.public()), "{label}: the server learns the client key");
        task.abort();
    }
}

#[tokio::test]
async fn the_client_refuses_a_server_of_another_key_and_names_it() {
    pod_testkit::need_loopback!();
    for (label, provider) in providers() {
        let (server, client, stranger) = (id(1), id(2), id(3));
        let config = server_config(provider.clone(), server.clone(), Keys::new([named(client.public())]), &[ALPN]);
        let (addr, _rx, task) = serve(config.unwrap()).await;
        let config = client_config(provider, client, named(stranger.public()), &[ALPN]).unwrap();
        let e = connect(addr, config).await.expect_err("a client took a server of another key");
        let found = refusal(&e).unwrap_or_else(|| panic!("{label}: no refusal in {e}"));
        assert_eq!(found, Refusal::Unpinned(server.public()), "{label}");
        assert!(found.to_string().contains(&server.public().fingerprint()), "{label}: {found}");
        assert!(found.to_string().contains(&server.public().display_fingerprint()), "{label}: {found}");
        task.abort();
    }
}

#[tokio::test]
async fn the_server_refuses_a_client_of_another_key_and_names_it() {
    pod_testkit::need_loopback!();
    for (label, provider) in providers() {
        let (server, client, stranger) = (id(1), id(2), id(3));
        let config = server_config(provider.clone(), server.clone(), Keys::new([named(client.public())]), &[ALPN]);
        let (addr, mut rx, task) = serve(config.unwrap()).await;
        let config = client_config(provider, stranger.clone(), named(server.public()), &[ALPN]).unwrap();
        assert!(connect(addr, config).await.is_err(), "{label}: a client of another key read an answer");
        let e = next(&mut rx).await.expect_err("the server took a client of another key");
        assert_eq!(refusal(&e), Some(Refusal::Unpinned(stranger.public())), "{label}: {e}");
        task.abort();
    }
}

#[tokio::test]
async fn a_client_with_no_certificate_is_refused() {
    pod_testkit::need_loopback!();
    let provider: Arc<CryptoProvider> = Arc::new(pod_tls::crypto::provider());
    let (server, client) = (id(1), id(2));
    let config = server_config(provider.clone(), server.clone(), Keys::new([named(client.public())]), &[ALPN]);
    let (addr, mut rx, task) = serve(config.unwrap()).await;
    let mut plain = (*client_config(provider, client, named(server.public()), &[ALPN]).unwrap()).clone();
    plain.client_auth_cert_resolver = Arc::new(NoCertificate);
    assert!(connect(addr, Arc::new(plain)).await.is_err(), "a client with no certificate read an answer");
    let e = next(&mut rx).await.expect_err("the server took a client with no certificate");
    assert_eq!(refusal(&e), None, "{e}");
    task.abort();
}

/// A client that presents no certificate.
#[derive(Debug)]
struct NoCertificate;

impl rustls::client::ResolvesClientCert for NoCertificate {
    fn resolve(&self, _: &[&[u8]], _: &[rustls::SignatureScheme]) -> Option<Arc<CertifiedKey>> {
        None
    }

    fn has_certs(&self) -> bool {
        false
    }
}

/// A certificate of `identity` in another form than the one of this
/// crate: another subject, another serial, a validity of ten years and a
/// critical extension (basicConstraints, not a CA).
fn other_form(identity: &Identity) -> CertificateDer<'static> {
    fn der(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        if body.len() < 0x80 {
            out.push(body.len() as u8);
        } else {
            out.extend_from_slice(&[0x81, body.len() as u8]);
        }
        out.extend_from_slice(body);
        out
    }
    let algorithm = der(0x30, &der(0x06, &[0x2b, 0x65, 0x70]));
    let name =
        der(0x30, &der(0x31, &der(0x30, &[der(0x06, &[0x55, 0x04, 0x03]), der(0x0c, b"another.form")].concat())));
    let validity = der(0x30, &[der(0x17, b"200101000000Z"), der(0x17, b"300101000000Z")].concat());
    let spki =
        [&[0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00][..], &identity.public().0[..]]
            .concat();
    let basic = der(0x30, &[der(0x06, &[0x55, 0x1d, 0x13]), der(0x01, &[0xff]), der(0x04, &der(0x30, &[]))].concat());
    let tbs = der(
        0x30,
        &[
            der(0xa0, &der(0x02, &[2])),
            der(0x02, &[0x05, 0x39]),
            algorithm.clone(),
            name.clone(),
            validity,
            name,
            spki,
            der(0xa3, &der(0x30, &basic)),
        ]
        .concat(),
    );
    let mut bits = vec![0u8];
    bits.extend_from_slice(&identity.sign(&tbs));
    CertificateDer::from(der(0x30, &[tbs, algorithm, der(0x03, &bits)].concat()))
}

#[tokio::test]
async fn a_certificate_of_another_form_passes_when_its_key_is_pinned() {
    pod_testkit::need_loopback!();
    for (label, provider) in providers() {
        let (server, client) = (id(5), id(6));
        let made = other_form(&server);
        assert_ne!(made, certificate(&server));
        let certified = Arc::new(CertifiedKey::new(vec![made], signing_key(server.clone())));
        let (addr, _rx, task) = serve(plain_server(provider.clone(), certified)).await;
        let config = client_config(provider, client, named(server.public()), &[ALPN]).unwrap();
        let (seen, _) = connect(addr, config).await.unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(seen, Some(server.public()), "{label}");
        task.abort();
    }
}

#[tokio::test]
async fn a_certificate_with_no_ed25519_key_is_refused() {
    pod_testkit::need_loopback!();
    for (label, provider) in providers() {
        let made = cert::self_signed("p256.test");
        let der = CertificateDer::from(made.der.clone());
        // A server of a P-256 key cannot sign with Ed25519, the one scheme
        // that the client offers.
        let signing = p256::ecdsa::SigningKey::from_slice(&made.key_der).expect("a P-256 key");
        let certified = pod_tls::server::certified_key(vec![der.clone()], signing).unwrap();
        let (addr, _rx, task) = serve(plain_server(provider.clone(), certified)).await;
        let config = client_config(provider.clone(), id(2), named(id(1).public()), &[ALPN]).unwrap();
        assert!(connect(addr, config.clone()).await.is_err(), "{label}: a client took a P-256 server");
        task.abort();
        // A certificate of a P-256 key with an Ed25519 signer: the check of
        // the certificate refuses it by name.
        let certified = Arc::new(CertifiedKey::new(vec![der], signing_key(id(1))));
        let (addr, _rx, task) = serve(plain_server(provider, certified)).await;
        let e = connect(addr, config).await.expect_err("a client took a certificate of no Ed25519 key");
        assert_eq!(refusal(&e), Some(Refusal::NotEd25519), "{label}: {e}");
        task.abort();
    }
}

#[tokio::test]
async fn a_certificate_of_the_pinned_key_signed_by_another_key_is_refused() {
    pod_testkit::need_loopback!();
    for (label, provider) in providers() {
        // The certificate of the pinned key, and the handshake signed by a
        // key that is not its own: the peer does not hold the pinned key.
        let (pinned, thief) = (id(7), id(8));
        let certified = Arc::new(CertifiedKey::new(vec![certificate(&pinned)], signing_key(thief)));
        let (addr, _rx, task) = serve(plain_server(provider.clone(), certified)).await;
        let config = client_config(provider, id(2), named(pinned.public()), &[ALPN]).unwrap();
        let e = connect(addr, config).await.expect_err("a client took a server that does not hold the pinned key");
        assert_eq!(refusal(&e), None, "{label}: the key is pinned; the signature is wrong: {e}");
        task.abort();
    }
}

#[tokio::test]
async fn a_key_is_named_by_its_fingerprint_or_its_display_form_and_a_rotation_takes_effect() {
    pod_testkit::need_loopback!();
    let provider: Arc<CryptoProvider> = Arc::new(pod_tls::crypto::provider());
    let (server, old, new) = (id(1), id(10), id(11));
    let by_display = KeyName::parse(&old.public().display_fingerprint()).expect("the display form");
    let by_fingerprint = KeyName::parse(&new.public().fingerprint()).expect("the fingerprint");
    let clients = Keys::new([by_display, by_fingerprint.clone()]);
    let config = server_config(provider.clone(), server.clone(), clients.clone(), &[ALPN]).unwrap();
    let (addr, mut rx, task) = serve(config).await;
    let to_server = KeyName::parse(&server.public().fingerprint()).unwrap();
    let old_config = client_config(provider.clone(), old.clone(), to_server.clone(), &[ALPN]).unwrap();
    let new_config = client_config(provider, new.clone(), to_server, &[ALPN]).unwrap();
    assert_eq!(connect(addr, old_config.clone()).await.unwrap().1, old.public().fingerprint());
    assert_eq!(next(&mut rx).await.unwrap(), Some(old.public()));
    assert_eq!(connect(addr, new_config.clone()).await.unwrap().1, new.public().fingerprint());
    assert_eq!(next(&mut rx).await.unwrap(), Some(new.public()));
    // The server that runs takes the new set at the next handshake.
    clients.set([by_fingerprint]);
    assert!(connect(addr, old_config).await.is_err(), "the old key connected after the rotation");
    assert_eq!(refusal(&next(&mut rx).await.unwrap_err()), Some(Refusal::Unpinned(old.public())));
    assert_eq!(connect(addr, new_config).await.unwrap().1, new.public().fingerprint());
    task.abort();
}

#[test]
fn a_provider_with_no_ed25519_is_refused_by_name() {
    let mut provider = rustls::crypto::ring::default_provider();
    let all = provider.signature_verification_algorithms;
    let mapping: Vec<_> = all.mapping.iter().filter(|(s, _)| *s != rustls::SignatureScheme::ED25519).copied().collect();
    provider.signature_verification_algorithms =
        rustls::crypto::WebPkiSupportedAlgorithms { all: all.all, mapping: Box::leak(mapping.into_boxed_slice()) };
    let e = pod_tls::peer::supports(&provider).unwrap_err();
    assert!(e.contains("Ed25519"), "{e}");
    let e = client_config(Arc::new(provider), id(1), named(id(2).public()), &[]).unwrap_err();
    assert!(e.to_string().contains("Ed25519"), "{e}");
    assert!(pod_tls::peer::supports(&pod_tls::crypto::provider()).is_ok());
}
