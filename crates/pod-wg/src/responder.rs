//! The responder, sans I/O: a private key and a list of peers, each with
//! its sessions. The caller gives each datagram with its source and the
//! time `now` of its monotonic clock, and sends the bytes that come back;
//! [`Responder::tick`] gives the keepalives that are due. The responder
//! never sends an initiation: each rekey is the initiator's (the WireGuard
//! paper, section 6.2).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use pod_core::entropy::{Entropy, OsEntropy};
use zeroize::Zeroize;

use crate::Error;
use crate::blake2s::mac;
use crate::cookie::{cookie_key, mac2, reply};
use crate::crypto::{random, x25519_base};
use crate::handshake::{INITIATION_LEN, MSG_DATA, MSG_INITIATION, mac1};
use crate::respond::{open_static, same};
use crate::session::Session;

mod peer;

pub use peer::Peer;
use peer::PeerState;

/// A session is not used after this time (REJECT_AFTER_TIME).
pub const REJECT_AFTER: Duration = Duration::from_secs(180);
/// A keepalive goes this time after a packet came, when nothing went out
/// (KEEPALIVE_TIMEOUT).
pub const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(10);
/// The secret of the cookies changes after this time.
pub const COOKIE_SECRET_AGE: Duration = Duration::from_secs(120);
/// The shortest time between two initiations that a peer gets answered:
/// 1 s / INITIATIONS_PER_SECOND (50), as the Linux kernel has it.
pub const INITIATION_GAP: Duration = Duration::from_millis(20);
/// A session seals no message past this counter (REJECT_AFTER_MESSAGES).
pub const REJECT_AFTER_MESSAGES: u64 = u64::MAX - (1 << 13);

/// What a datagram gave.
#[derive(Debug)]
pub enum Incoming {
    /// The response to an initiation of `peer`: send it to the source. The
    /// new session takes packets out after the first transport message of
    /// the peer in it.
    Response { peer: usize, message: Vec<u8> },
    /// A cookie reply (under load, to an initiation with no good mac2):
    /// send it to the source.
    CookieReply(Vec<u8>),
    /// An IP packet of `peer`.
    Packet { peer: usize, packet: Vec<u8> },
    /// A keepalive of `peer`.
    Keepalive { peer: usize },
    /// A datagram that the responder does not take; no answer goes out.
    Dropped(Error),
}

/// The responder of WireGuard for a list of peers.
pub struct Responder {
    private: [u8; 32],
    public: [u8; 32],
    /// The key that seals each cookie reply.
    cookie_key: [u8; 32],
    peers: Vec<PeerState>,
    /// The peer of each local index.
    index: HashMap<u32, usize>,
    /// The secret of the cookies, and the time it was made.
    secret: Option<([u8; 32], Instant)>,
    entropy: Box<dyn Entropy + Send>,
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.private.zeroize();
        if let Some((secret, _)) = &mut self.secret {
            secret.zeroize();
        }
    }
}

impl std::fmt::Debug for Responder {
    /// No byte of a key: a debug line can go to a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Responder").field("peers", &self.peers.len()).finish_non_exhaustive()
    }
}

impl Responder {
    /// A responder with the private key `private` for `peers`, on the random
    /// bytes of the system.
    pub fn new(private: [u8; 32], peers: Vec<Peer>) -> Responder {
        Responder::with_entropy(private, peers, Box::new(OsEntropy))
    }

    /// A responder whose ephemeral keys, indexes, cookie secrets and nonces
    /// come from `entropy`.
    pub fn with_entropy(private: [u8; 32], peers: Vec<Peer>, entropy: Box<dyn Entropy + Send>) -> Responder {
        let public = x25519_base(&private);
        Responder {
            private,
            public,
            cookie_key: cookie_key(&public),
            peers: peers.into_iter().map(PeerState::new).collect(),
            index: HashMap::new(),
            secret: None,
            entropy,
        }
    }

    /// The public key of the responder.
    pub fn public(&self) -> [u8; 32] {
        self.public
    }

    /// The number of peers; a peer is its place in the list.
    pub fn peers(&self) -> usize {
        self.peers.len()
    }

    /// The session that seals the packets to `peer`, if one is confirmed.
    pub fn session(&self, peer: usize) -> Option<&Session> {
        self.peers.get(peer)?.current.as_ref()
    }

    /// Take the datagram `message` from `from`. With `under_load`, an
    /// initiation needs the mac2 of the cookie of `from`, and one with no
    /// good mac2 gets a cookie reply.
    pub fn receive(&mut self, message: &[u8], from: SocketAddr, under_load: bool, now: Instant) -> Incoming {
        match message.first() {
            Some(&MSG_INITIATION) if message.len() == INITIATION_LEN => self.initiation(message, from, under_load, now),
            Some(&MSG_INITIATION) => Incoming::Dropped(Error::NotInitiation),
            Some(&MSG_DATA) => self.transport(message, now),
            _ => Incoming::Dropped(Error::NotForResponder),
        }
    }

