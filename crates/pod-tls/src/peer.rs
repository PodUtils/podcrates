//! TLS between two peers that each pin the Ed25519 key of the other: the
//! key of `pod-identity`, with no CA and no name. Each side presents a
//! self-signed certificate of its own identity ([`certificate`]); the
//! client takes only the server key that it names, and the server requires
//! a client certificate whose key is in its set ([`Keys`]). A key is named
//! as `pod_identity::KeyName` names one: the key itself, its fingerprint or
//! its display form.
//!
//! TLS 1.3 only, the scheme Ed25519 only, and no resumption: each
//! connection proves its key against the pins of now, so a key that a
//! rotation removed does not connect again. The handshake signature is
//! checked with the pinned key itself, not with a second reading of the
//! certificate. A peer of another key fails the handshake with a
//! [`Refusal`] that names the key that it proved; [`refusal`] finds it in
//! the error of the handshake.

mod cert;

use std::fmt;
use std::sync::{Arc, RwLock};

use pod_identity::{KeyName, PublicKey};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{CertificateError, DigitallySignedStruct, DistinguishedName, OtherError, PeerMisbehaved, SignatureScheme};

pub use cert::{certificate, certified_key, signing_key};

use crate::error::Error;
use crate::mutual::Presents;
use crate::server::Served;

/// The pinned keys of one side. A clone shares the set, so a server that
/// runs takes a new set at its next handshake.
#[derive(Clone, Debug, Default)]
pub struct Keys(Arc<RwLock<Vec<KeyName>>>);

impl Keys {
    pub fn new(keys: impl IntoIterator<Item = KeyName>) -> Keys {
        Keys(Arc::new(RwLock::new(keys.into_iter().collect())))
    }

    /// Replace the set: a rotation sets the old and the new key, then the
    /// new key alone.
    pub fn set(&self, keys: impl IntoIterator<Item = KeyName>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = keys.into_iter().collect();
    }

    pub fn list(&self) -> Vec<KeyName> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Whether a name of the set names `key`.
    pub fn admits(&self, key: &PublicKey) -> bool {
        self.0.read().unwrap_or_else(|e| e.into_inner()).iter().any(|name| name.matches(key))
    }
}

/// Why a side refused the certificate of its peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The peer proved this Ed25519 key, and no pin names it.
    Unpinned(PublicKey),
    /// The certificate of the peer holds no Ed25519 key.
    NotEd25519,
}

impl Refusal {
    /// The key that the peer showed, when it is an Ed25519 key.
    pub fn key(&self) -> Option<PublicKey> {
        match self {
            Refusal::Unpinned(key) => Some(*key),
            Refusal::NotEd25519 => None,
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Unpinned(key) => {
                write!(f, "the peer's key {} ({}) is not pinned", key.fingerprint(), key.display_fingerprint())
            }
            Refusal::NotEd25519 => f.write_str("the peer's certificate holds no Ed25519 key"),
        }
    }
}

impl std::error::Error for Refusal {}

impl From<Refusal> for rustls::Error {
    fn from(refusal: Refusal) -> rustls::Error {
        rustls::Error::InvalidCertificate(CertificateError::Other(OtherError(Arc::new(refusal))))
    }
}

