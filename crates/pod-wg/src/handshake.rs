//! The handshake: Noise IKpsk2 with no pre-shared key (the WireGuard paper,
//! section 5.4, and wireguard.com/protocol). This side is the initiator: it
//! sends an initiation, and a response of the peer gives the [`Session`].
//!
//! WARP reads bytes 1 to 3 of each message as the `client_id` of a device
//! (`Keys::reserved`). It writes them after the MAC, and its front door
//! checks the MAC with them zero, as WireGuard has them.

use std::time::{Instant, SystemTime};

use pod_core::entropy::{Entropy, OsEntropy};
use zeroize::Zeroize;

use crate::Error;
use crate::blake2s::{hash, kdf, mac};
use crate::cookie::Cookie;
use crate::crypto::{open, random, seal, tai64n, x25519, x25519_base};
use crate::session::Session;

pub(crate) const CONSTRUCTION: &[u8] = b"Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s";
pub(crate) const IDENTIFIER: &[u8] = b"WireGuard v1 zx2c4 Jason@zx2c4.com";
pub(crate) const LABEL_MAC1: &[u8] = b"mac1----";

/// The type byte of each message.
pub const MSG_INITIATION: u8 = 1;
pub const MSG_RESPONSE: u8 = 2;
pub const MSG_COOKIE: u8 = 3;
pub const MSG_DATA: u8 = 4;

/// The length of an initiation and of a response.
pub const INITIATION_LEN: usize = 148;
pub const RESPONSE_LEN: usize = 92;

/// The keys of a device and of its peer.
#[derive(Clone)]
pub struct Keys {
    pub private: [u8; 32],
    pub peer_public: [u8; 32],
    /// Bytes 1 to 3 of each message (WARP: the `client_id`).
    pub reserved: [u8; 3],
}

impl Keys {
    pub fn public(&self) -> [u8; 32] {
        x25519_base(&self.private)
    }
}

impl std::fmt::Debug for Keys {
    /// No byte of a key: a debug line can go to a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys { .. }")
    }
}

/// An initiation that waits for its response.
pub struct Initiation {
    chaining: [u8; 32],
    hash: [u8; 32],
    ephemeral: [u8; 32],
    sender: u32,
    /// The message to send.
    pub message: Vec<u8>,
}

impl Drop for Initiation {
    /// The keys of the handshake do not stay in freed memory.
    fn drop(&mut self) {
        self.chaining.zeroize();
        self.hash.zeroize();
        self.ephemeral.zeroize();
    }
}

/// The mac1 of a message, over its bytes with the reserved bytes (1 to 3)
/// zero.
pub(crate) fn mac1(peer_public: &[u8; 32], message: &[u8]) -> [u8; 16] {
    let key = hash(&[LABEL_MAC1, peer_public]);
    let mut zeroed = message.to_vec();
    if zeroed.len() >= 4 {
        zeroed[1..4].fill(0);
    }
    mac(&key, &[&zeroed])
}

/// Make the initiation of a handshake: random bytes of the system, and the
/// timestamp of this host's wall clock now.
pub fn initiate(keys: &Keys) -> Result<Initiation, Error> {
    initiate_from(keys, &mut OsEntropy, tai64n(SystemTime::now()))
}

/// Make the initiation of a handshake with the ephemeral key and the index
/// from `entropy`, and `timestamp` (TAI64N; [`crate::Stamps`] gives one
/// that never goes back).
pub fn initiate_from(keys: &Keys, entropy: &mut dyn Entropy, timestamp: [u8; 12]) -> Result<Initiation, Error> {
    let ephemeral = random::<32>(entropy)?;
    let sender = u32::from_le_bytes(random::<4>(entropy)?);
    Ok(initiate_with(keys, ephemeral, sender, timestamp))
}

