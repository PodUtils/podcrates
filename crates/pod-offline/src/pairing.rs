//! Pairing by an eight-word code, single-use, bound to an Ed25519 key.
//!
//! The giver makes an [`Offer`]: its key, the hash of the code and an
//! expiry, signed by that key. The taker checks the offer with the code,
//! uses the code once ([`UsedCodes`]), and makes a [`Paired`] record. The
//! record names the taker's key as `identity` and the giver's key as
//! `peer`, and the `proof` is the taker's signature over the peer key and
//! the code hash. A verifier checks the proof under `identity`: the paired
//! key is inside the signed words, not beside them.
//!
//! The words of the code are 48 bits (eight words of 64). The caller draws
//! the entropy; the crate has no source of it. A code is a credential: the
//! `Debug` of [`Code`] shows no word.
//!
//! The broadcast hash is PBKDF2-HMAC-SHA256 of the words with the salt
//! [`KDF_SALT`] and [`KDF_ITERATIONS`] iterations, so an offer that a
//! stranger receives does not permit a cheap offline search of the code.
//! An offer names an expiry, and the giver's [`Giver`] records each code
//! once: a second claim is [`Error::CodeUsed`] and an offer past its
//! expiry is [`Error::CodeExpired`].

use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pod_identity::{Identity, PublicKey};
use sha2::{Digest, Sha256};

use crate::Error;
use crate::hex;

/// The eight words of one code.
pub const WORDS_COUNT: usize = 8;

/// The entropy of a code: six bits for each of its eight words.
pub const CODE_BITS: usize = WORDS_COUNT * 6;

/// The salt of the code's slow hash.
pub const KDF_SALT: &[u8] = b"pod-offline pair code kdf 1";

/// The iterations of PBKDF2-HMAC-SHA256 for a code's hash.
pub const KDF_ITERATIONS: u32 = 100_000;

/// The lifetime of an offer by default: ten minutes.
pub const OFFER_WINDOW: Duration = Duration::from_secs(600);

/// The list of the words of a code.
pub const WORDS: &[&str; 64] = &[
    "amber", "anchor", "apple", "arrow", "badge", "bamboo", "basket", "bison", "blade", "bottle", "brick", "bridge",
    "bronze", "bucket", "butter", "cactus", "candle", "canyon", "carpet", "castle", "cedar", "cherry", "cinder",
    "citrus", "clover", "cobalt", "copper", "coral", "cotton", "crayon", "crystal", "daisy", "dolphin", "dragon",
    "ember", "falcon", "feather", "fern", "flint", "forest", "garnet", "ginger", "glacier", "granite", "harbor",
    "hazel", "heron", "honey", "indigo", "island", "ivory", "jasper", "jungle", "kettle", "ladder", "lantern", "lemon",
    "lilac", "magnet", "maple", "marble", "meadow", "melon", "meteor",
];

const CODE_LABEL: &[u8] = b"pod-offline pair code 2\n";
const OFFER_LABEL: &[u8] = b"pod-offline pair offer 2\n";
const PAIR_LABEL: &[u8] = b"pod-offline pair record 2\n";
const OFFER_FORM: &str = "pod-offline pair offer 2";
const PAIRED_FORM: &str = "pod-offline pair record 2";

/// Eight words of the list, held as their places.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Code([u8; WORDS_COUNT]);

impl Code {
    /// The code of eight bytes, each taken modulo the list.
    pub fn from_entropy(bytes: &[u8; WORDS_COUNT]) -> Code {
        Code(std::array::from_fn(|at| bytes[at] & 0x3f))
    }

    /// The code of eight words with hyphens between them.
    pub fn parse(text: &str) -> Result<Code, Error> {
        let parts: Vec<&str> = text.split('-').collect();
        if parts.len() != WORDS_COUNT {
            return Err(Error::Code { why: format!("{text:?} is not eight words") });
        }
        let mut places = [0u8; WORDS_COUNT];
        for (place, word) in parts.iter().enumerate() {
            let found = WORDS
                .iter()
                .position(|known| known.eq_ignore_ascii_case(word))
                .ok_or_else(|| Error::Code { why: format!("the word {word:?} is not in the list") })?;
            places[place] = found as u8;
        }
        Ok(Code(places))
    }

