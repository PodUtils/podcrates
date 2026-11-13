//! The framing of the ICMP echo carrier as a [`crate::datagram::Datagrams`]:
//! each datagram of the ARQ rides in the data of one echo message, with the
//! session that tells one link from another and the `from` mark that tells
//! the dialer's bytes from the listener's. [`EchoFrames`] wraps any inner
//! datagram carrier that moves whole ICMP messages (a raw socket, or a
//! memory pair in a test), so the same framing runs with a privilege and
//! without it (call C276).

use std::sync::atomic::{AtomicU16, Ordering};

use super::wire::{self, CLIENT, SERVER};
use crate::datagram::{DatagramFuture, Datagrams};

/// A datagram carrier that frames each datagram as an ICMP echo message over
/// an inner carrier of whole ICMP messages. This end sends messages of
/// `kind` with the mark `from`, and takes only the messages of its `session`
/// that carry the mark `take`; a message of another session or mark (its
/// own, looped back on a shared socket, or the kernel's reply to an echo
/// request) is dropped.
pub struct EchoFrames<D> {
    inner: D,
    kind: u8,
    session: u32,
    id: u16,
    from: u8,
    take: u8,
    seq: AtomicU16,
}

impl<D: Datagrams> EchoFrames<D> {
    /// Frame over `inner`: send echo messages of `kind` with the mark `from`,
    /// and take the messages of `session` that carry the mark `take`.
    pub fn new(inner: D, kind: u8, session: u32, id: u16, from: u8, take: u8) -> EchoFrames<D> {
        EchoFrames { inner, kind, session, id, from, take, seq: AtomicU16::new(0) }
    }

    /// The dialer's side: send echo requests marked [`CLIENT`], take replies
    /// marked [`SERVER`].
    pub fn client(inner: D, session: u32, id: u16) -> EchoFrames<D> {
        EchoFrames::new(inner, wire::REQUEST, session, id, CLIENT, SERVER)
    }

    /// The listener's side: send echo replies marked [`SERVER`], take
    /// requests marked [`CLIENT`].
    pub fn server(inner: D, session: u32, id: u16) -> EchoFrames<D> {
        EchoFrames::new(inner, wire::REPLY, session, id, SERVER, CLIENT)
    }
}

impl<D: Datagrams> Datagrams for EchoFrames<D> {
    fn mtu(&self) -> usize {
        self.inner.mtu().saturating_sub(wire::HEADER + wire::SESSION)
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let message = wire::encode(self.kind, self.id, seq, self.session, self.from, datagram);
        Box::pin(async move { self.inner.send(&message).await })
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        Box::pin(async move {
            loop {
                let message = self.inner.recv().await?;
                if let Some(echo) = wire::decode(&message)
                    && echo.session == self.session
                    && echo.from == self.take
                {
                    return Ok(echo.payload);
                }
            }
        })
    }
}

impl<D> std::fmt::Debug for EchoFrames<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EchoFrames").field("session", &self.session).field("from", &self.from).finish_non_exhaustive()
    }
}