/// Make the initiation of a handshake with these values: the same values
/// give the same message.
pub fn initiate_with(keys: &Keys, ephemeral: [u8; 32], sender: u32, timestamp: [u8; 12]) -> Initiation {
    let mut c = hash(&[CONSTRUCTION]);
    let mut h = hash(&[&c, IDENTIFIER]);
    h = hash(&[&h, &keys.peer_public]);
    let e_pub = x25519_base(&ephemeral);
    [c] = kdf::<1>(&c, &e_pub);
    h = hash(&[&h, &e_pub]);
    let [c2, k] = kdf::<2>(&c, &x25519(&ephemeral, &keys.peer_public));
    c = c2;
    let enc_static = seal(&k, 0, &keys.public(), &h);
    h = hash(&[&h, &enc_static]);
    let [c3, k] = kdf::<2>(&c, &x25519(&keys.private, &keys.peer_public));
    c = c3;
    let enc_time = seal(&k, 0, &timestamp, &h);
    h = hash(&[&h, &enc_time]);
    let mut m = Vec::with_capacity(INITIATION_LEN);
    m.push(MSG_INITIATION);
    m.extend_from_slice(&keys.reserved);
    m.extend_from_slice(&sender.to_le_bytes());
    m.extend_from_slice(&e_pub);
    m.extend_from_slice(&enc_static);
    m.extend_from_slice(&enc_time);
    let mac = mac1(&keys.peer_public, &m);
    m.extend_from_slice(&mac);
    m.extend_from_slice(&[0u8; 16]); // mac2: no cookie
    Initiation { chaining: c, hash: h, ephemeral, sender, message: m }
}

impl Initiation {
    /// The index of this side in the handshake: the receiver of the
    /// response.
    pub fn sender(&self) -> u32 {
        self.sender
    }

    /// The mac1 of the message: a cookie reply to it is sealed with it.
    pub fn mac1(&self) -> [u8; 16] {
        self.message[116..132].try_into().expect("16 bytes")
    }

    /// Put the mac2 of `cookie` in the message when the cookie is fresh at
    /// `now`. A responder under load takes only an initiation with it.
    pub fn add_mac2(&mut self, cookie: &Cookie, now: Instant) {
        if cookie.fresh(now) {
            let mac = cookie.mac2(&self.message[..132]);
            self.message[132..].copy_from_slice(&mac);
        }
    }

    /// The session of the response of the peer, made at `now`.
    pub fn finish(&self, keys: &Keys, response: &[u8], now: Instant) -> Result<Session, Error> {
        if response.len() != RESPONSE_LEN || response[0] != MSG_RESPONSE {
            return Err(Error::NotResponse);
        }
        let remote = u32::from_le_bytes(response[4..8].try_into().expect("4 bytes"));
        let receiver = u32::from_le_bytes(response[8..12].try_into().expect("4 bytes"));
        if receiver != self.sender {
            return Err(Error::OtherHandshake);
        }
        if mac1(&keys.public(), &response[..60]) != response[60..76] {
            return Err(Error::Mac1);
        }
        let e_pub: [u8; 32] = response[12..44].try_into().expect("32 bytes");
        let empty = &response[44..60];
        let mut c = self.chaining;
        let mut h = self.hash;
        [c] = kdf::<1>(&c, &e_pub);
        h = hash(&[&h, &e_pub]);
        [c] = kdf::<1>(&c, &x25519(&self.ephemeral, &e_pub));
        [c] = kdf::<1>(&c, &x25519(&keys.private, &e_pub));
        let [c4, tau, k] = kdf::<3>(&c, &[0u8; 32]);
        c = c4;
        h = hash(&[&h, &tau]);
        open(&k, 0, empty, &h).ok_or(Error::ResponseNotAuthentic)?;
        let [send_key, recv_key] = kdf::<2>(&c, &[]);
        c.zeroize();
        Ok(Session::new(send_key, recv_key, self.sender, remote, now))
    }
}

#[cfg(test)]
mod tests;
