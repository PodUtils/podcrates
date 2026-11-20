//! The certificates and keys of a bundle: an ECDSA or Ed25519 signature by
//! a key, a certificate signed by its issuer within the life of both, and
//! the identity of the leaf.

use rustls_pki_types::SignatureVerificationAlgorithm;
use x509_parser::prelude::*;

use super::Identity;

/// The extension of Fulcio's leaf that names the token's issuer: the second
/// form, a DER UTF8String, and the first, the raw text.
pub const ISSUER_V2: &str = "1.3.6.1.4.1.57264.1.8";
pub const ISSUER_V1: &str = "1.3.6.1.4.1.57264.1.1";

pub(super) const P256: &str = "1.2.840.10045.3.1.7";
pub(super) const P384: &str = "1.3.132.0.34";
pub(super) const ECDSA_SHA256: &str = "1.2.840.10045.4.3.2";
pub(super) const ECDSA_SHA384: &str = "1.2.840.10045.4.3.3";
pub(super) const ED25519: &str = "1.3.101.112";

/// The curve of an EC key, as the OID of its parameters.
pub(super) fn curve(key_info: &SubjectPublicKeyInfo<'_>) -> Option<String> {
    key_info.algorithm.parameters.as_ref().and_then(|p| p.as_oid().ok()).map(|o| o.to_id_string())
}

/// `signature` of `message` by the key of `key_info`, with the digest that
/// `algorithm` (an OID of `ecdsa-with-SHA*`) names.
pub(super) fn ecdsa(
    key_info: &SubjectPublicKeyInfo<'_>,
    algorithm: &str,
    message: &[u8],
    signature: &[u8],
) -> Result<(), String> {
    use pod_tls::crypto::sign::{EcdsaP256Sha256, EcdsaP256Sha384, EcdsaP384Sha256, EcdsaP384Sha384};
    let key = &key_info.subject_public_key.data;
    let verifier: &dyn SignatureVerificationAlgorithm = match (curve(key_info).as_deref(), algorithm) {
        (Some(P256), ECDSA_SHA256) => &EcdsaP256Sha256,
        (Some(P256), ECDSA_SHA384) => &EcdsaP256Sha384,
        (Some(P384), ECDSA_SHA256) => &EcdsaP384Sha256,
        (Some(P384), ECDSA_SHA384) => &EcdsaP384Sha384,
        (curve, algorithm) => return Err(format!("a key of curve {curve:?} and the algorithm {algorithm}")),
    };
    verifier.verify_signature(key, message, signature).map_err(|_| "the signature does not verify".to_string())
}

/// `signature` of `message` by the key of `key_info`: an Ed25519 key, or an
/// EC key with the digest of its curve (SHA-256 for P-256, SHA-384 for
/// P-384), as a log signs its promise and its checkpoint.
pub(super) fn by_key(key_info: &SubjectPublicKeyInfo<'_>, message: &[u8], signature: &[u8]) -> Result<(), String> {
    if key_info.algorithm.algorithm.to_id_string() == ED25519 {
        return pod_tls::crypto::sign::ED25519
            .verify_signature(&key_info.subject_public_key.data, message, signature)
            .map_err(|_| "the signature does not verify".to_string());
    }
    let algorithm = if curve(key_info).as_deref() == Some(P384) { ECDSA_SHA384 } else { ECDSA_SHA256 };
    ecdsa(key_info, algorithm, message, signature)
}

/// `cert` was signed by `issuer`, and both hold at `time`.
pub(super) fn signed_by(cert: &X509Certificate<'_>, issuer: &X509Certificate<'_>, time: i64) -> Result<(), String> {
    if cert.issuer() != issuer.subject() {
        return Err(format!("{} was not issued by {}", cert.subject(), issuer.subject()));
    }
    let algorithm = cert.signature_algorithm.algorithm.to_id_string();
    ecdsa(issuer.public_key(), &algorithm, cert.tbs_certificate.as_ref(), &cert.signature_value.data)
        .map_err(|why| format!("{}'s signature by {}: {why}", cert.subject(), issuer.subject()))?;
    for c in [cert, issuer] {
        let (from, to) = (c.validity().not_before.timestamp(), c.validity().not_after.timestamp());
        if time < from || time > to {
            return Err(format!("{} did not hold at the time of the signature", c.subject()));
        }
    }
    Ok(())
}

/// The leaf's identity: code signing, the URI of `who`, and the issuer of
/// `who`.
pub(super) fn identity(leaf: &X509Certificate<'_>, who: &Identity) -> Result<(), String> {
    let signs = leaf.extended_key_usage().ok().flatten().is_some_and(|e| e.value.code_signing);
    if !signs {
        return Err("the leaf is not for code signing".into());
    }
    let uris: Vec<&str> = match leaf.subject_alternative_name().ok().flatten() {
        Some(san) => san
            .value
            .general_names
            .iter()
            .filter_map(|n| if let GeneralName::URI(u) = n { Some(*u) } else { None })
            .collect(),
        None => Vec::new(),
    };
    if !uris.contains(&who.uri.as_str()) {
        return Err(format!("the leaf names {uris:?}, not {}", who.uri));
    }
    let issuer = leaf.extensions().iter().find_map(|e| match e.oid.to_id_string().as_str() {
        // A UTF8String: its tag, a length of one byte, the text.
        ISSUER_V2 if e.value.len() > 2 && e.value[0] == 0x0c && usize::from(e.value[1]) == e.value.len() - 2 => {
            Some(String::from_utf8_lossy(&e.value[2..]).into_owned())
        }
        ISSUER_V1 => Some(String::from_utf8_lossy(e.value).into_owned()),
        _ => None,
    });
    match issuer {
        Some(issuer) if issuer == who.issuer => Ok(()),
        other => Err(format!("the leaf's token came from {other:?}, not {}", who.issuer)),
    }
}
