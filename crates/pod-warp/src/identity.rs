//! The MASQUE identity: a P-256 key and the self-signed certificate that
//! presents it.
//!
//! The MASQUE front door authenticates the client by mutual TLS. The public
//! half of the key is enrolled at the registration ([`crate::register`]),
//! and the private half signs a self-signed certificate that goes in the
//! TLS `Certificate` message. No bearer token goes on the tunnel: the
//! certificate is the credential.
//!
//! The key is podssh's pure-Rust key ([`DeviceKey`]); the API and the file
//! form are podnet's: a certificate PEM and a PKCS #8 key PEM. Each TLS
//! handshake presents a certificate made for it, valid for a day
//! ([`crate::key::VALIDITY`]), as podssh does: a kept certificate never
//! expires under a long-lived identity.

use base64::Engine as _;
use pod_core::{Class, Failure, Stage};
use zeroize::Zeroizing;

use crate::der;
use crate::key::DeviceKey;

/// The MASQUE identity of a device.
#[derive(Clone, Debug)]
pub struct Identity {
    key: DeviceKey,
    /// The SubjectPublicKeyInfo DER of the key.
    spki_der: Vec<u8>,
}

fn bad(text: impl Into<String>) -> Failure {
    Failure::new(Stage::Warp, Class::Config, text)
}

/// Seconds since the Unix epoch.
pub(crate) fn unix_now() -> i64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    i64::try_from(now.as_secs()).unwrap_or(i64::MAX)
}

impl Identity {
    /// A new P-256 key from the random source of the host.
    pub fn generate() -> Result<Identity, Failure> {
        let key = DeviceKey::generate().map_err(|e| Failure::new(Stage::Warp, Class::Unavailable, e))?;
        Ok(Identity::of(key))
    }

    /// The identity of `key`.
    pub fn of(key: DeviceKey) -> Identity {
        let spki_der = key.spki();
        Identity { key, spki_der }
    }

    /// The key.
    pub fn key(&self) -> &DeviceKey {
        &self.key
    }

    /// The base64 X.509 SubjectPublicKeyInfo to enrol.
    pub fn spki_b64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.spki_der)
    }

    /// The raw SubjectPublicKeyInfo DER.
    pub fn spki_der(&self) -> &[u8] {
        &self.spki_der
    }

    /// The certificate and the key as rustls presents them: a certificate
    /// valid from a minute before `now` (seconds since the epoch), so that a
    /// clock a little ahead of the front door's does not refuse it.
    pub fn certified(&self, now: i64) -> rustls::sign::CertifiedKey {
        self.key.certified(now - 60)
    }

    /// The certificate and the private key as PEM, so that a caller keeps
    /// the identity and resumes it with no second device. The key is the
    /// credential: whoever holds it opens the tunnel of the device.
    pub fn to_pem(&self) -> (String, Zeroizing<String>) {
        let cert = der::pem(&self.key.certificate(unix_now() - 60), "CERTIFICATE");
        let key = Zeroizing::new(der::pem(&self.key.pkcs8(), "PRIVATE KEY"));
        (cert, key)
    }

    /// The identity of [`Identity::to_pem`], or of podnet's identity file
    /// (rcgen's PEM). The certificate must carry the key of the private key.
    pub fn from_pem(cert_pem: &str, key_pem: &str) -> Result<Identity, Failure> {
        let pkcs8 = Zeroizing::new(
            der::from_pem(key_pem, "PRIVATE KEY").ok_or_else(|| bad("the MASQUE key PEM is not a PRIVATE KEY"))?,
        );
        let key = DeviceKey::from_pkcs8(&pkcs8).map_err(|e| bad(format!("the MASQUE key: {e}")))?;
        let cert = der::from_pem(cert_pem, "CERTIFICATE").ok_or_else(|| bad("the certificate PEM is empty"))?;
        let identity = Identity::of(key);
        match spki_from_cert(&cert) {
            Some(spki) if spki == identity.spki_der => Ok(identity),
            Some(_) => Err(bad("the MASQUE certificate carries another key than the private key")),
            None => Err(bad("the MASQUE certificate does not parse")),
        }
    }
}

/// The SubjectPublicKeyInfo DER of a PEM `PUBLIC KEY` block: the API gives
/// the key of the front door so, and the pin compares DER with DER.
pub fn spki_from_pem(pem: &str) -> Result<Vec<u8>, Failure> {
    der::from_pem(pem, "PUBLIC KEY").ok_or_else(|| {
        Failure::new(Stage::Warp, Class::Protocol, "the peer key of the device record is not a PUBLIC KEY PEM")
    })
}

