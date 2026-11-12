//! The certificate of an Ed25519 identity: self-signed, minted here from
//! the key of `pod-identity`, and signed by that key. The handshake
//! signature is made by the identity itself, so no key loader of the
//! provider is necessary and the seed never leaves its `Identity`.

use std::sync::Arc;

use pod_identity::{Identity, PublicKey};
use rustls::pki_types::{CertificateDer, SubjectPublicKeyInfoDer, alg_id};
use rustls::sign::CertifiedKey;
use rustls::{SignatureAlgorithm, SignatureScheme};

/// The DER of an Ed25519 SubjectPublicKeyInfo before its 32 bytes of key:
/// the algorithm 1.3.101.112 with no parameters, and a BIT STRING of 33
/// bytes. RFC 8410 allows no other form.
const SPKI_HEAD: [u8; 12] = [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00];

/// The OID of Ed25519 (RFC 8410), as DER contents.
const ED25519: [u8; 3] = [0x2b, 0x65, 0x70];

/// The OID of the commonName attribute, as DER contents.
const COMMON_NAME: [u8; 3] = [0x55, 0x04, 0x03];

/// The subject and the issuer. TLS 1.3 encrypts the certificate, and no
/// peer reads a name: the key is the identity.
const SUBJECT: &[u8] = b"pod-tls peer";

/// The validity: from 1970 to the date of RFC 5280 for "no end"
/// (99991231235959Z). A pin does not expire; the owner of the key removes
/// it.
const NOT_BEFORE: &[u8] = b"700101000000Z";
const NOT_AFTER: &[u8] = b"99991231235959Z";

/// The Ed25519 key of the SubjectPublicKeyInfo of a DER certificate. A
/// certificate of another key, or one that cannot be read, gives none.
pub(crate) fn key_of(certificate: &CertificateDer<'_>) -> Option<PublicKey> {
    let spki = crate::validity::spki(certificate.as_ref()).ok()?;
    let key = spki.strip_prefix(&SPKI_HEAD[..])?;
    Some(PublicKey(key.try_into().ok()?))
}

/// The SubjectPublicKeyInfo of `key`, in the one form of RFC 8410.
pub(crate) fn spki_of(key: &PublicKey) -> SubjectPublicKeyInfoDer<'static> {
    rustls::sign::public_key_to_spki(&alg_id::ED25519, key.0)
}

/// One DER element: `tag`, the length, `body`. A certificate here is less
/// than 64 KiB.
fn der(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    match body.len() {
        n @ 0..=0x7f => out.push(n as u8),
        n @ 0x80..=0xff => out.extend_from_slice(&[0x81, n as u8]),
        n => out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]),
    }
    out.extend_from_slice(body);
    out
}

/// The self-signed certificate of `identity`: X.509 v3, an Ed25519 key,
/// signed by that key, with no extension. The same identity gives the same
/// bytes each time (an Ed25519 signature is deterministic).
pub fn certificate(identity: &Identity) -> CertificateDer<'static> {
    let key = identity.public();
    let algorithm = der(0x30, &der(0x06, &ED25519));
    let name = der(0x30, &der(0x31, &der(0x30, &[der(0x06, &COMMON_NAME), der(0x0c, SUBJECT)].concat())));
    // A positive serial of 16 bytes with no leading zero: the head of the
    // key, so that two identities do not share one.
    let mut serial = [0u8; 16];
    serial.copy_from_slice(&key.0[..16]);
    serial[0] = (serial[0] & 0x7f) | 0x40;
    let validity = der(0x30, &[der(0x17, NOT_BEFORE), der(0x18, NOT_AFTER)].concat());
    let spki = [&SPKI_HEAD[..], &key.0[..]].concat();
    let tbs = der(
        0x30,
        &[der(0xa0, &der(0x02, &[2])), der(0x02, &serial), algorithm.clone(), name.clone(), validity, name, spki]
            .concat(),
    );
    let mut bits = vec![0u8];
    bits.extend_from_slice(&identity.sign(&tbs));
    CertificateDer::from(der(0x30, &[tbs, algorithm, der(0x03, &bits)].concat()))
}

/// The signing key of `identity` for rustls: Ed25519, with any provider.
pub fn signing_key(identity: Arc<Identity>) -> Arc<dyn rustls::sign::SigningKey> {
    Arc::new(Ed25519Key(identity))
}

/// The certificate of `identity` with its signing key: what each side of
/// [`super`] presents.
pub fn certified_key(identity: Arc<Identity>) -> Arc<CertifiedKey> {
    Arc::new(CertifiedKey::new(vec![certificate(&identity)], signing_key(identity)))
}

/// The key of an identity. Its `Debug` shows the fingerprint, never the seed.
#[derive(Debug)]
struct Ed25519Key(Arc<Identity>);

impl rustls::sign::SigningKey for Ed25519Key {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn rustls::sign::Signer>> {
        offered
            .contains(&SignatureScheme::ED25519)
            .then(|| Box::new(Ed25519Signer(self.0.clone())) as Box<dyn rustls::sign::Signer>)
    }

    fn public_key(&self) -> Option<SubjectPublicKeyInfoDer<'_>> {
        Some(spki_of(&self.0.public()))
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::ED25519
    }
}

#[derive(Debug)]
struct Ed25519Signer(Arc<Identity>);

impl rustls::sign::Signer for Ed25519Signer {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        Ok(self.0.sign(message).to_vec())
    }

    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::ED25519
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The length of the head and of the body of the DER element at the
    /// start of `der`.
    fn element(der: &[u8]) -> (usize, usize) {
        match der[1] {
            0x81 => (3, der[2] as usize),
            0x82 => (4, ((der[2] as usize) << 8) | der[3] as usize),
            n => (2, n as usize),
        }
    }

    /// The TBSCertificate of a certificate, whole, and its signature.
    fn parts(der: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let (head, len) = element(der);
        assert_eq!(head + len, der.len(), "the length covers the certificate");
        let body = &der[head..];
        let (tbs_head, tbs_len) = element(body);
        (body[..tbs_head + tbs_len].to_vec(), body[body.len() - 64..].to_vec())
    }

    #[test]
    fn the_certificate_holds_the_key_and_is_signed_by_it() {
        let identity = Arc::new(Identity::from_seed(&[3; 32]));
        let der = certificate(&identity);
        assert_eq!(key_of(&der), Some(identity.public()));
        let (tbs, signature) = parts(der.as_ref());
        let signature: [u8; 64] = signature.try_into().unwrap();
        assert!(identity.public().verifies(&tbs, &signature), "the self-signature");
        assert_eq!(certificate(&identity), der, "the same bytes each time");
        // webpki reads it, and the key matches the signer.
        let certified = certified_key(identity);
        assert!(certified.keys_match().is_ok(), "{:?}", certified.keys_match());
    }

    #[test]
    fn a_certificate_of_another_key_or_of_no_form_gives_no_key() {
        assert_eq!(key_of(&CertificateDer::from(vec![0x30, 0x00])), None);
        let other = Identity::from_seed(&[4; 32]);
        let mut der = certificate(&other).as_ref().to_vec();
        // The OID of Ed448 (1.3.101.113) in place of Ed25519's.
        let at = der.windows(SPKI_HEAD.len()).position(|w| w == SPKI_HEAD).unwrap();
        der[at + 8] = 0x71;
        assert_eq!(key_of(&CertificateDer::from(der)), None);
    }
}
