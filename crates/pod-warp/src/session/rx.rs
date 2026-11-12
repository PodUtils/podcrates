//! The read half of a session.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use pod_core::{Class, Failure, Stage};

use crate::capsule::CapsuleDecoder;

/// The read half of a session, for the task that pumps inbound packets.
pub struct MasqueRx {
    recv: h2::RecvStream,
    decoder: CapsuleDecoder,
    closed: bool,
    /// The count of DATA frames received: proof that the peer is alive.
    progress: Arc<AtomicU64>,
}

impl MasqueRx {
    pub(super) fn new(recv: h2::RecvStream, decoder: CapsuleDecoder) -> MasqueRx {
        MasqueRx { recv, decoder, closed: false, progress: Arc::default() }
    }

    /// The counter of the frames that came in.
    pub fn progress(&self) -> Arc<AtomicU64> {
        self.progress.clone()
    }

    /// The addresses of an ADDRESS_ASSIGN capsule that came since the last
    /// call (PodUtils/podnet issue 121).
    pub fn take_assigned(&mut self) -> Option<Vec<(IpAddr, u8)>> {
        self.decoder.take_assigned()
    }

    /// Receive one IP packet, and wait for more body bytes when needed.
    /// `None` when the front door ended the body.
    pub async fn recv_packet(&mut self) -> Result<Option<Bytes>, Failure> {
        loop {
            if let Some(packet) = self.decoder.next_packet() {
                return Ok(Some(packet));
            }
            if let Some(err) = self.decoder.error() {
                // PodUtils/podnet issue 123: the stream is not usable after it.
                self.closed = true;
                return Err(Failure::new(Stage::Warp, Class::Protocol, format!("the CONNECT-IP body: {err}")));
            }
            if self.closed {
                return Ok(None);
            }
            match self.recv.data().await {
                Some(Ok(data)) => {
                    let _ = self.recv.flow_control().release_capacity(data.len());
                    self.decoder.push(&data);
                    self.progress.fetch_add(1, Ordering::Relaxed);
                }
                Some(Err(err)) => {
                    self.closed = true;
                    return Err(Failure::caused(Stage::Warp, Class::Unavailable, "reading the CONNECT-IP body", err));
                }
                None => {
                    self.closed = true;
                    return Ok(None);
                }
            }
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}
