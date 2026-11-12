//! The write half of a session, over each transport.

use std::time::Duration;

use bytes::{Bytes, BytesMut};
use pod_core::{Class, Failure, Stage};

use super::MasqueRx;
use super::keepalive::Keepalive;
use crate::capsule;
use crate::h3::H3Tx;
use crate::wg::{WgKeepalive, WgTx};

/// The HTTP/2 send capacity that the MASQUE stream asks for: the most
/// packet bytes that wait for the window of the front door.
pub const SEND_RESERVE: usize = 256 * 1024;

/// The longest wait for HTTP/2 send capacity: then the session is dead.
pub const SEND_STALL: Duration = Duration::from_secs(30);

/// The write half of a session, for the task that pumps outbound packets.
pub struct MasqueTx {
    inner: TxInner,
    closed: bool,
    waits: u64,
    /// The longest wait for send capacity ([`SEND_STALL`]).
    stall: Duration,
}

pub(super) enum TxInner {
    /// HTTP/2: the PING handle (until the keepalive takes it) and the
    /// request body.
    H2 { ping: Option<h2::PingPong>, send: h2::SendStream<Bytes> },
    /// HTTP/3: the datagrams and the request stream.
    H3 { tx: Box<H3Tx>, keepalive_taken: bool },
    /// WireGuard: the messages, and the timers until the keepalive takes
    /// them.
    Wg { tx: Box<WgTx>, keepalive: Option<WgKeepalive> },
}

fn dead(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Warp, class, text)
}

impl MasqueTx {
    pub(super) fn new(inner: TxInner) -> MasqueTx {
        MasqueTx { inner, closed: false, waits: 0, stall: SEND_STALL }
    }

    /// True for a session over HTTP/3.
    pub(super) fn is_h3(&self) -> bool {
        matches!(self.inner, TxInner::H3 { .. })
    }

    /// True for a session over WireGuard.
    pub(super) fn is_wg(&self) -> bool {
        matches!(self.inner, TxInner::Wg { .. })
    }

    /// Send one IP packet through the tunnel.
    ///
    /// Over HTTP/2, the packet waits for the send capacity of the stream,
    /// and h2 holds at most [`SEND_RESERVE`] bytes for the window of the
    /// front door. A packet with no capacity is not dropped
    /// (PodUtils/podnet issue 93): the pure ACKs and the window updates of a
    /// download stall the far sender when they are lost. A full queue in
    /// front of this call drops packets in its place.
    pub async fn send_packet(&mut self, packet: &[u8]) -> Result<(), Failure> {
        self.send_packets(std::slice::from_ref(&packet)).await
    }

    /// Send several packets: over HTTP/2 in one DATA frame (PodUtils/podnet
    /// issue 122), with one reserve, one wait for capacity and one write for
    /// all; over HTTP/3 and WireGuard, one datagram for each.
    pub async fn send_packets<P: AsRef<[u8]>>(&mut self, packets: &[P]) -> Result<(), Failure> {
        if self.closed {
            return Err(dead(Class::Unavailable, "the MASQUE session is closed"));
        }
        let sent = match &mut self.inner {
            TxInner::H2 { .. } => None,
            TxInner::H3 { tx, .. } => Some(tx.send_packets(packets).await),
            TxInner::Wg { tx, .. } => Some(tx.send_packets(packets).await),
        };
        if let Some(result) = sent {
            self.closed = result.is_err();
            return result;
        }
        self.send_h2(packets).await
    }

    async fn send_h2<P: AsRef<[u8]>>(&mut self, packets: &[P]) -> Result<(), Failure> {
        let TxInner::H2 { send, .. } = &mut self.inner else {
            unreachable!("send_h2 is for a session over HTTP/2");
        };
        let total: usize = packets.iter().map(|p| p.as_ref().len() + 4).sum();
        let mut buf = BytesMut::with_capacity(total);
        for packet in packets {
            capsule::put_capsule(&mut buf, capsule::CAPSULE_DATA, packet.as_ref());
        }
        send.reserve_capacity(SEND_RESERVE.max(buf.len()));
        while send.capacity() < buf.len() {
            self.waits += 1;
            let capacity = std::future::poll_fn(|cx| send.poll_capacity(cx));
            let Ok(capacity) = tokio::time::timeout(self.stall, capacity).await else {
                self.closed = true;
                let text = format!("the MASQUE session gave no send capacity in {} s", self.stall.as_secs());
                return Err(dead(Class::Timeout, text));
            };
            match capacity {
                Some(Ok(_)) => {}
                Some(Err(err)) => {
                    self.closed = true;
                    return Err(Failure::caused(
                        Stage::Warp,
                        Class::Unavailable,
                        "waiting for MASQUE send capacity",
                        err,
                    ));
                }
                None => {
                    self.closed = true;
                    return Err(dead(Class::Unavailable, "the MASQUE stream has no send capacity any more"));
                }
            }
            send.reserve_capacity(SEND_RESERVE.max(buf.len()));
        }
        send.send_data(buf.freeze(), false)
            .map_err(|e| Failure::caused(Stage::Warp, Class::Unavailable, "writing a CONNECT-IP DATA capsule", e))
    }

    /// How many times a packet waited for HTTP/2 send capacity.
    pub fn waits(&self) -> u64 {
        self.waits
    }

    /// The liveness check of this session, with the progress of `rx`; once,
    /// as the handle of the check moves to it.
    pub fn keepalive(&mut self, rx: &MasqueRx) -> Option<Keepalive> {
        let progress = rx.progress();
        match &mut self.inner {
            TxInner::H2 { ping, .. } => ping.take().map(|ping| Keepalive::h2(ping, progress)),
            TxInner::H3 { tx, keepalive_taken } => {
                let first = !std::mem::replace(keepalive_taken, true);
                first.then(|| Keepalive::h3(tx.connection(), progress))
            }
            TxInner::Wg { keepalive, .. } => keepalive.take().map(|k| Keepalive::wg(k, progress)),
        }
    }

    /// Close the session: the end of the request body (HTTP/2), the close
    /// of the stream and of the connection (HTTP/3). WireGuard has no
    /// close message.
    pub fn close(&mut self) -> Result<(), Failure> {
        if !self.closed {
            self.closed = true;
            match &mut self.inner {
                TxInner::H2 { send, .. } => send
                    .send_data(Bytes::new(), true)
                    .map_err(|e| Failure::caused(Stage::Warp, Class::Unavailable, "closing the CONNECT-IP body", e))?,
                TxInner::H3 { tx, .. } => tx.close(),
                TxInner::Wg { .. } => {}
            }
        }
        Ok(())
    }
}
