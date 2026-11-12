//! Mutual TLS with pinned keys: a client that presents a P-256 key and a
//! server that requires a key of its client. Each side trusts only the keys
//! that it pins: the SHA-256 of a SubjectPublicKeyInfo, written
//! `sha256/BASE64` as curl's `--pinnedpubkey` writes it. No CA is read and
//! no name is checked: a pin names one key, and the handshake proves that
//! the peer holds it. Both sides take the provider of the caller, TLS 1.3
//! only, and no resumption: each connection proves its key. A set of pins
//! can change while a server runs: a rotation pins two keys at once, then
//! one, and the next handshake reads the new set.

use std::sync::{Arc, RwLock};

use base64::Engine as _;
use rustls::DigitallySignedStruct;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::sign::CertifiedKey;
use rustls::{CertificateError, DistinguishedName, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::error::Error;
use crate::server::Served;

/// One pinned key: the SHA-256 of its SubjectPublicKeyInfo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pin([u8; 32]);

impl Pin {
    /// The pin of a SubjectPublicKeyInfo in DER.
    pub fn of_spki(spki: &[u8]) -> Pin {
        Pin(Sha256::digest(spki).into())
    }

    /// The pin of the key of a DER certificate. No signature is checked.
    pub fn of_certificate(der: &CertificateDer<'_>) -> Result<Pin, Error> {
        crate::validity::spki(der.as_ref())
            .map(Pin::of_spki)
            .map_err(|why| Error::Server(format!("the key of the certificate: {why}")))
    }

    /// A pin written `sha256/BASE64` (44 characters of base64).
    pub fn parse(text: &str) -> Result<Pin, Error> {
        let bad = || Error::Server(format!("the pin {text:?} is not sha256/ and the base64 of 32 bytes"));
        let b64 = text.strip_prefix("sha256/").ok_or_else(bad)?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|_| bad())?;
        Ok(Pin(bytes.try_into().map_err(|_| bad())?))
    }
}

impl std::fmt::Display for Pin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sha256/{}", base64::engine::general_purpose::STANDARD.encode(self.0))
    }
}

/// The pinned keys of one side. A clone shares the set, so a server that
/// runs takes a new set at its next handshake.
#[derive(Clone, Debug, Default)]
pub struct Pins(Arc<RwLock<Vec<Pin>>>);

impl Pins {
    pub fn new(pins: impl IntoIterator<Item = Pin>) -> Pins {
        Pins(Arc::new(RwLock::new(pins.into_iter().collect())))
    }

    /// Replace the set: a rotation sets the old and the new key, then the
    /// new key alone.
    pub fn set(&self, pins: impl IntoIterator<Item = Pin>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = pins.into_iter().collect();
    }

    pub fn list(&self) -> Vec<Pin> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn holds(&self, pin: &Pin) -> bool {
        self.0.read().unwrap_or_else(|e| e.into_inner()).contains(pin)
    }
}

/// The pin of the key that the peer proved in the handshake (its leaf), for
/// an audit line: `peer_certificates()` of the connection.
pub fn peer_pin(certificates: Option<&[CertificateDer<'_>]>) -> Option<Pin> {
    certificates?.first().and_then(|leaf| Pin::of_certificate(leaf).ok())
}

/// A server that presents what `served` holds and requires a client whose
/// key `clients` pins. A client with another key, or with no certificate,
/// fails the handshake.
pub fn server_config(
    provider: Arc<CryptoProvider>,
    served: Arc<Served>,
    clients: Pins,
    alpn: &[&[u8]],
) -> Result<Arc<rustls::ServerConfig>, Error> {
    let verifier = Arc::new(Pinned::new(clients, &provider));
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| Error::Server(format!("the TLS versions: {e}")))?
        .with_client_cert_verifier(verifier)
        .with_cert_resolver(served);
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    // No resumption: a resumed session keeps the client of its first
    // handshake, so a key that a rotation removed would still connect.
    // Each connection proves its key against the pins of now.
    config.send_tls13_tickets = 0;
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    Ok(Arc::new(config))
}

/// A client that presents `key` (`server::certified_key`) and trusts only
/// a server whose key `server` pins. The server name of the connect is not
/// checked: the pin names the server.
pub fn client_config(
    provider: Arc<CryptoProvider>,
    key: Arc<CertifiedKey>,
    server: Pins,
    alpn: &[&[u8]],
) -> Result<Arc<rustls::ClientConfig>, Error> {
    let verifier = Arc::new(Pinned::new(server, &provider));
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| Error::Server(format!("the TLS versions: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_cert_resolver(Arc::new(Presents(key)));
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    // No resumption, as on the server: each connection checks the pins.
    config.resumption = rustls::client::Resumption::disabled();
    Ok(Arc::new(config))
}

/// The client's key, presented at each request of the server.
#[derive(Debug)]
pub(crate) struct Presents(pub(crate) Arc<CertifiedKey>);

impl rustls::client::ResolvesClientCert for Presents {
    fn resolve(&self, _hints: &[&[u8]], _schemes: &[SignatureScheme]) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }

    fn has_certs(&self) -> bool {
        true
    }
}

/// The verifier of each side: the leaf's key is pinned, and the handshake
/// signature is made with that key.
#[derive(Debug)]
struct Pinned {
    pins: Pins,
    schemes: WebPkiSupportedAlgorithms,
}

impl Pinned {
    fn new(pins: Pins, provider: &CryptoProvider) -> Pinned {
        Pinned { pins, schemes: provider.signature_verification_algorithms }
    }

    fn check(&self, leaf: &CertificateDer<'_>) -> Result<(), rustls::Error> {
        let pin =
            Pin::of_certificate(leaf).map_err(|_| rustls::Error::InvalidCertificate(CertificateError::BadEncoding))?;
        match self.pins.holds(&pin) {
            true => Ok(()),
            false => Err(rustls::Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)),
        }
    }
}

impl ClientCertVerifier for Pinned {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        leaf: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        self.check(leaf).map(|()| ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.schemes)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.schemes)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes.supported_schemes()
    }
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        leaf: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.check(leaf).map(|()| ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.schemes)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.schemes)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes.supported_schemes()
    }
}
