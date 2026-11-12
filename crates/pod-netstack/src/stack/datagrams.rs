//! A UDP socket of the stack as [`pod_core::Datagrams`], so that a user of
//! datagrams (QUIC, a DNS client) sends UDP through the stack.

use std::io::{self, IoSliceMut};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use pod_core::{Datagrams, RecvMeta, Transmit};
use tokio::sync::mpsc;

use super::reactor::{Inbound, Outbound};
use super::socket::{DropKick, one_family, took_datagram, udp_closed};
use super::udp_service::Limits;
use super::{NetStats, UdpSocket};

/// The datagrams of one UDP socket of the stack.
///
/// A send does not wait: when the queue to the stack is full, the datagram
/// is dropped, as a full UDP buffer of the kernel drops it, and counted in
/// `NetStats::udp_tx_dropped`. A datagram over [`UdpDatagrams::max_datagram`]
/// is `InvalidInput`.
pub struct UdpDatagrams {
    id: u32,
    local: SocketAddr,
    recv_rx: Mutex<mpsc::Receiver<Inbound>>,
    send_tx: mpsc::Sender<Outbound>,
    read_stalled: Arc<AtomicBool>,
    stats: Arc<NetStats>,
    limits: Limits,
    /// The last field: its drop kicks the reactor after the channels closed.
    kick: DropKick,
}

impl std::fmt::Debug for UdpDatagrams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdpDatagrams").field("id", &self.id).field("local", &self.local).finish()
    }
}

impl UdpSocket {
    /// This socket as [`Datagrams`].
    pub fn into_datagrams(self) -> UdpDatagrams {
        UdpDatagrams {
            id: self.id,
            local: self.local,
            recv_rx: Mutex::new(self.recv_rx),
            send_tx: self.send_tx,
            read_stalled: self.read_stalled,
            stats: self.stats,
            limits: self.limits,
            kick: self.kick,
        }
    }
}

impl UdpDatagrams {
    /// The largest datagram that this socket sends to `to`
    /// ([`crate::StackConfig::max_datagram`]).
    pub fn max_datagram(&self, to: IpAddr) -> usize {
        self.limits.to(to)
    }
}

impl Datagrams for UdpDatagrams {
    fn poll_writable(&self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.send_tx.is_closed() {
            return Poll::Ready(Err(udp_closed()));
        }
        Poll::Ready(Ok(()))
    }

    fn try_send(&self, t: &Transmit<'_>) -> io::Result<()> {
        // The source of the transmit is the source of the datagram (a socket
        // on any address answers from the destination of the peer).
        one_family(t.src_ip.unwrap_or(self.local.ip()), t.destination)?;
        let size = t.segment_size.filter(|s| *s > 0).unwrap_or(t.contents.len().max(1));
        // Each segment is one datagram; the largest is the first.
        self.limits.check(size.min(t.contents.len()), t.destination, &self.stats)?;
        for chunk in t.contents.chunks(size) {
            match self.send_tx.try_send(Outbound { data: chunk.to_vec(), to: t.destination, from: t.src_ip }) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => NetStats::count(&self.stats.udp_tx_dropped),
                Err(mpsc::error::TrySendError::Closed(_)) => return Err(udp_closed()),
            }
        }
        self.kick.notify();
        Ok(())
    }

    fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        let (Some(buf), Some(m)) = (bufs.first_mut(), meta.first_mut()) else {
            return Poll::Ready(Ok(0));
        };
        let mut rx = self.recv_rx.lock().unwrap_or_else(|e| e.into_inner());
        let datagram = rx.poll_recv(cx);
        if datagram.is_ready() {
            took_datagram(&self.read_stalled, &self.kick);
        }
        match datagram {
            Poll::Ready(Some(Inbound { data, from, to })) => {
                let len = data.len().min(buf.len());
                buf[..len].copy_from_slice(&data[..len]);
                *m = RecvMeta { addr: from, len, stride: len, dst_ip: Some(to.ip()) };
                Poll::Ready(Ok(1))
            }
            Poll::Ready(None) => Poll::Ready(Err(udp_closed())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local)
    }

    /// The stack refuses a datagram over the MTU, but with the feature
    /// `ipv4-fragmentation`, which fragments an IPv4 datagram.
    fn may_fragment(&self) -> bool {
        crate::IPV4_FRAGMENTATION
    }
}
