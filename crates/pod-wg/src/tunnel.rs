//! The state of a tunnel after its first handshake, sans I/O: the current
//! and the previous session, the rekey with its timers, and the keepalive.
//! Each call takes the time `now` of the caller's monotonic clock, and gives
//! the messages to send; the caller owns the socket.

use std::time::{Duration, Instant};

use pod_core::entropy::OsEntropy;

use crate::Error;
use crate::cookie::Cookie;
use crate::crypto::Stamps;
use crate::handshake::{Initiation, Keys, MSG_COOKIE, MSG_DATA, MSG_RESPONSE, initiate_from};
use crate::session::Session;

/// A new session after this time (REKEY_AFTER_TIME).
pub const REKEY_AFTER: Duration = Duration::from_secs(120);
/// A handshake with no answer is sent again after this time (REKEY_TIMEOUT).
pub const REKEY_TIMEOUT: Duration = Duration::from_secs(5);
/// A handshake with no answer in this time: the path is dead.
pub const HANDSHAKE_DEAD: Duration = Duration::from_secs(20);
/// The persistent keepalive.
pub const KEEPALIVE: Duration = Duration::from_secs(25);

/// A rekey that waits for its response.
struct Pending {
    init: Initiation,
    /// The first initiation of this rekey: the dead time counts from it.
    first: Instant,
    /// The last initiation sent.
    sent: Instant,
}

/// What a message from the peer gave.
#[derive(Debug)]
pub enum Received {
    /// An IP packet.
    Packet(Vec<u8>),
    /// A keepalive: a transport message with no packet.
    Keepalive,
    /// The response of a rekey: its session is now the current one.
    Rekeyed,
    /// The cookie reply of a responder under load: the next initiation of
    /// the rekey carries its mac2.
    Cookie,
    /// A message that this tunnel does not take: a response with no rekey,
    /// a transport message for no session, another type.
    Ignored,
    /// A message that failed its check; the tunnel drops it.
    Dropped(Error),
}

/// The messages of a send: the initiation of a rekey, if one is due, then
/// a transport message for each packet.
#[derive(Debug, Default)]
pub struct Outgoing {
    pub handshake: Option<Vec<u8>>,
    pub messages: Vec<Vec<u8>>,
}

/// A tunnel to one peer, from the session of its first handshake.
pub struct Tunnel {
    keys: Keys,
    current: Session,
    previous: Option<Session>,
    pending: Option<Pending>,
    last_send: Instant,
    stamps: Stamps,
    cookie: Option<Cookie>,
}

impl Tunnel {
    /// A tunnel with `session`, whose handshake ended at `now`. `stamps`
    /// gave the timestamp of that handshake: the rekeys go on from it.
    pub fn new(keys: Keys, session: Session, stamps: Stamps, now: Instant) -> Tunnel {
        Tunnel { keys, current: session, previous: None, pending: None, last_send: now, stamps, cookie: None }
    }

    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    /// The session that seals each message out.
    pub fn current(&self) -> &Session {
        &self.current
    }

    /// True while a rekey waits for its response.
    pub fn rekeying(&self) -> bool {
        self.pending.is_some()
    }

    fn initiate(&mut self, now: Instant) -> Result<Initiation, Error> {
        let mut init = initiate_from(&self.keys, &mut OsEntropy, self.stamps.next(now))?;
        if let Some(cookie) = &self.cookie {
            init.add_mac2(cookie, now);
        }
        Ok(init)
    }

    /// A new initiation, or the one that waits when it is new enough. The
    /// message to send, if any. An error when a rekey had no answer in
    /// [`HANDSHAKE_DEAD`].
    pub fn rekey(&mut self, now: Instant) -> Result<Option<Vec<u8>>, Error> {
        match &self.pending {
            Some(p) if now.duration_since(p.first) > HANDSHAKE_DEAD => Err(Error::Dead(HANDSHAKE_DEAD)),
            Some(p) if now.duration_since(p.sent) > REKEY_TIMEOUT => {
                let first = p.first;
                let init = self.initiate(now)?;
                let m = init.message.clone();
                self.pending = Some(Pending { init, first, sent: now });
                Ok(Some(m))
            }
            Some(_) => Ok(None),
            None if now.duration_since(self.current.created) > REKEY_AFTER => {
                let init = self.initiate(now)?;
                let m = init.message.clone();
                self.pending = Some(Pending { init, first: now, sent: now });
                Ok(Some(m))
            }
            None => Ok(None),
        }
    }

    /// Seal each packet with the current session, after the rekey that is
    /// due.
    pub fn send<P: AsRef<[u8]>>(&mut self, packets: &[P], now: Instant) -> Result<Outgoing, Error> {
        let handshake = self.rekey(now)?;
        let reserved = self.keys.reserved;
        let messages = packets.iter().map(|p| self.current.seal(&reserved, p.as_ref())).collect();
        self.last_send = now;
        Ok(Outgoing { handshake, messages })
    }

    /// Take a message from the peer.
    pub fn receive(&mut self, message: &[u8], now: Instant) -> Received {
        match message.first() {
            Some(&MSG_DATA) => {
                let receiver = Session::receiver_of(message);
                let session = if receiver == Some(self.current.local) {
                    Some(&mut self.current)
                } else {
                    self.previous.as_mut().filter(|s| Some(s.local) == receiver)
                };
                let Some(session) = session else { return Received::Ignored };
                match session.open(message) {
                    Ok(packet) if packet.is_empty() => Received::Keepalive,
                    Ok(packet) => Received::Packet(packet),
                    Err(err) => Received::Dropped(err),
                }
            }
            Some(&MSG_RESPONSE) => {
                let Some(pending) = self.pending.take() else { return Received::Ignored };
                match pending.init.finish(&self.keys, message, now) {
                    Ok(session) => {
                        let old = std::mem::replace(&mut self.current, session);
                        self.previous = Some(old);
                        Received::Rekeyed
                    }
                    Err(err) => {
                        // The rekey waits on with its times: a response
                        // that fails its check does not move the dead time.
                        self.pending = Some(pending);
                        Received::Dropped(err)
                    }
                }
            }
            Some(&MSG_COOKIE) => {
                let Some(pending) = &self.pending else { return Received::Ignored };
                match Cookie::open(&self.keys, &pending.init, message, now) {
                    // The initiation is sent again at its time, with the
                    // mac2 of the cookie (the WireGuard paper, section
                    // 5.4.7): a reply does not make a send at once.
                    Ok(cookie) => {
                        self.cookie = Some(cookie);
                        Received::Cookie
                    }
                    Err(err) => Received::Dropped(err),
                }
            }
            _ => Received::Ignored,
        }
    }

    /// The liveness at `now`: the initiation of a rekey that is due, and a
    /// keepalive when nothing went out for [`KEEPALIVE`]. An error when a
    /// rekey had no answer in [`HANDSHAKE_DEAD`].
    pub fn tick(&mut self, now: Instant) -> Result<Vec<Vec<u8>>, Error> {
        let handshake = self.rekey(now)?;
        let keepalive = if now.duration_since(self.last_send) >= KEEPALIVE {
            self.last_send = now;
            let reserved = self.keys.reserved;
            Some(self.current.seal(&reserved, &[]))
        } else {
            None
        };
        Ok(handshake.into_iter().chain(keepalive).collect())
    }
}

#[cfg(test)]
mod tests;
