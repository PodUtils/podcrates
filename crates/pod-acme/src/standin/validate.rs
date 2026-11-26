//! TLS-ALPN-01 as a CA validates it (RFC 8737, section 3): a TLS 1.2 or 1.3
//! connection to the address of the name, ALPN `acme-tls/1` alone, the
//! name as SNI; then `acme-tls/1` chosen, and a certificate whose
//! subjectAltName is the name alone and whose critical acmeIdentifier
//! holds the SHA-256 of the key authorization. The chain of that
//! certificate is not checked (RFC 8737, section 4); the handshake proves
//! its key.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::{DigitallySignedStruct, SignatureScheme};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use sha2::Digest as _;
use x509_parser::prelude::*;

/// The OID of id-pe-acmeIdentifier (RFC 8737, section 6.1).
pub const ACME_IDENTIFIER: &str = "1.3.6.1.5.5.7.1.31";
/// The limit of the connection and of its handshake.
const LIMIT: Duration = Duration::from_secs(10);

/// Validate TLS-ALPN-01 of `name` at `at`, for `key_authorization`. The
/// error says why, in the words that a CA gives in the challenge's error.
pub async fn validate(
    provider: Arc<CryptoProvider>,
    at: SocketAddr,
    name: &str,
    key_authorization: &str,
) -> Result<(), String> {
    let config = config(provider)?;
    let tcp = tokio::time::timeout(LIMIT, tokio::net::TcpStream::connect(at))
        .await
        .map_err(|_| format!("no connection to {at} within {} s", LIMIT.as_secs()))?
        .map_err(|e| format!("no connection to {at}: {}", pod_core::text::io_words(&e)))?;
    let words = pod_tls::Words::new("the stand-in CA");
    let tls = pod_tls::tls_over(tcp, name, config, LIMIT, &words)
        .await
        .map_err(|e| format!("the TLS handshake with {at} for {name}: {}", e.chain()))?;
    let (_, session) = tls.get_ref();
    if session.alpn_protocol() != Some(pod_tls::server::ACME_TLS_1) {
        return Err(format!("{at} did not choose the protocol acme-tls/1 for {name}"));
    }
    let leaf = session.peer_certificates().and_then(|c| c.first()).ok_or("the listener gave no certificate")?;
    check_certificate(leaf.as_ref(), name, key_authorization)
}

/// The checks of a certificate of TLS-ALPN-01 for `name`.
pub fn check_certificate(der: &[u8], name: &str, key_authorization: &str) -> Result<(), String> {
    let (_, cert) = parse_x509_certificate(der).map_err(|e| format!("the certificate cannot be read: {e}"))?;
    let san = cert
        .subject_alternative_name()
        .map_err(|e| format!("the subjectAltName cannot be read: {e}"))?
        .ok_or("the certificate has no subjectAltName")?;
    match san.value.general_names.as_slice() {
        [GeneralName::DNSName(d)] if d.eq_ignore_ascii_case(name) => {}
        names => return Err(format!("the subjectAltName is {names:?}, not the one dNSName {name}")),
    }
    let found = cert.extensions().iter().filter(|e| e.oid.to_id_string() == ACME_IDENTIFIER).collect::<Vec<_>>();
    let [extension] = found.as_slice() else {
        return Err(format!("the certificate has {} acmeIdentifier extensions, not one", found.len()));
    };
    if !extension.critical {
        return Err("the acmeIdentifier extension is not critical".into());
    }
    // The extnValue is the DER of an OCTET STRING of 32 bytes.
    let digest = sha2::Sha256::digest(key_authorization.as_bytes());
    let mut want = vec![0x04, 0x20];
    want.extend_from_slice(&digest);
    if extension.value != want.as_slice() {
        return Err("the acmeIdentifier is not the SHA-256 of the key authorization".into());
    }
    Ok(())
}

/// The client configuration of a validation: TLS 1.3 and 1.2 on
/// `provider`, ALPN `acme-tls/1` alone, any certificate whose key signs
/// the handshake.
fn config(provider: Arc<CryptoProvider>) -> Result<Arc<rustls::ClientConfig>, String> {
    let verifier = Arc::new(AnyCertificate(provider.clone()));
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|e| format!("the TLS versions: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    config.alpn_protocols = vec![pod_tls::server::ACME_TLS_1.to_vec()];
    Ok(Arc::new(config))
}

/// Takes the certificate of a challenge, which is self-signed, and checks
/// the signature of the handshake with its key.
#[derive(Debug)]
struct AnyCertificate(Arc<CryptoProvider>);

impl AnyCertificate {
    /// The signature of the handshake, by the key of `cert`. webpki does
    /// not read a certificate with a critical acmeIdentifier, so the key
    /// comes from x509-parser, and the algorithms of the scheme are the
    /// provider's.
    fn signed(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let bad = || rustls::Error::InvalidCertificate(rustls::CertificateError::BadSignature);
        let (_, parsed) = parse_x509_certificate(cert.as_ref()).map_err(|_| bad())?;
        let key = &parsed.public_key().subject_public_key.data;
        let algorithms = &self.0.signature_verification_algorithms;
        let (_, each) = algorithms.mapping.iter().find(|(scheme, _)| *scheme == dss.scheme).ok_or_else(bad)?;
        if each.iter().any(|alg| alg.verify_signature(key, message, dss.signature()).is_ok()) {
            return Ok(HandshakeSignatureValid::assertion());
        }
        Err(bad())
    }
}

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.signed(message, cert, dss)
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
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