    /// The words, with hyphens between them.
    pub fn text(&self) -> String {
        self.0.map(|place| WORDS[usize::from(place)]).join("-")
    }

    /// The slow hash of the words. It is the name of the code in the store,
    /// and it never goes into a log.
    pub fn hash(&self) -> [u8; 32] {
        let mut words = Vec::with_capacity(CODE_LABEL.len() + WORDS_COUNT);
        words.extend_from_slice(CODE_LABEL);
        words.extend_from_slice(&self.0);
        pbkdf2_sha256(&words, KDF_SALT, KDF_ITERATIONS)
    }
}

/// HMAC-SHA-256, for the code's PBKDF2.
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    let mut outer = Sha256::new();
    for byte in block {
        inner.update([byte ^ 0x36]);
        outer.update([byte ^ 0x5c]);
    }
    inner.update(data);
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// PBKDF2-HMAC-SHA-256 with one 32-byte output block.
fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut first = salt.to_vec();
    first.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac_sha256(password, &first);
    let mut out = u;
    for _ in 1..iterations {
        u = hmac_sha256(password, &u);
        for (a, b) in out.iter_mut().zip(u.iter()) {
            *a ^= *b;
        }
    }
    out
}

impl fmt::Debug for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Code(<eight words hidden>)")
    }
}

/// The giver's announcement of a code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub identity: PublicKey,
    pub code_hash: [u8; 32],
    /// The unix second at which the offer was made.
    pub issued: u64,
    /// The unix second after which the offer is not taken.
    pub expires: u64,
    pub signature: [u8; 64],
}

impl Offer {
    /// Sign the hash of `code` with `identity`, inside the default window.
    pub fn make(identity: &Identity, code: &Code) -> Offer {
        Offer::make_at(identity, code, unix_now(), OFFER_WINDOW)
    }

    /// Sign the hash of `code` with `identity` at `issued`, for `window`.
    pub fn make_at(identity: &Identity, code: &Code, issued: u64, window: Duration) -> Offer {
        let code_hash = code.hash();
        let expires = issued.saturating_add(window.as_secs());
        Offer {
            identity: identity.public(),
            code_hash,
            issued,
            expires,
            signature: identity.sign(&offer_message(&code_hash, issued, expires)),
        }
    }

    /// Whether the key signed this hash and window, and the hash is the
    /// code's.
    pub fn verify(&self, code: &Code) -> bool {
        self.code_hash == code.hash()
            && self.identity.verifies(&offer_message(&self.code_hash, self.issued, self.expires), &self.signature)
    }

    /// Whether `now` is at or past the offer's expiry.
    pub fn expired(&self, now: u64) -> bool {
        now >= self.expires
    }

    /// The text of the offer, for a carrier.
    pub fn encode(&self) -> Vec<u8> {
        format!(
            "{OFFER_FORM}\nkey: {}\ncode: {}\nissued: {}\nexpires: {}\nproof: {}\n",
            self.identity.hex(),
            hex::encode(&self.code_hash),
            self.issued,
            self.expires,
            hex::encode(&self.signature)
        )
        .into_bytes()
    }

    /// Read the text of an offer. A signature is checked by
    /// [`Offer::verify`], with the code.
    pub fn decode(bytes: &[u8]) -> Result<Offer, Error> {
        let fields = fields(bytes, OFFER_FORM)?;
        let identity = field(&fields, "key").and_then(PublicKey::parse).ok_or(Error::Proof)?;
        let code_hash = field(&fields, "code")
            .and_then(hex::decode)
            .and_then(|raw| <[u8; 32]>::try_from(raw).ok())
            .ok_or(Error::Proof)?;
        let issued = field(&fields, "issued").and_then(|value| value.parse().ok()).ok_or(Error::Proof)?;
        let expires = field(&fields, "expires").and_then(|value| value.parse().ok()).ok_or(Error::Proof)?;
        let signature = field(&fields, "proof")
            .and_then(hex::decode)
            .and_then(|raw| <[u8; 64]>::try_from(raw).ok())
            .ok_or(Error::Proof)?;
        Ok(Offer { identity, code_hash, issued, expires, signature })
    }
}

