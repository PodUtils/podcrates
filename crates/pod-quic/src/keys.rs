//! The keys of an endpoint: HMAC-SHA-256 in pure Rust, of a random key for
//! each endpoint, so that noq needs no `ring` and no `aws-lc` (call C43).
//!
//! * [`ResetKey`] signs the stateless resets that the endpoint could send,
//!   as noq's default does.
//! * [`TokenKey`] tags the tokens of address validation of a server (a
//!   Retry's, and those that a connection gives for the next). noq's own
//!   needs a provider of AEAD. A token holds the client's own address and a
//!   time, nothing secret from it: the tag keeps it from being forged, which
//!   is all that a validation needs.

use hmac::{Hmac, KeyInit, Mac};
use noq::crypto::{CryptoError, HandshakeTokenKey, HmacKey};
use pod_core::entropy::{Entropy, OsEntropy};
use pod_core::{Class, Failure, Stage};
use sha2::Sha256;

/// The length of a signature and of a token's tag.
const TAG: usize = 32;

/// A key of 32 random bytes from `entropy`.
fn random(entropy: &mut dyn Entropy) -> Result<Hmac<Sha256>, Failure> {
    let mut key = [0u8; 32];
    entropy
        .fill(&mut key)
        .map_err(|e| Failure::caused(Stage::Quic, Class::Unavailable, format!("no randomness for QUIC: {e}"), e))?;
    Hmac::new_from_slice(&key).map_err(|e| Failure::new(Stage::Quic, Class::Config, e.to_string()))
}

/// The key of the stateless resets of an endpoint.
pub struct ResetKey(Hmac<Sha256>);

impl ResetKey {
    /// A key of the system's random bytes.
    pub fn random() -> Result<ResetKey, Failure> {
        ResetKey::from_entropy(&mut OsEntropy)
    }

    /// A key of `entropy`'s bytes.
    pub fn from_entropy(entropy: &mut dyn Entropy) -> Result<ResetKey, Failure> {
        random(entropy).map(ResetKey)
    }
}

impl HmacKey for ResetKey {
    fn sign(&self, data: &[u8], signature_out: &mut [u8]) {
        let mut mac = self.0.clone();
        mac.update(data);
        let tag = mac.finalize().into_bytes();
        let n = signature_out.len().min(tag.len());
        signature_out[..n].copy_from_slice(&tag[..n]);
    }

    fn signature_len(&self) -> usize {
        TAG
    }

    fn verify(&self, data: &[u8], signature: &[u8]) -> Result<(), CryptoError> {
        let mut mac = self.0.clone();
        mac.update(data);
        mac.verify_slice(signature).map_err(|_| CryptoError)
    }
}

/// The key of the tokens of address validation of a server.
pub struct TokenKey(Hmac<Sha256>);

impl TokenKey {
    /// A key of the system's random bytes.
    pub fn random() -> Result<TokenKey, Failure> {
        TokenKey::from_entropy(&mut OsEntropy)
    }

    /// A key of `entropy`'s bytes.
    pub fn from_entropy(entropy: &mut dyn Entropy) -> Result<TokenKey, Failure> {
        random(entropy).map(TokenKey)
    }

    fn mac(&self, nonce: u128, data: &[u8]) -> Hmac<Sha256> {
        let mut mac = self.0.clone();
        mac.update(&nonce.to_be_bytes());
        mac.update(data);
        mac
    }
}

impl HandshakeTokenKey for TokenKey {
    fn seal(&self, nonce: u128, data: &mut Vec<u8>) -> Result<(), CryptoError> {
        let tag = self.mac(nonce, data).finalize().into_bytes();
        data.extend_from_slice(&tag[..TAG]);
        Ok(())
    }

    fn open<'a>(&self, nonce: u128, data: &'a mut [u8]) -> Result<&'a [u8], CryptoError> {
        let data: &'a [u8] = data;
        let len = data.len().checked_sub(TAG).ok_or(CryptoError)?;
        let (body, tag) = data.split_at(len);
        self.mac(nonce, body).verify_slice(tag).map_err(|_| CryptoError)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pod_core::entropy::NoRandom;

    /// A token opens with its own nonce and its bytes, and with nothing
    /// else: a changed byte, another nonce, a cut tag or another key's.
    #[test]
    fn a_token_opens_only_as_it_was_sealed() {
        let (key, other) = (TokenKey::random().unwrap(), TokenKey::random().unwrap());
        let mut token = b"192.0.2.7:51000 at 1793577600".to_vec();
        key.seal(7, &mut token).unwrap();
        assert_eq!(token.len(), 29 + TAG);
        assert_eq!(key.open(7, &mut token.clone()).unwrap(), b"192.0.2.7:51000 at 1793577600");
        assert!(key.open(8, &mut token.clone()).is_err(), "another nonce");
        assert!(other.open(7, &mut token.clone()).is_err(), "another key");
        let mut changed = token.clone();
        changed[3] ^= 1;
        assert!(key.open(7, &mut changed).is_err(), "a changed byte");
        assert!(key.open(7, &mut token[..TAG - 1].to_vec()).is_err(), "shorter than a tag");
    }

    /// A reset's signature verifies with its key and its bytes alone.
    #[test]
    fn a_reset_signature_verifies_only_with_its_key_and_its_bytes() {
        let (key, other) = (ResetKey::random().unwrap(), ResetKey::random().unwrap());
        let mut signature = [0u8; TAG];
        key.sign(b"connection id", &mut signature);
        assert_eq!(key.signature_len(), TAG);
        assert!(key.verify(b"connection id", &signature).is_ok());
        assert!(key.verify(b"connection iD", &signature).is_err(), "other bytes");
        assert!(other.verify(b"connection id", &signature).is_err(), "another key");
    }

    /// A system with no random bytes gives a failure, not a weak key.
    #[test]
    fn no_randomness_is_a_failure_of_quic() {
        struct Dry;
        impl Entropy for Dry {
            fn fill(&mut self, _: &mut [u8]) -> Result<(), NoRandom> {
                Err(NoRandom)
            }
        }
        let e = ResetKey::from_entropy(&mut Dry).err().expect("no key");
        assert_eq!((e.stage, e.class), (Stage::Quic, Class::Unavailable));
        assert!(e.to_string().starts_with("no randomness for QUIC"), "{e}");
        assert!(TokenKey::from_entropy(&mut Dry).is_err());
    }
}
