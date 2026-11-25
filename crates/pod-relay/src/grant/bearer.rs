//! A bearer grant bound to one end-to-end channel (PodUtils/podssh#355): a
//! short token that a key of the user's identity file signs, for a person
//! with no key file on the host. It rides where a grant rides, in the
//! operator's proof, and it names the binding of its channel
//! ([`pod_e2e::Binding`]), so a copy is worthless on each other channel.
//!
//! The form, 221 bytes: [`MAGIC`] (13), the signer's Ed25519 key (32), the
//! holder's key (32), `iat` and `exp` (u64, big-endian, seconds since the
//! epoch), the client's nonce (32), the `jti` (32, HMAC-SHA-256 of the nonce
//! under the binding), then the signer's Ed25519 signature of each byte
//! before it (64). The magic keeps the signed bytes apart from SSH's (which
//! start with a length), so an agent's signature of a bearer signs no login.
//!
//! The holder is the operator's key that the channel proves: the binding is
//! taken before the operator's proof, so it does not cover that key.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, PoisonError};

use hmac::{Hmac, KeyInit, Mac};
use pod_e2e::Binding;
use pod_identity::{Identity, PublicKey};
use sha2::Sha256;

/// The first bytes of a bearer: the form and its version.
pub const MAGIC: &[u8; 13] = b"pod-bearer/1\n";
/// The size of a bearer.
pub const LEN: usize = MAGIC.len() + 32 + 32 + 8 + 8 + 32 + 32 + 64;
/// The longest life of a bearer: `exp` is at most this after `iat`.
pub const LIFE: u64 = 30;
/// The skew of the clocks that the verifier allows at each end of a life.
pub const SKEW: u64 = 10;
/// The most live entries of a verifier's record of the `jti`s.
pub const SEEN: usize = 16_384;

/// Where the signature starts.
const SIGNED: usize = LEN - 64;

/// Whether `blob` has the form of a bearer, not of a certificate: a node
/// that takes both tells them apart by it.
pub fn is_bearer(blob: &[u8]) -> bool {
    blob.starts_with(MAGIC)
}

/// The `jti` of `nonce` on the channel of `binding`: HMAC-SHA-256 of the
/// nonce under the binding.
pub fn jti(binding: &Binding, nonce: &[u8; 32]) -> [u8; 32] {
    // HMAC takes a key of each length, so `new_from_slice` cannot fail.
    let mut mac = match Hmac::<Sha256>::new_from_slice(binding.bytes()) {
        Ok(mac) => mac,
        Err(_) => <Hmac<Sha256> as KeyInit>::new(&Default::default()),
    };
    mac.update(nonce);
    mac.finalize().into_bytes().into()
}

/// The fields of a bearer before its signature: what the signer signs.
#[derive(Clone)]
pub struct Unsigned {
    message: [u8; SIGNED],
}

impl Unsigned {
    /// A bearer of `signer` for the operator key `holder` on the channel of
    /// `binding`, made at `iat` with the client's fresh `nonce`; it ends
    /// [`LIFE`] seconds later.
    pub fn new(signer: PublicKey, holder: PublicKey, binding: &Binding, iat: u64, nonce: [u8; 32]) -> Unsigned {
        let mut message = [0u8; SIGNED];
        let parts: [&[u8]; 7] = [
            MAGIC,
            &signer.0,
            &holder.0,
            &iat.to_be_bytes(),
            &iat.saturating_add(LIFE).to_be_bytes(),
            &nonce,
            &jti(binding, &nonce),
        ];
        let mut at = 0;
        for part in parts {
            message[at..at + part.len()].copy_from_slice(part);
            at += part.len();
        }
        Unsigned { message }
    }

    /// The bytes that the signer signs, as they are: an agent signs them
    /// with the signer's Ed25519 key.
    pub fn message(&self) -> &[u8] {
        &self.message
    }

    /// The bearer, with the signer's `signature` of [`Unsigned::message`].
    pub fn signed(&self, signature: &[u8; 64]) -> Vec<u8> {
        let mut out = Vec::with_capacity(LEN);
        out.extend_from_slice(&self.message);
        out.extend_from_slice(signature);
        out
    }
}

/// A bearer that `signer` signs for the operator key `holder` on the
/// channel of `binding`, made at `iat` with the client's fresh `nonce`.
pub fn mint(signer: &Identity, holder: PublicKey, binding: &Binding, iat: u64, nonce: [u8; 32]) -> Vec<u8> {
    let unsigned = Unsigned::new(signer.public(), holder, binding, iat, nonce);
    unsigned.signed(&signer.sign(unsigned.message()))
}

/// A bearer, read: its fields, unchecked.
#[derive(Clone)]
pub struct Bearer {
    pub signer: PublicKey,
    pub holder: PublicKey,
    pub iat: u64,
    pub exp: u64,
    nonce: [u8; 32],
    jti: [u8; 32],
    signature: [u8; 64],
    signed: [u8; SIGNED],
}

/// The nonce, the `jti` and the signature stay out of each print.
impl fmt::Debug for Bearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bearer")
            .field("signer", &self.signer.fingerprint())
            .field("holder", &self.holder.fingerprint())
            .field("iat", &self.iat)
            .field("exp", &self.exp)
            .finish_non_exhaustive()
    }
}