/// The record that binds the paired key to the identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    pub identity: PublicKey,
    pub peer: PublicKey,
    pub proof: [u8; 64],
}

impl Paired {
    /// Whether `identity` signed `peer` and the hash of `code`.
    pub fn verify(&self, code: &Code) -> bool {
        self.identity.verifies(&pair_message(&self.peer, &code.hash()), &self.proof)
    }

    pub fn encode(&self) -> Vec<u8> {
        format!(
            "{PAIRED_FORM}\nkey: {}\npeer: {}\nproof: {}\n",
            self.identity.hex(),
            self.peer.hex(),
            hex::encode(&self.proof)
        )
        .into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Paired, Error> {
        let fields = fields(bytes, PAIRED_FORM)?;
        let identity = field(&fields, "key").and_then(PublicKey::parse).ok_or(Error::Proof)?;
        let peer = field(&fields, "peer").and_then(PublicKey::parse).ok_or(Error::Proof)?;
        let proof = field(&fields, "proof")
            .and_then(hex::decode)
            .and_then(|raw| <[u8; 64]>::try_from(raw).ok())
            .ok_or(Error::Proof)?;
        Ok(Paired { identity, peer, proof })
    }
}

/// The place where a code is used once. Each code is one file, made with
/// `create_new`: two processes cannot use it.
#[derive(Debug, Clone)]
pub struct UsedCodes {
    dir: PathBuf,
}

impl UsedCodes {
    pub fn open(dir: impl Into<PathBuf>) -> Result<UsedCodes, Error> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).map_err(|e| Error::file(&dir, e))?;
        Ok(UsedCodes { dir })
    }

    /// Use `hash` once. A second use is [`Error::CodeUsed`].
    pub fn claim(&self, hash: &[u8; 32]) -> Result<(), Error> {
        let path = self.dir.join(hex::encode(hash));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(format!("{}\n", hex::encode(hash)).as_bytes()).map_err(|e| Error::file(&path, e))?;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(Error::CodeUsed),
            Err(e) => Err(Error::file(&path, e)),
        }
    }

    /// Whether `hash` was used.
    pub fn used(&self, hash: &[u8; 32]) -> bool {
        self.dir.join(hex::encode(hash)).exists()
    }
}

/// Take the offer with `code` as `me`. The code is used once, and the
/// record binds the giver's key to the key of `me`.
pub fn take(me: &Identity, store: &UsedCodes, offer: &Offer, code: &Code) -> Result<Paired, Error> {
    take_at(me, store, offer, code, unix_now())
}

/// [`take`] at the clock `now`, for a test of the window.
pub fn take_at(me: &Identity, store: &UsedCodes, offer: &Offer, code: &Code, now: u64) -> Result<Paired, Error> {
    if offer.expired(now) {
        return Err(Error::CodeExpired);
    }
    if !offer.verify(code) {
        return Err(Error::Proof);
    }
    store.claim(&code.hash())?;
    Ok(Paired {
        identity: me.public(),
        peer: offer.identity,
        proof: me.sign(&pair_message(&offer.identity, &code.hash())),
    })
}

/// The giver's record of the offers that it made: each code is issued once
/// and claimed once, and an offer is taken only inside its window.
#[derive(Debug, Clone)]
pub struct Giver {
    dir: PathBuf,
    window: Duration,
}

