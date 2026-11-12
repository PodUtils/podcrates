//! The device's key: P-256, as the API enrols it (`key_type` `secp256r1`),
//! and the self-signed certificate over it that the client presents in
//! TLS, valid for a day, as usque makes one. Pure Rust: no C compiler.

use std::fmt;
use std::sync::Arc;

use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey};
use pod_core::entropy::{Entropy as _, OsEntropy};
use rustls::{SignatureAlgorithm, SignatureScheme};
use zeroize::Zeroizing;

/// How long the device's certificate is valid: a day, as usque's.
pub const VALIDITY: i64 = 24 * 3600;

/// The device's key. Its scalar is wiped when the last copy is dropped
/// (ecdsa's `SigningKey`), and no `Debug` shows it.
#[derive(Clone)]
pub struct DeviceKey {
    signing: Arc<SigningKey>,
}

impl fmt::Debug for DeviceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceKey(P-256)")
    }
}

impl DeviceKey {
    /// A new key from this host's random source. A draw that is zero or not
    /// below the group's order is drawn again, eight times at most.
    pub fn generate() -> Result<DeviceKey, String> {
        for _ in 0..8 {
            let mut bytes = Zeroizing::new([0u8; 32]);
            OsEntropy.fill(&mut bytes[..]).map_err(|_| "this host's random source gave no bytes".to_string())?;
            if let Ok(key) = DeviceKey::from_scalar(&bytes[..]) {
                return Ok(key);
            }
        }
        Err("this host's random source gave no usable P-256 key".into())
    }

    /// The key of the 32 bytes of its scalar.
    pub fn from_scalar(bytes: &[u8]) -> Result<DeviceKey, String> {
        if bytes.len() != 32 {
            return Err(format!("a P-256 key is 32 bytes, not {}", bytes.len()));
        }
        let signing = SigningKey::from_slice(bytes).map_err(|_| "not a P-256 key".to_string())?;
        Ok(DeviceKey { signing: Arc::new(signing) })
    }

    /// The key of a PKCS #8 (`-----BEGIN PRIVATE KEY-----`, the form of
    /// podnet's identity file).
    pub fn from_pkcs8(der: &[u8]) -> Result<DeviceKey, String> {
        let scalar = crate::der::pkcs8::read(der).ok_or("not the PKCS #8 of a P-256 key")?;
        DeviceKey::from_scalar(scalar)
    }

    /// The 32 bytes of its scalar.
    pub fn scalar(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(self.signing.to_bytes().to_vec())
    }

    /// The key as PKCS #8, in a buffer that is wiped when it is dropped.
    pub fn pkcs8(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(crate::der::pkcs8::write(&self.scalar(), &self.point()))
    }

    /// Its public point, uncompressed (SEC1).
    pub fn point(&self) -> Vec<u8> {
        self.signing.verifying_key().to_sec1_point(false).as_bytes().to_vec()
    }

    /// Its SubjectPublicKeyInfo, as the API enrols it.
    pub fn spki(&self) -> Vec<u8> {
        crate::der::spki_p256(&self.point())
    }

    /// An ECDSA signature with SHA-256 of `message`, in DER, as X.509 and
    /// TLS carry it. RFC 6979's nonce: no random source at each signature.
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let signature: Signature = self.signing.sign(message);
        signature.to_der().as_bytes().to_vec()
    }

    /// The certificate that the client presents, valid from `now` (seconds
    /// since the epoch) for [`VALIDITY`].
    pub fn certificate(&self, now: i64) -> Vec<u8> {
        crate::der::self_signed(&self.spki(), now, now + VALIDITY, |tbs| self.sign(tbs))
    }

    /// The certificate and the key, as rustls presents them.
    pub fn certified(&self, now: i64) -> rustls::sign::CertifiedKey {
        let cert = rustls_pki_types::CertificateDer::from(self.certificate(now));
        rustls::sign::CertifiedKey::new(vec![cert], self.signer())
    }

    /// The key as rustls signs with it, for a certificate made elsewhere.
    pub fn signer(&self) -> Arc<dyn rustls::sign::SigningKey> {
        Arc::new(TlsKey(self.clone()))
    }
}

/// The key as rustls signs with it: ECDSA P-256 with SHA-256 alone, the one
/// scheme of a P-256 key in TLS 1.3.
#[derive(Debug)]
struct TlsKey(DeviceKey);

