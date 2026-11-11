//! The session of a finished handshake: a key for each way, the indexes of
//! both sides, the counter of the messages out, and the window of the
//! counters in.

use std::time::Instant;

use zeroize::Zeroize;

use crate::Error;
use crate::crypto::{open, seal};
use crate::handshake::MSG_DATA;

/// The keys of a session: one way each.
pub struct Session {
    pub send_key: [u8; 32],
    pub recv_key: [u8; 32],
    /// The index of this side (the receiver of the messages to it).
    pub local: u32,
    /// The index of the peer (the receiver of the messages to the peer).
    pub remote: u32,
    pub send_counter: u64,
    pub created: Instant,
    replay: Replay,
}

impl Drop for Session {
    /// The keys do not stay in freed memory.
    fn drop(&mut self) {
        self.send_key.zeroize();
        self.recv_key.zeroize();
    }
}

impl std::fmt::Debug for Session {
    /// No byte of a key: a debug line can go to a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("local", &self.local)
            .field("remote", &self.remote)
            .field("send_counter", &self.send_counter)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub(crate) fn new(send_key: [u8; 32], recv_key: [u8; 32], local: u32, remote: u32, created: Instant) -> Session {
        Session { send_key, recv_key, local, remote, send_counter: 0, created, replay: Replay::default() }
    }

    /// A transport message with `packet` (padded to 16 bytes; an empty
    /// packet is a keepalive).
    pub fn seal(&mut self, reserved: &[u8; 3], packet: &[u8]) -> Vec<u8> {
        let counter = self.send_counter;
        self.send_counter += 1;
        let mut plain = packet.to_vec();
        plain.resize(packet.len().div_ceil(16) * 16, 0);
        let mut m = Vec::with_capacity(16 + plain.len() + 16);
        m.push(MSG_DATA);
        m.extend_from_slice(reserved);
        m.extend_from_slice(&self.remote.to_le_bytes());
        m.extend_from_slice(&counter.to_le_bytes());
        m.extend_from_slice(&seal(&self.send_key, counter, &plain, &[]));
        m
    }

    /// The IP packet of a transport message to this session: the padding is
    /// cut by the length in the IP header. An empty vector is a keepalive.
    pub fn open(&mut self, message: &[u8]) -> Result<Vec<u8>, Error> {
        if message.len() < 32 || message[0] != MSG_DATA {
            return Err(Error::NotTransport);
        }
        let receiver = u32::from_le_bytes(message[4..8].try_into().expect("4 bytes"));
        if receiver != self.local {
            return Err(Error::OtherSession);
        }
        let counter = u64::from_le_bytes(message[8..16].try_into().expect("8 bytes"));
        if !self.replay.check(counter) {
            return Err(Error::Replayed(counter));
        }
        let mut plain = open(&self.recv_key, counter, &message[16..], &[]).ok_or(Error::NotAuthentic)?;
        self.replay.mark(counter);
        let len = match plain.first().map(|b| b >> 4) {
            Some(4) if plain.len() >= 20 => u16::from_be_bytes([plain[2], plain[3]]) as usize,
            Some(6) if plain.len() >= 40 => 40 + u16::from_be_bytes([plain[4], plain[5]]) as usize,
            _ => 0,
        };
        plain.truncate(len.min(plain.len()));
        Ok(plain)
    }

    /// The receiver index of a message: to find its session.
    pub fn receiver_of(message: &[u8]) -> Option<u32> {
        Some(u32::from_le_bytes(message.get(4..8)?.try_into().ok()?))
    }
}

/// A sliding window of the counters that came (RFC 6479 shape, 2048 bits).
#[derive(Default)]
struct Replay {
    top: u64,
    bits: [u64; 32],
    any: bool,
}

impl Replay {
    const WINDOW: u64 = 2048 - 64;

    fn check(&self, n: u64) -> bool {
        if !self.any || n > self.top {
            return true;
        }
        if self.top - n >= Self::WINDOW {
            return false;
        }
        let bit = n % 2048;
        self.bits[(bit / 64) as usize] & (1 << (bit % 64)) == 0
    }

    fn mark(&mut self, n: u64) {
        if !self.any || n > self.top {
            let from = if self.any { self.top + 1 } else { n };
            if n - from >= 2048 {
                self.bits = [0; 32];
            } else {
                for i in from..=n {
                    let bit = i % 2048;
                    self.bits[(bit / 64) as usize] &= !(1 << (bit % 64));
                }
            }
            self.top = n;
            self.any = true;
        }
        let bit = n % 2048;
        self.bits[(bit / 64) as usize] |= 1 << (bit % 64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replay_window() {
        let mut r = Replay::default();
        for n in [0u64, 1, 5, 3] {
            assert!(r.check(n), "{n}");
            r.mark(n);
            assert!(!r.check(n), "{n} again");
        }
        assert!(r.check(4));
        r.mark(5000);
        assert!(!r.check(5));
        assert!(r.check(4999));
    }
}