impl Giver {
    /// Open a giver whose offers live for `window`, in `dir`.
    pub fn open(dir: impl Into<PathBuf>, window: Duration) -> Result<Giver, Error> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).map_err(|e| Error::file(&dir, e))?;
        Ok(Giver { dir, window })
    }

    /// Make an offer for `code` and record it once with its expiry.
    pub fn offer(&self, identity: &Identity, code: &Code) -> Result<Offer, Error> {
        let offer = Offer::make_at(identity, code, unix_now(), self.window);
        let path = self.dir.join(hex::encode(&offer.code_hash));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let text =
                    format!("key: {}\nissued: {}\nexpires: {}\n", offer.identity.hex(), offer.issued, offer.expires);
                file.write_all(text.as_bytes()).map_err(|e| Error::file(&path, e))?;
                Ok(offer)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(Error::CodeUsed),
            Err(e) => Err(Error::file(&path, e)),
        }
    }

    /// Take the [`Paired`] record of an offer that this giver made. The
    /// record must verify, the offer must be inside its window, and the
    /// code is claimed once.
    pub fn accept(&self, code: &Code, paired: &Paired, now: u64) -> Result<(), Error> {
        let hash = code.hash();
        let path = self.dir.join(hex::encode(&hash));
        let text = std::fs::read_to_string(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::Proof,
            _ => Error::file(&path, e),
        })?;
        let fields: Vec<(&str, &str)> = text.lines().filter_map(|line| line.split_once(": ")).collect();
        let key = field(&fields, "key").and_then(PublicKey::parse).ok_or(Error::Proof)?;
        let expires: u64 = field(&fields, "expires").and_then(|value| value.parse().ok()).ok_or(Error::Proof)?;
        if now >= expires {
            return Err(Error::CodeExpired);
        }
        if paired.peer != key || !paired.verify(code) {
            return Err(Error::Proof);
        }
        let used = self.dir.join(format!("{}.used", hex::encode(&hash)));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&used) {
            Ok(mut file) => {
                file.write_all(format!("{}\n", hex::encode(&hash)).as_bytes()).map_err(|e| Error::file(&used, e))?;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(Error::CodeUsed),
            Err(e) => Err(Error::file(&used, e)),
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|since| since.as_secs()).unwrap_or(0)
}

fn offer_message(code_hash: &[u8; 32], issued: u64, expires: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(OFFER_LABEL.len() + 32 + 16);
    out.extend_from_slice(OFFER_LABEL);
    out.extend_from_slice(code_hash);
    out.extend_from_slice(&issued.to_be_bytes());
    out.extend_from_slice(&expires.to_be_bytes());
    out
}

fn pair_message(peer: &PublicKey, code_hash: &[u8; 32]) -> Vec<u8> {
    [PAIR_LABEL, &peer.0, code_hash].concat()
}

fn fields<'a>(bytes: &'a [u8], form: &str) -> Result<Vec<(&'a str, &'a str)>, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Proof)?;
    let mut lines = text.lines();
    if lines.next() != Some(form) {
        return Err(Error::Proof);
    }
    let mut out = Vec::new();
    for line in lines {
        if let Some((key, value)) = line.split_once(": ") {
            out.push((key, value));
        } else {
            return Err(Error::Proof);
        }
    }
    Ok(out)
}

fn field<'a>(fields: &[(&'a str, &'a str)], name: &str) -> Option<&'a str> {
    fields.iter().find(|(key, _)| *key == name).map(|(_, value)| *value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_keeps_its_eight_words_and_hides_them_in_debug() {
        let code = Code::from_entropy(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let text = code.text();
        assert_eq!(Code::parse(&text).unwrap(), code);
        assert_eq!(text.split('-').count(), WORDS_COUNT);
        assert!(!format!("{code:?}").contains(WORDS[1]));
        assert_eq!(Code::parse("amber-anchor-apple").unwrap_err().name(), "code");
        assert_eq!(Code::parse("amber-anchor-apple-nope").unwrap_err().name(), "code");
    }

    #[test]
    fn the_words_are_unique() {
        for (at, word) in WORDS.iter().enumerate() {
            assert_eq!(WORDS.iter().filter(|other| *other == word).count(), 1, "{at}");
        }
    }

    #[test]
    fn a_code_hashes_differently_from_another() {
        assert_ne!(Code::from_entropy(&[0; WORDS_COUNT]).hash(), Code::from_entropy(&[1; WORDS_COUNT]).hash());
    }
}
