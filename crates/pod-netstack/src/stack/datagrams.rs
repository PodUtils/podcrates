//! A UDP socket of the stack as [`pod_core::Datagrams`], so that a user of
//! datagrams (QUIC, a DNS client) sends UDP through the stack.

use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use pod_core::{Datagrams, RecvMeta, Transmit};
use tokio::sync::{Notify, mpsc};

use super::UdpSocket;
use super::socket::udp_closed;

/// The datagrams of one UDP socket of the stack.
///
/// A send does not wait: when the queue to the stack is full, the datagram
/// is dropped, as a full UDP buffer of the kernel drops it.
pub struct UdpDatagrams {
    id: u32,
    local: SocketAddr,
    recv_rx: Mutex<mpsc::Receiver<(Vec<u8>, SocketAddr)>>,
    send_tx: mpsc::Sender<(Vec<u8>, SocketAddr)>,
    kick: Arc<Notify>,
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
            kick: self.kick,
        }
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
        let size = t.segment_size.filter(|s| *s > 0).unwrap_or(t.contents.len().max(1));
        for chunk in t.contents.chunks(size) {
            match self.send_tx.try_send((chunk.to_vec(), t.destination)) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                Err(mpsc::error::TrySendError::Closed(_)) => return Err(udp_closed()),
            }
        }
        self.kick.notify_one();
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
        match rx.poll_recv(cx) {
            Poll::Ready(Some((data, from))) => {
                let len = data.len().min(buf.len());
                buf[..len].copy_from_slice(&data[..len]);
                *m = RecvMeta { addr: from, len, stride: len, dst_ip: Some(self.local.ip()) };
                Poll::Ready(Ok(1))
            }
            Poll::Ready(None) => Poll::Ready(Err(udp_closed())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local)
    }
}
