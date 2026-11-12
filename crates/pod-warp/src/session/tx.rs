//! The write half of a session.

use std::time::Duration;

use bytes::{Bytes, BytesMut};
use pod_core::{Class, Failure, Stage};

use super::MasqueRx;
use super::keepalive::Keepalive;
use crate::capsule;

/// The HTTP/2 send capacity that the MASQUE stream asks for: the most
/// packet bytes that wait for the window of the front door.
pub const SEND_RESERVE: usize = 256 * 1024;

/// The longest wait for HTTP/2 send capacity: then the session is dead.
pub const SEND_STALL: Duration = Duration::from_secs(30);

/// The write half of a session, for the task that pumps outbound packets.
pub struct MasqueTx {
    ping: Option<h2::PingPong>,
    send: h2::SendStream<Bytes>,
    closed: bool,
    waits: u64,
    /// The longest wait for send capacity ([`SEND_STALL`]).
    stall: Duration,
}

fn dead(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Warp, class, text)
}

impl MasqueTx {
    pub(super) fn new(ping: Option<h2::PingPong>, send: h2::SendStream<Bytes>) -> MasqueTx {
        MasqueTx { ping, send, closed: false, waits: 0, stall: SEND_STALL }
    }

    /// Send one IP packet through the tunnel.
    ///
    /// The packet waits for the HTTP/2 send capacity of the stream, and h2
    /// holds at most [`SEND_RESERVE`] bytes for the window of the front door.
    /// A packet with no capacity is not dropped (PodUtils/podnet issue 93):
    /// the pure ACKs and the window updates of a download stall the far
    /// sender when they are lost. A full queue in front of this call drops
    /// packets in its place.
    pub async fn send_packet(&mut self, packet: &[u8]) -> Result<(), Failure> {
        self.send_packets(std::slice::from_ref(&packet)).await
    }

    /// Send several packets in one HTTP/2 DATA frame (PodUtils/podnet issue
    /// 122): one reserve, one wait for capacity and one write for all.
    pub async fn send_packets<P: AsRef<[u8]>>(&mut self, packets: &[P]) -> Result<(), Failure> {
        if self.closed {
            return Err(dead(Class::Unavailable, "the MASQUE session is closed"));
        }
        let total: usize = packets.iter().map(|p| p.as_ref().len() + 4).sum();
        let mut buf = BytesMut::with_capacity(total);
        for packet in packets {
            capsule::put_capsule(&mut buf, capsule::CAPSULE_DATA, packet.as_ref());
        }
        let send = &mut self.send;
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

    /// How many times a packet waited for send capacity.
    pub fn waits(&self) -> u64 {
        self.waits
    }

    /// The liveness check of this session, with the progress of `rx`; once,
    /// as the PING handle moves to it.
    pub fn keepalive(&mut self, rx: &MasqueRx) -> Option<Keepalive> {
        let progress = rx.progress();
        self.ping.take().map(|ping| Keepalive::h2(ping, progress))
    }

    /// Close the request body: the front door ends the tunnel.
    pub fn close(&mut self) -> Result<(), Failure> {
        if !self.closed {
            self.closed = true;
            self.send
                .send_data(Bytes::new(), true)
                .map_err(|e| Failure::caused(Stage::Warp, Class::Unavailable, "closing the CONNECT-IP body", e))?;
        }
        Ok(())
    }
}
