//! The steps of the handshake on the side of the responder (the WireGuard
//! paper, sections 5.4.2 and 5.4.3): read an initiation, then make the
//! response. [`crate::Responder`] and the stand-in both use them.

use zeroize::Zeroize;

use crate::blake2s::{hash, kdf};
use crate::crypto::{open, seal, x25519, x25519_base};
use crate::handshake::{CONSTRUCTION, IDENTIFIER, MSG_RESPONSE, RESPONSE_LEN, mac1};

/// An initiation read up to the static key of the initiator.
pub(crate) struct Opened {
    chaining: [u8; 32],
    hash: [u8; 32],
    /// The ephemeral key of the initiator.
    pub(crate) ephemeral: [u8; 32],
    /// The static key of the initiator, as the initiation gives it.
    pub(crate) initiator: [u8; 32],
    /// The index of the initiator: the receiver of the response.
    pub(crate) sender: u32,
}

impl Drop for Opened {
    /// The keys of the handshake do not stay in freed memory.
    fn drop(&mut self) {
        self.chaining.zeroize();
        self.hash.zeroize();
    }
}

/// Read the static key of the initiator from `init` (148 bytes; its mac1 is
/// checked before). `None` when it does not open.
pub(crate) fn open_static(private: &[u8; 32], init: &[u8]) -> Option<Opened> {
    let public = x25519_base(private);
    let mut c = hash(&[CONSTRUCTION]);
    let mut h = hash(&[&c, IDENTIFIER]);
    h = hash(&[&h, &public]);
    let ephemeral: [u8; 32] = init[8..40].try_into().expect("32 bytes");
    [c] = kdf::<1>(&c, &ephemeral);
    h = hash(&[&h, &ephemeral]);
    let [c2, mut k] = kdf::<2>(&c, &x25519(private, &ephemeral));
    c = c2;
    let initiator = open(&k, 0, &init[40..88], &h);
    k.zeroize();
    let initiator: [u8; 32] = initiator?.try_into().ok()?;
    h = hash(&[&h, &init[40..88]]);
    let sender = u32::from_le_bytes(init[4..8].try_into().expect("4 bytes"));
    Some(Opened { chaining: c, hash: h, ephemeral, initiator, sender })
}

impl Opened {
    /// Read the timestamp with the static key `peer` (the key of the list,
    /// or any key when the list has none: the same work, so that the time
    /// of an answer does not tell a stranger which keys are listed).
    pub(crate) fn timestamp(&mut self, private: &[u8; 32], peer: &[u8; 32], init: &[u8]) -> Option<[u8; 12]> {
        let [c3, mut k] = kdf::<2>(&self.chaining, &x25519(private, peer));
        self.chaining = c3;
        let time = open(&k, 0, &init[88..116], &self.hash);
        k.zeroize();
        self.hash = hash(&[&self.hash, &init[88..116]]);
        time?.try_into().ok()
    }

    /// The response with the ephemeral key `e_priv`, the index `sender` of
    /// the responder and the preshared key `psk` (zero for none): the
    /// message, the send key and the receive key of the responder.
    pub(crate) fn respond(
        &self,
        peer: &[u8; 32],
        psk: &[u8; 32],
        e_priv: &[u8; 32],
        sender: u32,
    ) -> (Vec<u8>, [u8; 32], [u8; 32]) {
        let mut c = self.chaining;
        let mut h = self.hash;
        let e_r = x25519_base(e_priv);
        [c] = kdf::<1>(&c, &e_r);
        h = hash(&[&h, &e_r]);
        [c] = kdf::<1>(&c, &x25519(e_priv, &self.ephemeral));
        [c] = kdf::<1>(&c, &x25519(e_priv, peer));
        let [c4, tau, mut k] = kdf::<3>(&c, psk);
        c = c4;
        h = hash(&[&h, &tau]);
        let empty = seal(&k, 0, &[], &h);
        k.zeroize();
        let mut m = Vec::with_capacity(RESPONSE_LEN);
        m.extend_from_slice(&[MSG_RESPONSE, 0, 0, 0]);
        m.extend_from_slice(&sender.to_le_bytes());
        m.extend_from_slice(&self.sender.to_le_bytes());
        m.extend_from_slice(&e_r);
        m.extend_from_slice(&empty);
        let mac = mac1(peer, &m);
        m.extend_from_slice(&mac);
        m.extend_from_slice(&[0; 16]);
        let [recv_key, send_key] = kdf::<2>(&c, &[]);
        c.zeroize();
        (m, send_key, recv_key)
    }
}

/// True when `a` and `b` are equal, in a time that does not depend on
/// where they differ: a MAC or a key of a stranger is not compared byte
/// by byte.
pub(crate) fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && std::hint::black_box(a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y))) == 0
}
