//! The authenticated switch after a TLS handshake (SWITCH-1 to SWITCH-4).
//!
//! The server completes a real handshake with the cover site. The first
//! application bytes then hold a proof: a timestamp, a nonce and an HMAC of
//! both, bound to the exporter of that handshake. A proof that verifies opens
//! a session. Any other bytes go to the cover site, with no close and no
//! reset (SWITCH-2).
//!
//! The server keeps each nonce for the length of the window, so a captured
//! proof does not open a second session (SWITCH-3). A proof outside the
//! window is refused. The compare is constant time.
//!
//! The proof is not a TLS record and it is not secret: it is the first bytes
//! of the application data. Give the client proof to
//! [`Switch::authenticate`] and a plain request to the site.

use std::collections::HashMap;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

/// The bytes of a proof: a timestamp, a nonce and the tag.
pub const PROOF_LEN: usize = 8 + 16 + 32;

/// The bytes of the nonce.
pub const NONCE_LEN: usize = 16;

/// The length of the window, in seconds: a proof of an instant outside it is
/// refused.
pub const WINDOW_SECS: i64 = 240;

/// The seconds of a clock that runs ahead of the server, which the server
/// still takes.
pub const SKEW_SECS: i64 = 30;

/// The most nonces of one window that a server holds. A full window refuses
/// a new proof, so an evicted nonce never opens a second session.
pub const MAX_NONCES: usize = 4096;

/// The result of the first application bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The proof is valid: the connection is a session.
    Session,
    /// The bytes are not a valid proof: give them to the cover site.
    Cover,
}

/// The authentication of the first application bytes.
pub struct Switch {
    key: Zeroizing<[u8; 32]>,
    /// The exporter of the handshake, when the caller binds one.
    exporter: Vec<u8>,
    /// Each nonce that opened a session, with the instant it expires.
    seen: HashMap<[u8; NONCE_LEN], i64>,
}

impl std::fmt::Debug for Switch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Switch")
            .field("exporter", &self.exporter.len())
            .field("seen", &self.seen.len())
            .finish_non_exhaustive()
    }
}

impl Switch {
    /// A switch with `key`. The key is the one value that both ends hold.
    pub fn new(key: [u8; 32]) -> Switch {
        Switch { key: Zeroizing::new(key), exporter: Vec::new(), seen: HashMap::new() }
    }

    /// Bind the proof to the exporter of one handshake. Both ends call this
    /// with the same bytes. A proof of another handshake then fails.
    pub fn bind(mut self, exporter: &[u8]) -> Switch {
        self.exporter.clear();
        self.exporter.extend_from_slice(exporter);
        self
    }

    /// The proof of `nonce` at `now`, for the client.
    pub fn prove(&self, nonce: [u8; NONCE_LEN], now: i64) -> Vec<u8> {
        let mut out = Vec::with_capacity(PROOF_LEN);
        out.extend_from_slice(&now.to_be_bytes());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&self.tag(now, &nonce));
        out
    }

    /// Read the first application bytes at `now`. [`Outcome::Session`] opens
    /// a session. [`Outcome::Cover`] is not a refusal of the connection: the
    /// same bytes are a request for the cover site (SWITCH-2).
    pub fn authenticate(&mut self, bytes: &[u8], now: i64) -> Outcome {
        if bytes.len() < PROOF_LEN {
            return Outcome::Cover;
        }
        let (head, _rest) = bytes.split_at(PROOF_LEN);
        let Ok(timestamp) = head[..8].try_into() else { return Outcome::Cover };
        let timestamp = i64::from_be_bytes(timestamp);
        if now.saturating_sub(timestamp) > WINDOW_SECS || timestamp.saturating_sub(now) > SKEW_SECS {
            return Outcome::Cover;
        }
        let nonce = &head[8..8 + NONCE_LEN];
        let tag = &head[8 + NONCE_LEN..];
        if self.verify_tag(timestamp, nonce, tag).is_err() {
            return Outcome::Cover;
        }
        self.seen.retain(|_, expires| *expires >= now);
        let mut nonce_bytes = [0u8; NONCE_LEN];
        nonce_bytes.copy_from_slice(nonce);
        if self.seen.contains_key(&nonce_bytes) {
            return Outcome::Cover;
        }
        if self.seen.len() >= MAX_NONCES {
            return Outcome::Cover;
        }
        self.seen.insert(nonce_bytes, timestamp.saturating_add(WINDOW_SECS));
        Outcome::Session
    }

    /// The HMAC of one proof. The exporter of the handshake is part of the
    /// message, so a proof of one connection does not fit another.
    fn tag(&self, timestamp: i64, nonce: &[u8]) -> [u8; 32] {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.key.as_ref()).expect("HMAC takes any key length");
        mac.update(b"pod-cover switch v1");
        mac.update(&timestamp.to_be_bytes());
        mac.update(nonce);
        mac.update(&(self.exporter.len() as u32).to_be_bytes());
        mac.update(&self.exporter);
        let tag = mac.finalize().into_bytes();
        let mut out = [0u8; 32];
        out.copy_from_slice(&tag);
        out
    }

    /// The tag of `tag` against the one of this key and exporter. The
    /// compare is constant time.
    fn verify_tag(&self, timestamp: i64, nonce: &[u8], tag: &[u8]) -> Result<(), ()> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.key.as_ref()).expect("HMAC takes any key length");
        mac.update(b"pod-cover switch v1");
        mac.update(&timestamp.to_be_bytes());
        mac.update(nonce);
        mac.update(&(self.exporter.len() as u32).to_be_bytes());
        mac.update(&self.exporter);
        mac.verify_slice(tag).map_err(|_| ())
    }
}
