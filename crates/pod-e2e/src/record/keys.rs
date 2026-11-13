//! The keys of the records: the AEAD of a suite, and the schedule of one
//! direction. Each direction has a secret for each epoch; the key of an
//! epoch, and the secret of the next, come from it, and a rekey drops it,
//! so that a key of a later epoch does not give an earlier one. The resume
//! key and the secret of the tokens of each direction come from the
//! exporter.

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{AeadInOut, KeyInit};
use chacha20poly1305::ChaCha20Poly1305;
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use super::Suite;
use crate::TAG;
use crate::exporter::Exporter;

/// The AEAD of a suite, with its key.
pub enum Cipher {
    ChaCha(Box<ChaCha20Poly1305>),
    Aes(Box<Aes256Gcm>),
}

impl Cipher {
    pub fn new(suite: Suite, key: &[u8; 32]) -> Cipher {
        match suite {
            Suite::ChaCha20Poly1305 => Cipher::ChaCha(Box::new(ChaCha20Poly1305::new(key.into()))),
            Suite::Aes256Gcm => Cipher::Aes(Box::new(Aes256Gcm::new(key.into()))),
        }
    }

    /// Seal `body` in place under `nonce` with `aad`: its tag.
    pub fn seal(&self, nonce: &[u8; 12], aad: &[u8], body: &mut [u8]) -> [u8; TAG] {
        let tag = match self {
            Cipher::ChaCha(c) => c.encrypt_inout_detached(&(*nonce).into(), aad, body.into()).map(|t| t.into()),
            Cipher::Aes(c) => c.encrypt_inout_detached(&(*nonce).into(), aad, body.into()).map(|t| t.into()),
        };
        // Each body is at most MAX_LENGTH, far below the limit of either AEAD.
        tag.unwrap_or([0u8; TAG])
    }

    /// Open `body` in place under `nonce` with `aad` and `tag`; on a failed
    /// check the body is not plain text.
    pub fn open(&self, nonce: &[u8; 12], aad: &[u8], body: &mut [u8], tag: &[u8; TAG]) -> bool {
        match self {
            Cipher::ChaCha(c) => c.decrypt_inout_detached(&(*nonce).into(), aad, body.into(), &(*tag).into()).is_ok(),
            Cipher::Aes(c) => c.decrypt_inout_detached(&(*nonce).into(), aad, body.into(), &(*tag).into()).is_ok(),
        }
    }
}

/// The direction of the records that a sender of the role `initiator`
/// seals: its name in the context of each of its keys.
pub fn direction(initiator: bool) -> &'static [u8] {
    if initiator { b"initiator" } else { b"responder" }
}

/// The context of a key of `suite` for `direction`.
fn context(suite: Suite, direction: &[u8]) -> Vec<u8> {
    [suite.name().as_bytes(), &[0], direction].concat()
}

/// HKDF-Expand of `secret` with `info`.
fn expand<const N: usize>(secret: &[u8; 32], info: &[u8]) -> Zeroizing<[u8; N]> {
    let mut out = Zeroizing::new([0u8; N]);
    // A secret of 32 bytes is a PRK of SHA-256, and N is at most 32.
    if let Ok(hkdf) = Hkdf::<Sha256>::from_prk(secret) {
        let _ = hkdf.expand(info, &mut out[..]);
    }
    out
}

/// One epoch of a direction: its number, its key, and the secret of the
/// next.
pub struct Epoch {
    pub number: u64,
    pub cipher: Cipher,
    secret: Zeroizing<[u8; 32]>,
}

impl Epoch {
    /// The epoch after this one, of `suite`.
    pub fn next(&self, suite: Suite) -> Epoch {
        Epoch::of(suite, self.number + 1, expand(&self.secret, b"pod-e2e record next"))
    }

    fn of(suite: Suite, number: u64, secret: Zeroizing<[u8; 32]>) -> Epoch {
        let key = expand::<32>(&secret, b"pod-e2e record key");
        Epoch { number, cipher: Cipher::new(suite, &key), secret }
    }

    /// The parity of the epoch, as the flags carry it.
    pub fn phase(&self) -> bool {
        self.number & 1 == 1
    }
}

/// What the exporter gives one direction beside its first epoch: its first
/// prefix of the nonce, its resume key, and the secret of its tokens.
pub struct Start {
    pub prefix: [u8; 4],
    pub resume: Cipher,
    tokens: Zeroizing<[u8; 32]>,
}

impl Start {
    /// The first epoch of the direction of the sender `initiator`, and its
    /// start.
    pub fn of(exporter: &Exporter, suite: Suite, initiator: bool) -> Result<(Epoch, Start), crate::Error> {
        let ctx = context(suite, direction(initiator));
        let mut secret = Zeroizing::new([0u8; 32]);
        exporter.expand("pod-e2e record", &ctx, &mut secret[..])?;
        let mut prefix = [0u8; 4];
        exporter.expand("pod-e2e record prefix", &ctx, &mut prefix)?;
        let mut resume = Zeroizing::new([0u8; 32]);
        exporter.expand("pod-e2e resume key", &ctx, &mut resume[..])?;
        let mut tokens = Zeroizing::new([0u8; 32]);
        exporter.expand("pod-e2e resume token", &ctx, &mut tokens[..])?;
        Ok((Epoch::of(suite, 0, secret), Start { prefix, resume: Cipher::new(suite, &resume), tokens }))
    }

    /// The token of the RESUME record of number `index`.
    pub fn token(&self, index: u64) -> Zeroizing<[u8; 32]> {
        expand(&self.tokens, &[&b"pod-e2e token "[..], &index.to_be_bytes()].concat())
    }
}

/// The nonce of a record: the prefix of its direction, then its counter.
pub fn nonce(prefix: [u8; 4], counter: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..4].copy_from_slice(&prefix);
    n[4..].copy_from_slice(&counter.to_be_bytes());
    n
}

/// Equal bytes, in a time that does not depend on where they differ.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
