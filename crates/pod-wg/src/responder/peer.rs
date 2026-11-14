//! A peer of the responder: its keys, the last initiation that it took, and
//! its sessions. A new session waits as `next` until the first transport
//! message of the peer in it (the key confirmation of the WireGuard paper,
//! section 5.4.6); it then becomes the current one, and the current one the
//! previous one.

use std::time::Instant;

use zeroize::Zeroize;

use super::{KEEPALIVE_TIMEOUT, REJECT_AFTER, REJECT_AFTER_MESSAGES};
use crate::Error;
use crate::session::Session;

/// A peer of the responder: its public key and the preshared key of the
/// two (`None`: the zero key).
#[derive(Clone)]
pub struct Peer {
    pub public: [u8; 32],
    pub preshared: Option<[u8; 32]>,
}

impl std::fmt::Debug for Peer {
    /// No byte of a key: a debug line can go to a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Peer { .. }")
    }
}

pub(crate) struct PeerState {
    pub(crate) keys: Peer,
    /// The TAI64N timestamp of the last initiation that the responder took.
    pub(crate) last_timestamp: Option<[u8; 12]>,
    pub(crate) last_initiation: Option<Instant>,
    next: Option<Session>,
    pub(crate) current: Option<Session>,
    previous: Option<Session>,
    /// The time of a keepalive: a packet came, and nothing went out since.
    pub(crate) keepalive_due: Option<Instant>,
}

impl Drop for PeerState {
    fn drop(&mut self) {
        if let Some(psk) = &mut self.keys.preshared {
            psk.zeroize();
        }
    }
}

/// True while `session` can carry messages at `now`.
fn live(session: &Session, now: Instant) -> bool {
    now.saturating_duration_since(session.created) < REJECT_AFTER
}

impl PeerState {
    pub(crate) fn new(keys: Peer) -> PeerState {
        PeerState {
            keys,
            last_timestamp: None,
            last_initiation: None,
            next: None,
            current: None,
            previous: None,
            keepalive_due: None,
        }
    }

    /// Put the session of a new handshake in wait for its confirmation; the
    /// local index of a session that waited before it, which goes.
    pub(crate) fn take_next(&mut self, session: Session) -> Option<u32> {
        self.next.replace(session).map(|old| old.local)
    }

    /// The packet of a transport message to the session of the local index
    /// `receiver`, and the local index of a session that goes because of it.
    pub(crate) fn open(
        &mut self,
        receiver: u32,
        message: &[u8],
        now: Instant,
    ) -> Result<(Vec<u8>, Option<u32>), Error> {
        let is = |s: &Option<Session>| s.as_ref().is_some_and(|s| s.local == receiver);
        let confirms = is(&self.next);
        let session = if confirms {
            self.next.as_mut()
        } else if is(&self.current) {
            self.current.as_mut()
        } else if is(&self.previous) {
            self.previous.as_mut()
        } else {
            None
        };
        let session = session.ok_or(Error::OtherSession)?;
        if !live(session, now) {
            return Err(Error::Expired);
        }
        let packet = session.open(message)?;
        let mut dropped = None;
        if confirms {
            dropped = self.previous.take().map(|s| s.local);
            self.previous = self.current.take();
            self.current = self.next.take();
        }
        if !packet.is_empty() && self.keepalive_due.is_none() {
            self.keepalive_due = Some(now + KEEPALIVE_TIMEOUT);
        }
        Ok((packet, dropped))
    }

    /// Each packet sealed with the current session.
    pub(crate) fn seal<P: AsRef<[u8]>>(&mut self, packets: &[P], now: Instant) -> Result<Vec<Vec<u8>>, Error> {
        let session = self.current.as_mut().ok_or(Error::NoSession)?;
        let room = REJECT_AFTER_MESSAGES.saturating_sub(session.send_counter);
        if !live(session, now) || room < packets.len() as u64 {
            return Err(Error::Expired);
        }
        let out = packets.iter().map(|p| session.seal(&[0; 3], p.as_ref())).collect();
        self.keepalive_due = None;
        Ok(out)
    }

    /// Remove each session past [`REJECT_AFTER`] at `now`: their local
    /// indexes.
    pub(crate) fn expire(&mut self, now: Instant) -> Vec<u32> {
        let mut gone = Vec::new();
        for slot in [&mut self.next, &mut self.current, &mut self.previous] {
            if slot.as_ref().is_some_and(|s| !live(s, now)) {
                gone.extend(slot.take().map(|s| s.local));
            }
        }
        gone
    }

    /// The keepalive that is due at `now`, if the current session can
    /// carry it.
    pub(crate) fn keepalive(&mut self, now: Instant) -> Option<Vec<u8>> {
        if self.keepalive_due.is_some_and(|due| due <= now) {
            self.keepalive_due = None;
            return self.seal(&[[0u8; 0]], now).ok()?.pop();
        }
        None
    }
}
