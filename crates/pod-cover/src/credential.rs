//! An application-layer delegated credential, bound to the name and the key
//! of a TLS chain (DELEG-2 and DELEG-3; podssh T-765 and T-766).
//!
//! The server holds the certificate for a name that it owns. After the TLS
//! handshake, and after the switch proof of [`crate::switch`], it signs a
//! short credential with the key of that certificate. The client checks the
//! credential against the same chain: the same name, the same public key, a
//! window and, when the issuer binds one, the exporter of the connection.
//!
//! **What it proves.** The holder of the subject key received this name, this
//! window and this exporter binding from the holder of the chain's key. The
//! signature is Ed25519 and the fields are covered whole, so a changed name,
//! issuer, window or key fails.
//!
//! **What it does not prove.**
//!
//! * It is not RFC 9345. There is no TLS CertificateVerify, no delegated
//!   certificate extension, no signature scheme negotiation and no wire
//!   format of a TLS handshake. The TLS stack still authenticates the
//!   certificate of the name.
//! * It does not prove that the issuer is honest, only that the issuer key
//!   signed these fields.
//! * It is not single use. It has a window, and a replay inside that window
//!   succeeds unless the caller also binds the exporter or uses a nonce of
//!   its own.
//! * It runs after the handshake. A client that never gets a credential still
//!   has a verified connection to the cover site.

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

/// The domain of the signed message. It keeps a credential of this crate
/// apart from another message of the same keys.
pub const DOMAIN: &[u8] = b"pod-cover credential v1";

/// A credential that one name's key issued to a subject key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credential {
    /// The name of the issuer: the name of the certificate chain that signed.
    pub issuer: String,
    /// The name that the credential is for.
    pub name: String,
    /// The first second of the window, in Unix time.
    pub not_before: i64,
    /// The last second of the window, in Unix time.
    pub not_after: i64,
    /// The Ed25519 public key of the subject.
    pub subject_key: [u8; 32],
    /// The SHA-256 of the TLS exporter, when the issuer bound one.
    pub exporter: Option<[u8; 32]>,
    /// The Ed25519 signature over the fields above.
    pub signature: [u8; 64],
}

/// Why a credential was not issued or not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialError {
    /// The issuer name is empty or not a name.
    BadIssuer,
    /// The name is empty or not a name.
    BadName,
    /// The window is empty or runs the wrong way.
    Window,
    /// The issuer of the credential is not the name of the chain.
    WrongIssuer,
    /// The name of the credential is not the name of the chain.
    NameMismatch,
    /// The window has not opened.
    NotYetValid,
    /// The window has closed.
    Expired,
    /// The exporter of the connection is not the one that the issuer bound.
    WrongBinding,
    /// The signature does not verify under the key of the chain.
    Signature,
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            CredentialError::BadIssuer => "the issuer is not a visible name",
            CredentialError::BadName => "the name is not a visible name",
            CredentialError::Window => "the window is empty or runs the wrong way",
            CredentialError::WrongIssuer => "the credential was issued by another name",
            CredentialError::NameMismatch => "the credential is for another name",
            CredentialError::NotYetValid => "the credential is not valid yet",
            CredentialError::Expired => "the credential is expired",
            CredentialError::WrongBinding => "the credential is bound to another exporter",
            CredentialError::Signature => "the credential signature does not verify",
        };
        f.write_str(text)
    }
}

impl std::error::Error for CredentialError {}

impl Credential {
    /// Sign a credential for `name` with `signing_key`, the key of the
    /// certificate of `issuer`.
    pub fn issue(
        signing_key: &SigningKey,
        issuer: &str,
        name: &str,
        not_before: i64,
        not_after: i64,
        subject_key: [u8; 32],
        exporter: Option<&[u8]>,
    ) -> Result<Credential, CredentialError> {
        check_name(issuer).map_err(|_| CredentialError::BadIssuer)?;
        check_name(name).map_err(|_| CredentialError::BadName)?;
        if not_before >= not_after {
            return Err(CredentialError::Window);
        }
        let mut credential = Credential {
            issuer: issuer.to_string(),
            name: name.to_string(),
            not_before,
            not_after,
            subject_key,
            exporter: exporter.map(digest),
            signature: [0; 64],
        };
        credential.signature = signing_key.sign(&credential.message()).to_bytes();
        Ok(credential)
    }

    /// Check the credential against a chain of `chain_name` whose key is
    /// `chain_key`, at `now`, with the exporter of the connection. A chain
    /// name that is not the issuer is [`CredentialError::WrongIssuer`].
    pub fn verify(
        &self,
        chain_name: &str,
        now: i64,
        exporter: Option<&[u8]>,
        chain_key: &VerifyingKey,
    ) -> Result<(), CredentialError> {
        if self.issuer != chain_name {
            return Err(CredentialError::WrongIssuer);
        }
        if self.name != chain_name {
            return Err(CredentialError::NameMismatch);
        }
        if now < self.not_before {
            return Err(CredentialError::NotYetValid);
        }
        if now > self.not_after {
            return Err(CredentialError::Expired);
        }
        match (self.exporter, exporter) {
            (None, None) => {}
            (Some(bound), Some(given)) if constant_time_eq(&bound, &digest(given)) => {}
            _ => return Err(CredentialError::WrongBinding),
        }
        chain_key
            .verify_strict(&self.message(), &Signature::from_bytes(&self.signature))
            .map_err(|_| CredentialError::Signature)
    }

    /// The signed bytes of each field, each with its length where a length
    /// keeps two fields apart.
    fn message(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DOMAIN.len() + self.issuer.len() + self.name.len() + 128);
        out.extend_from_slice(DOMAIN);
        out.extend_from_slice(&(self.issuer.len() as u32).to_be_bytes());
        out.extend_from_slice(self.issuer.as_bytes());
        out.extend_from_slice(&(self.name.len() as u32).to_be_bytes());
        out.extend_from_slice(self.name.as_bytes());
        out.extend_from_slice(&self.not_before.to_be_bytes());
        out.extend_from_slice(&self.not_after.to_be_bytes());
        out.extend_from_slice(&self.subject_key);
        match self.exporter {
            Some(exporter) => {
                out.push(1);
                out.extend_from_slice(&exporter);
            }
            None => out.push(0),
        }
        out
    }
}

/// A name of a certificate: visible ASCII with no space and no control byte.
fn check_name(name: &str) -> Result<(), ()> {
    let okay = !name.is_empty()
        && name.len() <= 255
        && name.bytes().all(|b| b.is_ascii_graphic() && b != b'/' && b != b'\\' && b != b':');
    if okay { Ok(()) } else { Err(()) }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Whether two digests are equal, with no early return.
fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut difference = 0u8;
    for (left, right) in a.iter().zip(b.iter()) {
        difference |= left ^ right;
    }
    std::hint::black_box(difference) == 0
}
