//! The framing of the ICMP echo carrier as a [`crate::datagram::Datagrams`]:
//! each datagram of the ARQ rides in the data of one echo message, with the
//! session that tells one link from another. [`EchoFrames`] wraps any inner
//! datagram carrier that moves whole ICMP messages (a raw socket, or a
//! memory pair in a test), so the same framing runs with a privilege and
//! without it (PodUtils/podnet#469 asks for a skip without `CAP_NET_RAW`;
//! call C276).

use std::sync::atomic::{AtomicU16, Ordering};

use super::wire;
use crate::datagram::{DatagramFuture, Datagrams};

/// A datagram carrier that frames each datagram as an ICMP echo message over
/// an inner carrier of whole ICMP messages. This end sends messages of
/// `kind` (a request or a reply), and takes only the messages of the other
/// kind with its `session`; a message of its own kind (its own, looped back
/// on a shared socket), of another session, or of no echo, is dropped.
pub struct EchoFrames<D> {
    inner: D,
    kind: u8,
    session: u32,
    id: u16,
    seq: AtomicU16,
}

impl<D: Datagrams> EchoFrames<D> {
    /// Frame over `inner`: send echo messages of `kind` (`wire::REQUEST` or
    /// `wire::REPLY`) with `session` and `id`.
    pub fn new(inner: D, kind: u8, session: u32, id: u16) -> EchoFrames<D> {
        EchoFrames { inner, kind, session, id, seq: AtomicU16::new(0) }
    }
}

impl<D: Datagrams> Datagrams for EchoFrames<D> {
    fn mtu(&self) -> usize {
        self.inner.mtu().saturating_sub(wire::HEADER + wire::SESSION)
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let message = wire::encode(self.kind, self.id, seq, self.session, datagram);
        Box::pin(async move { self.inner.send(&message).await })
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        Box::pin(async move {
            loop {
                let message = self.inner.recv().await?;
                if let Some(echo) = wire::decode(&message)
                    && echo.kind != self.kind
                    && echo.session == self.session
                {
                    return Ok(echo.payload);
                }
            }
        })
    }
}

impl<D> std::fmt::Debug for EchoFrames<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EchoFrames").field("kind", &self.kind).field("session", &self.session).finish_non_exhaustive()
    }
}