/// The SubjectPublicKeyInfo DER of a DER certificate.
pub fn spki_from_cert(cert_der: &[u8]) -> Option<Vec<u8>> {
    der::spki_of_certificate(cert_der).map(<[u8]>::to_vec)
}

/// `yes` when the account of `warp status` has WARP+ (a paid account type,
/// or premium data), else `no`.
pub fn warp_plus_label(account: &serde_json::Value) -> &'static str {
    let kind = account.get("account_type").and_then(|v| v.as_str()).unwrap_or("");
    let premium = account.get("premium_data").and_then(|v| v.as_u64()).unwrap_or(0);
    if premium > 0 || !(kind.is_empty() || kind == "free") { "yes" } else { "no" }
}

/// The class of a failed license (PodUtils/podnet issue 173): a key that
/// the API refused (HTTP 4xx, other than 401, 403 and 429) is a usage
/// error; else the service is not available.
pub fn license_class(status: Option<u16>) -> Class {
    match status {
        Some(401 | 403 | 429) => Class::Unavailable,
        Some(400..=499) => Class::Usage,
        _ => Class::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_has_a_p256_spki() {
        let id = Identity::generate().unwrap();
        // A P-256 SPKI DER is 91 bytes and starts with the fixed
        // id-ecPublicKey AlgorithmIdentifier.
        assert_eq!(id.spki_der().len(), 91);
        assert_eq!(&id.spki_der()[..3], &[0x30, 0x59, 0x30]);
        assert_eq!(id.certified(1_800_000_000).cert.len(), 1);
        assert!(!id.spki_b64().is_empty());
    }

    #[test]
    fn the_certificate_carries_the_same_key_as_the_spki() {
        let id = Identity::generate().unwrap();
        let from_cert = spki_from_cert(id.certified(1_800_000_000).cert[0].as_ref()).unwrap();
        assert_eq!(from_cert, id.spki_der());
    }

    #[test]
    fn the_identity_survives_a_pem_round_trip() {
        let id = Identity::generate().unwrap();
        let (cert_pem, key_pem) = id.to_pem();
        assert!(cert_pem.starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(key_pem.contains("PRIVATE KEY"));
        let back = Identity::from_pem(&cert_pem, &key_pem).unwrap();
        assert_eq!(back.spki_der(), id.spki_der());
        assert_eq!(back.spki_b64(), id.spki_b64());
    }

    #[test]
    fn a_truncated_certificate_pem_is_an_error_not_a_panic() {
        let id = Identity::generate().unwrap();
        let (_, key_pem) = id.to_pem();
        let err = Identity::from_pem("-----BEGIN CERTIFICATE-----\n", &key_pem).unwrap_err();
        assert_eq!(err.class, Class::Config);
    }

    /// A certificate of another key is refused: the file was edited, or two
    /// files were mixed.
    #[test]
    fn a_certificate_of_another_key_is_refused() {
        let (cert_pem, _) = Identity::generate().unwrap().to_pem();
        let (_, key_pem) = Identity::generate().unwrap().to_pem();
        let err = Identity::from_pem(&cert_pem, &key_pem).unwrap_err();
        assert!(err.to_string().contains("another key"), "{err}");
    }

    #[test]
    fn pem_round_trip_matches() {
        let id = Identity::generate().unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(id.spki_der());
        let pem = format!("-----BEGIN PUBLIC KEY-----\n{b64}\n-----END PUBLIC KEY-----\n");
        assert_eq!(spki_from_pem(&pem).unwrap(), id.spki_der());
    }

    // PodUtils/podnet issue 173: the classes of warp license.
    #[test]
    fn the_classes_of_warp_license() {
        assert_eq!(license_class(Some(400)), Class::Usage);
        assert_eq!(license_class(Some(403)), Class::Unavailable);
        assert_eq!(license_class(Some(500)), Class::Unavailable);
        assert_eq!(license_class(None), Class::Unavailable);
    }

    #[test]
    fn warp_plus_is_a_paid_type_or_premium_data() {
        assert_eq!(warp_plus_label(&serde_json::json!({"account_type": "free"})), "no");
        assert_eq!(warp_plus_label(&serde_json::json!({"account_type": "limited"})), "yes");
        assert_eq!(warp_plus_label(&serde_json::json!({"premium_data": 5})), "yes");
    }
}