impl Bearer {
    /// The bearer in `blob`: the magic and the size alone are checked.
    pub fn read(blob: &[u8]) -> Result<Bearer, Refused> {
        if blob.len() != LEN || !is_bearer(blob) {
            return Err(Refused::Form);
        }
        let field = |at: usize| -> [u8; 32] { blob[at..at + 32].try_into().expect("32 bytes") };
        let number = |at: usize| u64::from_be_bytes(blob[at..at + 8].try_into().expect("8 bytes"));
        let m = MAGIC.len();
        Ok(Bearer {
            signer: PublicKey(field(m)),
            holder: PublicKey(field(m + 32)),
            iat: number(m + 64),
            exp: number(m + 72),
            nonce: field(m + 80),
            jti: field(m + 112),
            signature: blob[SIGNED..].try_into().expect("64 bytes"),
            signed: blob[..SIGNED].try_into().expect("the signed bytes"),
        })
    }
}

/// Why a verifier refused a bearer: one class for each check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// No binding came with it: it was checked outside the node's check of
    /// a grant, so nothing ties it to a channel.
    Unbound,
    /// Not the form of a bearer.
    Form,
    /// It is for another operator key than the one that the channel proved.
    Holder,
    /// Its signer is not a key that the node takes.
    Signer,
    /// Its signature does not verify under its signer's key.
    Signature,
    /// Its `exp` is not after its `iat`, or more than [`LIFE`] after it.
    Life,
    /// Its `iat` is more than [`SKEW`] after now.
    Early,
    /// Now is at or after its `exp` and the skew.
    Expired,
    /// Its `jti` is not the one of this channel: a bearer of another
    /// channel, or a changed one.
    Binding,
    /// Its `jti` was used already.
    Replayed,
    /// The record of the `jti`s is full, so it cannot be recorded.
    Full,
}

impl Refused {
    /// The class's stable name, for a log and a test.
    pub fn name(self) -> &'static str {
        match self {
            Refused::Unbound => "unbound",
            Refused::Form => "form",
            Refused::Holder => "holder",
            Refused::Signer => "signer",
            Refused::Signature => "signature",
            Refused::Life => "life",
            Refused::Early => "early",
            Refused::Expired => "expired",
            Refused::Binding => "binding",
            Refused::Replayed => "replayed",
            Refused::Full => "full",
        }
    }
}

/// The text names the class and the rule, never a byte of the bearer.
impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Refused::Unbound => "no binding of a channel came with it",
            Refused::Form => "it is not a bearer",
            Refused::Holder => "it is for another key than the one that the channel proved",
            Refused::Signer => "its signer is not a key that this node takes",
            Refused::Signature => "its signature does not verify",
            Refused::Life => "its life is not 1 to 30 s",
            Refused::Early => "it was made later than now and the skew",
            Refused::Expired => "its life ended",
            Refused::Binding => "it is for another channel",
            Refused::Replayed => "it was used already",
            Refused::Full => "the record of bearers is full",
        };
        write!(f, "a bearer refused ({}): {why}", self.name())
    }
}

impl std::error::Error for Refused {}

/// A bearer that let an operator in: what the node's line says of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub signer: PublicKey,
    pub iat: u64,
    pub exp: u64,
}

impl fmt::Display for Verified {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the bearer of {} made at {}, used now", self.signer, self.iat)
    }
}

/// A node's verifier of bearers: the record of each `jti` for its life.
pub struct Bearers {
    seen: Mutex<HashMap<[u8; 32], u64>>,
    limit: usize,
}

/// The count of the record, never a `jti`.
impl fmt::Debug for Bearers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let held = self.seen.lock().unwrap_or_else(PoisonError::into_inner).len();
        f.debug_struct("Bearers").field("held", &held).field("limit", &self.limit).finish()
    }
}

impl Default for Bearers {
    fn default() -> Bearers {
        Bearers::with_limit(SEEN)
    }
}

impl Bearers {
    /// A verifier whose record holds at most `limit` live entries.
    pub fn with_limit(limit: usize) -> Bearers {
        Bearers { seen: Mutex::default(), limit }
    }

    /// Check the bearer `blob` of the channel of `binding`, whose operator
    /// key the channel proved is `holder`, at `now` (seconds since the
    /// epoch); `signers` says whether the node takes a signer's key. `Ok`
    /// once: its `jti` is recorded until its life and the skew end.
    pub fn verify(
        &self,
        blob: &[u8],
        binding: Option<&Binding>,
        holder: &PublicKey,
        signers: impl Fn(&PublicKey) -> bool,
        now: u64,
    ) -> Result<Verified, Refused> {
        let binding = binding.ok_or(Refused::Unbound)?;
        let bearer = Bearer::read(blob)?;
        if bearer.holder != *holder {
            return Err(Refused::Holder);
        }
        if !signers(&bearer.signer) {
            return Err(Refused::Signer);
        }
        if !bearer.signer.verifies(&bearer.signed, &bearer.signature) {
            return Err(Refused::Signature);
        }
        if bearer.exp <= bearer.iat || bearer.exp - bearer.iat > LIFE {
            return Err(Refused::Life);
        }
        if bearer.iat > now.saturating_add(SKEW) {
            return Err(Refused::Early);
        }
        let until = bearer.exp.saturating_add(SKEW);
        if now >= until {
            return Err(Refused::Expired);
        }
        if !pod_core::entropy::same_secret(&jti(binding, &bearer.nonce), &bearer.jti) {
            return Err(Refused::Binding);
        }
        self.record(bearer.jti, until, now)?;
        Ok(Verified { signer: bearer.signer, iat: bearer.iat, exp: bearer.exp })
    }

    /// Record `jti` until `until`, once; the entries whose end passed go
    /// first.
    fn record(&self, jti: [u8; 32], until: u64, now: u64) -> Result<(), Refused> {
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.retain(|_, end| *end > now);
        if seen.contains_key(&jti) {
            return Err(Refused::Replayed);
        }
        if seen.len() >= self.limit {
            return Err(Refused::Full);
        }
        seen.insert(jti, until);
        Ok(())
    }
}

#[cfg(test)]
#[path = "bearer_tests.rs"]
mod tests;
