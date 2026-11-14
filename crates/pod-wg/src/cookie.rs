//! The cookie of a responder under load (the WireGuard paper, section
//! 5.4.7). A responder under load answers an initiation with a cookie
//! reply in place of a response. The initiator opens the cookie with the
//! mac1 of its initiation, and puts the mac2 of the cookie in each
//! initiation while the cookie is fresh; the responder then takes them.
//!
//! As for mac1, mac2 is the MAC of the bytes with the reserved bytes (1 to
//! 3) zero: WARP writes them after each MAC.

use std::time::{Duration, Instant};

use zeroize::Zeroize;

use crate::Error;
use crate::blake2s::{hash, mac};
use crate::crypto::{xopen, xseal};
use crate::handshake::{Initiation, Keys, MSG_COOKIE};

/// The length of a cookie reply.
pub const COOKIE_REPLY_LEN: usize = 64;

/// The time that a cookie is good for: the age of the secret of the
/// responder, less the latency that the protocol allows (120 s and 5 s, as
/// the Linux kernel has them).
pub const COOKIE_AGE: Duration = Duration::from_secs(120 - 5);

pub(crate) const LABEL_COOKIE: &[u8] = b"cookie--";

/// The key of the cookie of the responder with this static key.
pub(crate) fn cookie_key(responder_public: &[u8; 32]) -> [u8; 32] {
    hash(&[LABEL_COOKIE, responder_public])
}

/// The mac2 of a message with `cookie`, over its bytes up to mac2, with
/// the reserved bytes zero.
pub(crate) fn mac2(cookie: &[u8; 16], message: &[u8]) -> [u8; 16] {
    let mut zeroed = message.to_vec();
    if zeroed.len() >= 4 {
        zeroed[1..4].fill(0);
    }
    mac(cookie, &[&zeroed])
}

/// The cookie reply to the initiation `init`: `cookie` sealed with `key`
/// (the [`cookie_key`] of the responder), under `nonce`, with the mac1 of
/// `init` as its associated data.
pub(crate) fn reply(key: &[u8; 32], init: &[u8], cookie: &[u8; 16], nonce: &[u8; 24]) -> Vec<u8> {
    let mut m = Vec::with_capacity(COOKIE_REPLY_LEN);
    m.extend_from_slice(&[MSG_COOKIE, 0, 0, 0]);
    m.extend_from_slice(&init[4..8]);
    m.extend_from_slice(nonce);
    m.extend_from_slice(&xseal(key, nonce, cookie, &init[116..132]));
    m
}

/// The cookie of a reply, and the time it came.
#[derive(Clone)]
pub struct Cookie {
    value: [u8; 16],
    received: Instant,
}

impl Drop for Cookie {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl std::fmt::Debug for Cookie {
    /// No byte of the cookie: a debug line can go to a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cookie").field("received", &self.received).finish_non_exhaustive()
    }
}

impl Cookie {
    /// The cookie of `reply`, an answer to `init` from the peer of `keys`,
    /// received at `now`.
    pub fn open(keys: &Keys, init: &Initiation, reply: &[u8], now: Instant) -> Result<Cookie, Error> {
        if reply.len() != COOKIE_REPLY_LEN || reply[0] != MSG_COOKIE {
            return Err(Error::NotCookie);
        }
        let receiver = u32::from_le_bytes(reply[4..8].try_into().expect("4 bytes"));
        if receiver != init.sender() {
            return Err(Error::OtherHandshake);
        }
        let nonce: [u8; 24] = reply[8..32].try_into().expect("24 bytes");
        let plain = xopen(&cookie_key(&keys.peer_public), &nonce, &reply[32..], &init.mac1())
            .ok_or(Error::CookieNotAuthentic)?;
        let value: [u8; 16] = plain.as_slice().try_into().expect("a sealed cookie of 16 bytes");
        Ok(Cookie { value, received: now })
    }

    /// True while the cookie is good at `now`.
    pub fn fresh(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.received) < COOKIE_AGE
    }

    pub(crate) fn mac2(&self, message: &[u8]) -> [u8; 16] {
        mac2(&self.value, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handshake::{INITIATION_LEN, initiate_with};

    fn keys() -> Keys {
        Keys {
            private: [1; 32],
            peer_public: crate::crypto::x25519_base(&[2; 32]),
            reserved: [7, 8, 9],
            preshared: None,
        }
    }

    /// The cookie of a reply opens with the mac1 of its initiation; a
    /// fresh cookie gives mac2 to the next initiation; another initiation,
    /// a changed byte or another type do not open.
    #[test]
    fn a_cookie_reply_gives_mac2_to_the_next_initiation() {
        let keys = keys();
        let at = Instant::now();
        let init = initiate_with(&keys, [3; 32], 11, [0; 12]);
        let reply = crate::standin::cookie_reply(&keys.peer_public, &init.message, &[5; 16], &[6; 24]);
        assert_eq!(reply.len(), COOKIE_REPLY_LEN);
        let cookie = Cookie::open(&keys, &init, &reply, at).unwrap();
        assert!(cookie.fresh(at + COOKIE_AGE - Duration::from_secs(1)));
        assert!(!cookie.fresh(at + COOKIE_AGE));

        let mut next = initiate_with(&keys, [4; 32], 12, [0; 12]);
        assert_eq!(next.message[132..], [0; 16], "no cookie, no mac2");
        next.add_mac2(&cookie, at);
        assert_eq!(next.message.len(), INITIATION_LEN);
        assert_eq!(next.message[132..], mac2(&[5; 16], &next.message[..132]));
        // mac2 is over the bytes with zero reserved bytes.
        let mut zeroed = next.message[..132].to_vec();
        zeroed[1..4].fill(0);
        assert_eq!(next.message[132..], mac(&[5; 16], &[&zeroed]));
        // A stale cookie gives no mac2.
        let mut late = initiate_with(&keys, [4; 32], 13, [0; 12]);
        late.add_mac2(&cookie, at + COOKIE_AGE);
        assert_eq!(late.message[132..], [0; 16]);

        let other = initiate_with(&keys, [3; 32], 99, [0; 12]);
        assert!(matches!(Cookie::open(&keys, &other, &reply, at), Err(Error::OtherHandshake)));
        let mut bad = reply.clone();
        bad[40] ^= 1;
        assert!(matches!(Cookie::open(&keys, &init, &bad, at), Err(Error::CookieNotAuthentic)));
        let mut bad = reply.clone();
        bad[0] = 2;
        assert!(matches!(Cookie::open(&keys, &init, &bad, at), Err(Error::NotCookie)));
        assert!(matches!(Cookie::open(&keys, &init, &reply[..63], at), Err(Error::NotCookie)));
        // A reply for the mac1 of another initiation with the same index.
        let same_index = initiate_with(&keys, [9; 32], 11, [0; 12]);
        assert!(matches!(Cookie::open(&keys, &same_index, &reply, at), Err(Error::CookieNotAuthentic)));
    }
}