/// The [`Refusal`] in an error of a handshake: in a `rustls::Error`, in the
/// `std::io::Error` of tokio-rustls that holds one, or in an error whose
/// sources hold one.
pub fn refusal(error: &(dyn std::error::Error + 'static)) -> Option<Refusal> {
    let mut at = Some(error);
    while let Some(e) = at {
        if let Some(found) = e.downcast_ref::<Refusal>() {
            return Some(found.clone());
        }
        if let Some(rustls::Error::InvalidCertificate(CertificateError::Other(other))) = e.downcast_ref()
            && let Some(found) = other.0.downcast_ref::<Refusal>()
        {
            return Some(found.clone());
        }
        // The source of an io::Error is the source of its inner error, not
        // the inner error itself.
        at = match e.downcast_ref::<std::io::Error>().and_then(|io| io.get_ref()) {
            Some(inner) => Some(inner as &(dyn std::error::Error + 'static)),
            None => e.source(),
        };
    }
    None
}

/// The Ed25519 key that the peer proved in the handshake (its leaf), from
/// `peer_certificates()` of the connection.
pub fn peer_key(certificates: Option<&[CertificateDer<'_>]>) -> Option<PublicKey> {
    certificates?.first().and_then(cert::key_of)
}

/// Whether `provider` can make the TLS of this module: a suite of TLS 1.3,
/// and the verification of Ed25519. The error names what is missing.
pub fn supports(provider: &CryptoProvider) -> Result<(), String> {
    if !provider.cipher_suites.iter().any(|s| s.version() == &rustls::version::TLS13) {
        return Err("the TLS provider has no suite of TLS 1.3".into());
    }
    if !provider.signature_verification_algorithms.supported_schemes().contains(&SignatureScheme::ED25519) {
        return Err("the TLS provider verifies no Ed25519 signature".into());
    }
    Ok(())
}

/// A client that presents `identity` and takes only the server whose key
/// `server` names. The server name of the connect is not checked.
pub fn client_config(
    provider: Arc<CryptoProvider>,
    identity: Arc<pod_identity::Identity>,
    server: KeyName,
    alpn: &[&[u8]],
) -> Result<Arc<rustls::ClientConfig>, Error> {
    let verifier = Arc::new(Pinned::new(Keys::new([server]), &provider)?);
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| Error::Config(format!("the TLS versions: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_cert_resolver(Arc::new(Presents(certified_key(identity))));
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    config.resumption = rustls::client::Resumption::disabled();
    Ok(Arc::new(config))
}

/// A server that presents `identity` and requires a client whose key
/// `clients` names. A client of another key, or with no certificate, fails
/// the handshake.
pub fn server_config(
    provider: Arc<CryptoProvider>,
    identity: Arc<pod_identity::Identity>,
    clients: Keys,
    alpn: &[&[u8]],
) -> Result<Arc<rustls::ServerConfig>, Error> {
    let verifier = Arc::new(Pinned::new(clients, &provider)?);
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| Error::Config(format!("the TLS versions: {e}")))?
        .with_client_cert_verifier(verifier)
        .with_cert_resolver(Served::new(Some(certified_key(identity))));
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    // No resumption: a resumed session keeps the client of its first
    // handshake, so a key that a rotation removed would still connect.
    config.send_tls13_tickets = 0;
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    Ok(Arc::new(config))
}

/// The verifier of each side: the leaf holds a pinned Ed25519 key, and the
/// handshake signature is made with that key.
#[derive(Debug)]
struct Pinned {
    keys: Keys,
    schemes: WebPkiSupportedAlgorithms,
}

impl Pinned {
    fn new(keys: Keys, provider: &CryptoProvider) -> Result<Pinned, Error> {
        supports(provider).map_err(Error::Config)?;
        Ok(Pinned { keys, schemes: provider.signature_verification_algorithms })
    }

    fn check(&self, leaf: &CertificateDer<'_>) -> Result<(), rustls::Error> {
        let key = cert::key_of(leaf).ok_or(Refusal::NotEd25519)?;
        if self.keys.admits(&key) { Ok(()) } else { Err(Refusal::Unpinned(key).into()) }
    }

    /// The signature of the handshake, checked with the key of the leaf in
    /// the one form of RFC 8410.
    fn signed(
        &self,
        message: &[u8],
        leaf: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        if dss.scheme != SignatureScheme::ED25519 {
            return Err(PeerMisbehaved::SignedHandshakeWithUnadvertisedSigScheme.into());
        }
        let key = cert::key_of(leaf).ok_or(Refusal::NotEd25519)?;
        rustls::crypto::verify_tls13_signature_with_raw_key(message, &cert::spki_of(&key), dss, &self.schemes)
    }
}

/// TLS 1.2 is never offered; a signature of it is refused.
fn no_tls12() -> Result<HandshakeSignatureValid, rustls::Error> {
    Err(rustls::Error::General("TLS 1.2 is refused: the peers of pinned Ed25519 keys speak TLS 1.3".into()))
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
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        no_tls12()
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.signed(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
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
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        no_tls12()
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.signed(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
}
