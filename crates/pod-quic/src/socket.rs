//! The socket of noq over a [`Datagrams`]: the way of the caller (the system
//! socket, WARP, UDP in a stream) carries each datagram of QUIC.
//!
//! noq makes one sender for each task that sends (the endpoint, and each
//! connection), and each sender must keep the waker of its own task. A
//! [`Datagrams`] keeps one waker for [`Datagrams::poll_writable`], as the
//! UDP socket of tokio does. So each sender of this socket gives the way one
//! shared waker, the fan-out, which wakes each sender that waits: a way that
//! is full parks each sender, and its drain wakes each of them.

use std::fmt;
use std::io::{self, IoSliceMut};
use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use noq::udp::{RecvMeta, Transmit};
use noq::{AsyncUdpSocket, UdpSender};
use pod_core::Datagrams;

/// The most buffers that one receive fills.
const BATCH: usize = 32;

/// The socket of noq over `datagrams`.
pub struct DatagramSocket {
    datagrams: Arc<dyn Datagrams>,
    fanout: Arc<Fanout>,
}

impl DatagramSocket {
    pub fn new(datagrams: Arc<dyn Datagrams>) -> DatagramSocket {
        DatagramSocket { datagrams, fanout: Arc::new(Fanout::default()) }
    }
}

impl fmt::Debug for DatagramSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatagramSocket").field("datagrams", &self.datagrams).finish()
    }
}

/// The wakers of the senders that wait for room. The way keeps one waker,
/// this one, and its wake wakes each sender.
#[derive(Default)]
pub(crate) struct Fanout {
    wakers: Mutex<Vec<Waker>>,
}

impl Fanout {
    /// Keep `waker` until the next wake. Call it before each poll of the
    /// way, so that a drain between the poll and the keep wakes it too.
    pub(crate) fn keep(&self, waker: &Waker) {
        let mut wakers = self.wakers.lock().unwrap_or_else(|e| e.into_inner());
        if !wakers.iter().any(|w| w.will_wake(waker)) {
            wakers.push(waker.clone());
        }
    }

    /// Forget `waker`: its sender sent, and waits no more.
    pub(crate) fn forget(&self, waker: &Waker) {
        self.wakers.lock().unwrap_or_else(|e| e.into_inner()).retain(|w| !w.will_wake(waker));
    }
}

impl Wake for Fanout {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let wakers = std::mem::take(&mut *self.wakers.lock().unwrap_or_else(|e| e.into_inner()));
        for waker in wakers {
            waker.wake();
        }
    }
}

impl AsyncUdpSocket for DatagramSocket {
    fn create_sender(&self) -> Pin<Box<dyn UdpSender>> {
        Box::pin(Sender {
            datagrams: self.datagrams.clone(),
            fanout: self.fanout.clone(),
            waker: Waker::from(self.fanout.clone()),
        })
    }

    fn poll_recv(
        &mut self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        let blank = pod_core::RecvMeta { addr: (Ipv4Addr::UNSPECIFIED, 0).into(), len: 0, stride: 0, dst_ip: None };
        let mut ours = [blank; BATCH];
        let n = bufs.len().min(meta.len()).min(BATCH);
        let got = match self.datagrams.poll_recv(cx, &mut bufs[..n], &mut ours[..n]) {
            Poll::Ready(Ok(got)) => got.min(n),
            other => return other,
        };
        for (theirs, ours) in meta.iter_mut().zip(&ours[..got]) {
            let mut m = RecvMeta::default();
            m.addr = ours.addr;
            m.len = ours.len;
            m.stride = ours.stride;
            m.dst_ip = ours.dst_ip;
            *theirs = m;
        }
        Poll::Ready(Ok(got))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.datagrams.local_addr()
    }

    fn max_receive_segments(&self) -> NonZeroUsize {
        NonZeroUsize::new(self.datagrams.max_receive_segments()).unwrap_or(NonZeroUsize::MIN)
    }

    fn may_fragment(&self) -> bool {
        self.datagrams.may_fragment()
    }
}

/// One sender of noq: one task's waker in the fan-out.
struct Sender {
    datagrams: Arc<dyn Datagrams>,
    fanout: Arc<Fanout>,
    /// The fan-out as a waker, made once.
    waker: Waker,
}

impl fmt::Debug for Sender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Sender")
    }
}

impl UdpSender for Sender {
    fn poll_send(self: Pin<&mut Self>, transmit: &Transmit<'_>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let ours = pod_core::Transmit {
            destination: transmit.destination,
            contents: transmit.contents,
            segment_size: transmit.segment_size,
            src_ip: transmit.src_ip,
        };
        loop {
            this.fanout.keep(cx.waker());
            match this.datagrams.poll_writable(&mut Context::from_waker(&this.waker)) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(e)) => {
                    this.fanout.forget(cx.waker());
                    return Poll::Ready(Err(e));
                }
                Poll::Ready(Ok(())) => {}
            }
            match this.datagrams.try_send(&ours) {
                // The way was full after all: poll it again, which parks
                // the sender or finds room.
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                // A way that is gone ends the connection at once, not at
                // the end of its silence.
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
                    this.fanout.forget(cx.waker());
                    return Poll::Ready(Err(e));
                }
                // A datagram that the way refuses is lost, as UDP loses one
                // (noq's own socket does the same): QUIC sends it again, and
                // a probe of the MTU that is too large is refused so.
                Err(e) => tracing::debug!(to = %transmit.destination, error = %e, "a QUIC datagram was not sent"),
                Ok(()) => {}
            }
            this.fanout.forget(cx.waker());
            return Poll::Ready(Ok(()));
        }
    }

    fn max_transmit_segments(&self) -> NonZeroUsize {
        NonZeroUsize::new(self.datagrams.max_transmit_segments()).unwrap_or(NonZeroUsize::MIN)
    }
}

#[cfg(test)]
#[path = "socket_tests.rs"]
mod tests;
