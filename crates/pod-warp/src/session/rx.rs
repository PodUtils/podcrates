//! The read half of a session, over each transport.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use pod_core::{Class, Failure, Stage};

use crate::capsule::CapsuleDecoder;
use crate::h3::{H3Rx, Incoming};
use crate::wg::WgRx;

/// The read half of a session, for the task that pumps inbound packets.
pub struct MasqueRx {
    inner: RxInner,
    decoder: CapsuleDecoder,
    closed: bool,
    /// The count of DATA frames or datagrams received: proof that the peer
    /// is alive.
    progress: Arc<AtomicU64>,
}

pub(super) enum RxInner {
    H2(h2::RecvStream),
    H3(Box<H3Rx>),
    Wg(Box<WgRx>),
}

impl MasqueRx {
    pub(super) fn new(inner: RxInner) -> MasqueRx {
        MasqueRx { inner, decoder: CapsuleDecoder::new(), closed: false, progress: Arc::default() }
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

    fn tick(&self) {
        self.progress.fetch_add(1, Ordering::Relaxed);
    }

    /// Receive one IP packet, and wait for more bytes when needed. `None`
    /// when the front door ended the session.
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
            let came = match &mut self.inner {
                RxInner::H2(recv) => match recv.data().await {
                    Some(Ok(data)) => {
                        let _ = recv.flow_control().release_capacity(data.len());
                        Ok(Some(Incoming::Capsules(data)))
                    }
                    Some(Err(err)) => {
                        Err(Failure::caused(Stage::Warp, Class::Unavailable, "reading the CONNECT-IP body", err))
                    }
                    None => Ok(None),
                },
                RxInner::H3(rx) => rx.next().await,
                RxInner::Wg(rx) => rx.recv_packet().await.map(|p| p.map(Incoming::Packet)),
            };
            match came {
                Ok(Some(Incoming::Packet(packet))) => {
                    self.tick();
                    return Ok(Some(packet));
                }
                Ok(Some(Incoming::Capsules(data))) => {
                    self.decoder.push(&data);
                    self.tick();
                }
                Ok(None) => {
                    self.closed = true;
                    return Ok(None);
                }
                Err(err) => {
                    self.closed = true;
                    return Err(err);
                }
            }
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}