impl rustls::sign::SigningKey for TlsKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn rustls::sign::Signer>> {
        let scheme = SignatureScheme::ECDSA_NISTP256_SHA256;
        offered.contains(&scheme).then(|| Box::new(TlsSigner(self.0.clone())) as Box<dyn rustls::sign::Signer>)
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::ECDSA
    }
}

#[derive(Debug)]
struct TlsSigner(DeviceKey);

impl rustls::sign::Signer for TlsSigner {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        Ok(self.0.sign(message))
    }

    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::ECDSA_NISTP256_SHA256
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier as _;

    /// A key reads back from its scalar, with the same point.
    #[test]
    fn a_key_reads_back_from_its_scalar() {
        let key = DeviceKey::generate().unwrap();
        let again = DeviceKey::from_scalar(&key.scalar()).unwrap();
        assert_eq!(key.point(), again.point());
        assert_eq!(key.point().len(), 65);
        assert_eq!(key.point()[0], 0x04);
        assert!(DeviceKey::from_scalar(&[0u8; 32]).is_err(), "zero is no key");
        assert!(DeviceKey::from_scalar(&[1u8; 31]).is_err(), "31 bytes");
        assert_eq!(format!("{key:?}"), "DeviceKey(P-256)");
    }

    /// The PKCS #8 of a key reads back as the same key.
    #[test]
    fn a_key_reads_back_from_its_pkcs8() {
        let key = DeviceKey::generate().unwrap();
        let again = DeviceKey::from_pkcs8(&key.pkcs8()).unwrap();
        assert_eq!(key.point(), again.point());
        assert!(DeviceKey::from_pkcs8(b"not der").is_err());
    }

    /// The certificate as Go writes usque's: version 3, serial 0, empty
    /// names, a day, no extension, the key's SPKI, ECDSA with SHA-256, and
    /// a signature of the key over the TBS. The reader is the crate's own;
    /// the signature is checked with p256.
    #[test]
    fn the_certificate_reads_as_go_writes_usque_s() {
        use crate::der::{SEQUENCE, element, whole};
        let key = DeviceKey::generate().unwrap();
        let now = 1_791_440_989;
        let der = key.certificate(now);
        let (SEQUENCE, certificate, rest) = element(&der).unwrap() else { panic!("no SEQUENCE") };
        assert!(rest.is_empty());
        let (tbs, rest) = whole(certificate).unwrap();
        let (_, algorithm, rest) = element(rest).unwrap();
        assert_eq!(algorithm, crate::der::ECDSA_SHA256_ALG);
        let (_, bits, _) = element(rest).unwrap();
        let signature = Signature::from_der(&bits[1..]).unwrap();
        let verifying = p256::ecdsa::VerifyingKey::from_sec1_bytes(&key.point()).unwrap();
        verifying.verify(tbs, &signature).expect("signed by the key");
        let (_, fields, _) = element(tbs).unwrap();
        let (version, fields) = whole(fields).unwrap();
        assert_eq!(version, [0xa0, 3, 2, 1, 2], "version 3");
        let (serial, fields) = whole(fields).unwrap();
        assert_eq!(serial, [2, 1, 0], "serial 0");
        let (_, fields) = whole(fields).unwrap();
        let (issuer, fields) = whole(fields).unwrap();
        assert_eq!(issuer, [0x30, 0], "an empty issuer");
        let (validity, fields) = whole(fields).unwrap();
        assert_eq!(
            validity,
            crate::der::tlv(SEQUENCE, &[crate::der::time(now), crate::der::time(now + VALIDITY)].concat())
        );
        let (subject, _) = whole(fields).unwrap();
        assert_eq!(subject, [0x30, 0], "an empty subject");
        assert_eq!(crate::der::spki_of_certificate(&der), Some(key.spki().as_slice()));
        assert_eq!(crate::der::p256_point(&key.spki()), Some(key.point().as_slice()));
    }

    /// rustls's signer gives what the key gives, and only for its scheme.
    #[test]
    fn the_signer_of_tls_is_the_key_s() {
        use rustls::sign::SigningKey as _;
        let key = DeviceKey::generate().unwrap();
        let tls = TlsKey(key.clone());
        assert!(tls.choose_scheme(&[SignatureScheme::RSA_PSS_SHA256]).is_none());
        let signer = tls.choose_scheme(&[SignatureScheme::ECDSA_NISTP256_SHA256]).unwrap();
        let signature = Signature::from_der(&signer.sign(b"message").unwrap()).unwrap();
        let verifying = p256::ecdsa::VerifyingKey::from_sec1_bytes(&key.point()).unwrap();
        verifying.verify(b"message", &signature).unwrap();
    }
}
