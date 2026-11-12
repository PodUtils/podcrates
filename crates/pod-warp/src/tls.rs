//! TLS to the MASQUE front door: the client certificate of the device, and
//! the key of the front door pinned. No public CA signs the front door, and
//! its name is not the address that the client dials, so a chain check
//! cannot apply: the check is the exact key that the API gave. A proxy that
//! terminates TLS again shows another key and is refused.
//!
//! The provider is the caller's (`docs/design.md`, section 1, rule 4).

use std::sync::Arc;

use pod_core::{Class, Failure, Stage};
use rustls::client::ResolvesClientCert;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::sign::CertifiedKey;
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};

use crate::identity::{Identity, unix_now};

/// The text of a TLS failure when the front door shows a key that is not
/// the pinned key of the device record.
pub const PIN_MISMATCH: &str = "MASQUE peer key does not match the pinned key";

/// The ALPN of MASQUE over HTTP/2.
pub const ALPN_H2: &[u8] = b"h2";

/// The check of the front door: its key is `expected`.
#[derive(Debug)]
struct PinnedKey {
    expected: Vec<u8>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedKey {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let got = crate::identity::spki_from_cert(end_entity.as_ref())
            .ok_or_else(|| rustls::Error::General("unparsable peer certificate".into()))?;
        if got != self.expected {
            return Err(rustls::Error::General(PIN_MISMATCH.into()));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// The certificate of the device, made at each handshake.
#[derive(Debug)]
struct Fresh(Identity);

impl ResolvesClientCert for Fresh {
    fn resolve(&self, _hints: &[&[u8]], schemes: &[SignatureScheme]) -> Option<Arc<CertifiedKey>> {
        schemes.contains(&SignatureScheme::ECDSA_NISTP256_SHA256).then(|| Arc::new(self.0.certified(unix_now())))
    }

    fn has_certs(&self) -> bool {
        true
    }
}

/// The TLS configuration of a session: the certificate of `identity`, the
/// key `pinned_spki` (DER) of the front door, the ALPN `alpn`, on the
/// caller's `provider`.
pub fn client_config(
    provider: Arc<CryptoProvider>,
    identity: &Identity,
    pinned_spki: Vec<u8>,
    alpn: &[u8],
) -> Result<Arc<ClientConfig>, Failure> {
    let verifier = Arc::new(PinnedKey { expected: pinned_spki, provider: provider.clone() });
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Failure::caused(Stage::Tls, Class::Config, "the TLS versions of the provider", e))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_cert_resolver(Arc::new(Fresh(identity.clone())));
    config.alpn_protocols = vec![alpn.to_vec()];
    Ok(Arc::new(config))
}

/// The failure of a TLS handshake with the front door: a key that is not
/// the pinned key is `Protocol` and says so (the caller reads the device
/// record again); another failure is the class of its kind.
pub fn handshake_failure(door: &str, e: std::io::Error) -> Failure {
    let pin = e.to_string().contains(PIN_MISMATCH);
    let class = if pin { Class::Protocol } else { Class::of_io(e.kind()) };
    let text = format!("TLS handshake with the MASQUE front door {door}: {e}");
    Failure::caused(Stage::Tls, class, text, e)
}

/// True when `failure` is a key of the front door that is not the pinned
/// key.
pub fn is_pin_mismatch(failure: &Failure) -> bool {
    failure.chain().contains(PIN_MISMATCH)
}