    fn initiation(&mut self, init: &[u8], from: SocketAddr, under_load: bool, now: Instant) -> Incoming {
        if !same(&mac1(&self.public, &init[..116]), &init[116..132]) {
            return Incoming::Dropped(Error::InitiationMac1);
        }
        if under_load {
            let cookie = match self.cookie(from, now) {
                Ok(cookie) => cookie,
                Err(e) => return Incoming::Dropped(e),
            };
            if !same(&mac2(&cookie, &init[..132]), &init[132..]) {
                return match random::<24>(self.entropy.as_mut()) {
                    Ok(nonce) => Incoming::CookieReply(reply(&self.cookie_key, init, &cookie, &nonce)),
                    Err(e) => Incoming::Dropped(e),
                };
            }
        }
        let Some(mut opened) = open_static(&self.private, init) else {
            return Incoming::Dropped(Error::InitiationNotAuthentic);
        };
        // Each peer is compared, and the timestamp is read with the key of
        // the initiation when no peer has it: a listed key and an unlisted
        // key take the same work.
        let mut found = None;
        for (i, p) in self.peers.iter().enumerate() {
            if same(&p.keys.public, &opened.initiator) {
                found = Some(i);
            }
        }
        let initiator = opened.initiator;
        let timestamp = opened.timestamp(&self.private, &initiator, init);
        let Some(peer) = found else { return Incoming::Dropped(Error::UnknownPeer) };
        let Some(timestamp) = timestamp else { return Incoming::Dropped(Error::InitiationNotAuthentic) };
        let state = &mut self.peers[peer];
        if state.last_timestamp.is_some_and(|last| timestamp <= last) {
            return Incoming::Dropped(Error::OldTimestamp);
        }
        if state.last_initiation.is_some_and(|last| now.saturating_duration_since(last) < INITIATION_GAP) {
            return Incoming::Dropped(Error::Flood);
        }
        let (e_priv, local) = match self.fresh_keys() {
            Ok(keys) => keys,
            Err(e) => return Incoming::Dropped(e),
        };
        let state = &mut self.peers[peer];
        let mut psk = state.keys.preshared.unwrap_or([0; 32]);
        let (message, send, recv) = opened.respond(&initiator, &psk, &e_priv, local);
        psk.zeroize();
        state.last_timestamp = Some(timestamp);
        state.last_initiation = Some(now);
        let session = Session::new(send, recv, local, opened.sender, now);
        if let Some(old) = state.take_next(session) {
            self.index.remove(&old);
        }
        self.index.insert(local, peer);
        // The response is an authenticated message out.
        self.peers[peer].keepalive_due = None;
        Incoming::Response { peer, message }
    }

    /// An ephemeral key and a local index that no session has.
    fn fresh_keys(&mut self) -> Result<([u8; 32], u32), Error> {
        let e_priv = random::<32>(self.entropy.as_mut())?;
        loop {
            let local = u32::from_le_bytes(random::<4>(self.entropy.as_mut())?);
            if !self.index.contains_key(&local) {
                return Ok((e_priv, local));
            }
        }
    }

    /// The cookie of the address `from`: the MAC of its bytes with the
    /// secret of now, which changes each [`COOKIE_SECRET_AGE`].
    fn cookie(&mut self, from: SocketAddr, now: Instant) -> Result<[u8; 16], Error> {
        let secret = match self.secret {
            Some((secret, made)) if now.saturating_duration_since(made) < COOKIE_SECRET_AGE => secret,
            _ => {
                let secret = random::<32>(self.entropy.as_mut())?;
                self.secret = Some((secret, now));
                secret
            }
        };
        let (ip, port) = match from {
            SocketAddr::V4(a) => (a.ip().octets().to_vec(), a.port()),
            SocketAddr::V6(a) => (a.ip().octets().to_vec(), a.port()),
        };
        Ok(mac(&secret, &[&ip, &port.to_be_bytes()]))
    }

    fn transport(&mut self, message: &[u8], now: Instant) -> Incoming {
        if message.len() < 32 {
            return Incoming::Dropped(Error::NotTransport);
        }
        let receiver = Session::receiver_of(message).expect("32 bytes");
        let Some(&peer) = self.index.get(&receiver) else { return Incoming::Dropped(Error::OtherSession) };
        let state = &mut self.peers[peer];
        match state.open(receiver, message, now) {
            Ok((packet, dropped)) => {
                if let Some(old) = dropped {
                    self.index.remove(&old);
                }
                if packet.is_empty() { Incoming::Keepalive { peer } } else { Incoming::Packet { peer, packet } }
            }
            Err(e) => Incoming::Dropped(e),
        }
    }

    /// Seal each packet to `peer` with its current session. An error when
    /// it has none, or when that session is past [`REJECT_AFTER`]: the
    /// peer must make a new handshake.
    pub fn send<P: AsRef<[u8]>>(&mut self, peer: usize, packets: &[P], now: Instant) -> Result<Vec<Vec<u8>>, Error> {
        let state = self.peers.get_mut(peer).ok_or(Error::NoSession)?;
        state.seal(packets, now)
    }

    /// The timers at `now`: a keepalive for each peer that sent a packet
    /// [`KEEPALIVE_TIMEOUT`] ago and got nothing since; each session past
    /// [`REJECT_AFTER`] is removed. No initiation: the responder starts no
    /// rekey.
    pub fn tick(&mut self, now: Instant) -> Vec<(usize, Vec<u8>)> {
        let mut out = Vec::new();
        for (peer, state) in self.peers.iter_mut().enumerate() {
            for old in state.expire(now) {
                self.index.remove(&old);
            }
            if let Some(keepalive) = state.keepalive(now) {
                out.push((peer, keepalive));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
