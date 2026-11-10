//! [`Datagrams`] over a [`Link`] that has only async send and receive: the
//! UDP socket of a userspace stack (podnet's under WARP), which has no poll
//! interface.
//!
//! The edge of Cloudflare prefers QUIC on UDP 7844. WARP carries UDP, so the
//! missing piece is a socket of QUIC on the userspace stack in place of a
//! socket of the kernel. A sealed host can refuse `bind(2)` on `AF_INET` and
//! `connect(2)` to each destination, the loopback too. The userspace stack
//! has a UDP socket in the address space of the tunnel, so this adapter moves
//! datagrams between QUIC and that socket, and the kernel is never asked.
//!
//! * [`QueuedDatagrams::try_send`](Datagrams::try_send) pushes into a bounded
//!   queue; one task writes each datagram to the link.
//! * A second task reads from the link into a queue;
//!   [`poll_recv`](Datagrams::poll_recv) pops from it.
//! * [`poll_writable`](Datagrams::poll_writable) keeps the waker of its task
//!   when the queue is full, and the writer wakes it after each datagram, so
//!   a full queue is backpressure and not a spin.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use pod_core::{BoxFuture, Datagrams, RecvMeta, Transmit};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Datagrams that may sit in the send queue before QUIC is told to wait.
pub const SEND_QUEUE: usize = 1024;
/// The largest datagram read from the link.
const MAX_DATAGRAM: usize = 65_535;

/// A way of datagrams with async send and receive and no poll interface.
pub trait Link: Send + Sync + 'static {
    /// Send `datagram` to `to`. An error ends the link.
    fn send_to(&self, datagram: Vec<u8>, to: SocketAddr) -> BoxFuture<'_, io::Result<()>>;
    /// The next datagram of at most `max` bytes, and its sender. An error
    /// ends the link.
    fn recv_from(&self, max: usize) -> BoxFuture<'_, io::Result<(Vec<u8>, SocketAddr)>>;
    fn local_addr(&self) -> SocketAddr;
}

/// Datagrams waiting for `poll_recv`, with the peer each came from.
type Inbound = Arc<Mutex<VecDeque<(Vec<u8>, SocketAddr)>>>;
/// The send queue: the destination and the datagram.
type Out = mpsc::Sender<(SocketAddr, Vec<u8>)>;

/// [`Datagrams`] over a [`Link`], through a send queue and a receive queue.
pub struct QueuedDatagrams {
    out: Out,
    inbound: Inbound,
    recv_waker: Arc<AtomicWaker>,
    send_ready: SendReady,
    local: SocketAddr,
    sent: AtomicU64,
    received: AtomicU64,
    closed: Arc<AtomicBool>,
    tasks: Vec<JoinHandle<()>>,
}

impl fmt::Debug for QueuedDatagrams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueuedDatagrams")
            .field("local", &self.local)
            .field("sent", &self.sent.load(Ordering::Relaxed))
            .field("received", &self.received.load(Ordering::Relaxed))
            .finish()
    }
}

impl QueuedDatagrams {
    /// The queues over `link`, with a send queue of [`SEND_QUEUE`]
    /// datagrams, and the two tasks that move the datagrams. It must run in
    /// a tokio runtime. A drop stops both tasks.
    pub fn new(link: Arc<dyn Link>) -> QueuedDatagrams {
        QueuedDatagrams::with_queue(link, SEND_QUEUE)
    }

    /// [`QueuedDatagrams::new`] with a send queue of `queue` datagrams (at
    /// least one).
    pub fn with_queue(link: Arc<dyn Link>, queue: usize) -> QueuedDatagrams {
        let (out, mut out_rx) = mpsc::channel::<(SocketAddr, Vec<u8>)>(queue.max(1));
        let inbound: Inbound = Arc::new(Mutex::new(VecDeque::new()));
        let recv_waker = Arc::new(AtomicWaker::new());
        let send_ready = SendReady::default();
        let closed = Arc::new(AtomicBool::new(false));

        let writer = {
            let (link, send_ready, closed) = (link.clone(), send_ready.clone(), closed.clone());
            tokio::spawn(async move {
                while let Some((to, data)) = out_rx.recv().await {
                    // The queue has room again: wake a parked poller.
                    send_ready.wake();
                    if link.send_to(data, to).await.is_err() {
                        closed.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                // A poller waits no more on a queue that nobody drains.
                send_ready.wake();
            })
        };

        let reader = {
            let (link, inbound, recv_waker, closed) =
                (link.clone(), inbound.clone(), recv_waker.clone(), closed.clone());
            tokio::spawn(async move {
                loop {
                    match link.recv_from(MAX_DATAGRAM).await {
                        Ok((data, from)) => {
                            inbound.lock().unwrap_or_else(|e| e.into_inner()).push_back((data, from));
                            recv_waker.wake();
                        }
                        Err(_) => {
                            closed.store(true, Ordering::Relaxed);
                            recv_waker.wake();
                            break;
                        }
                    }
                }
            })
        };

        QueuedDatagrams {
            out,
            inbound,
            recv_waker,
            send_ready,
            local: link.local_addr(),
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            closed,
            tasks: vec![writer, reader],
        }
    }

    /// The datagrams that the queue took, and those that QUIC read.
    pub fn counts(&self) -> (u64, u64) {
        (self.sent.load(Ordering::Relaxed), self.received.load(Ordering::Relaxed))
    }

    fn pop(&self) -> Option<(Vec<u8>, SocketAddr)> {
        self.inbound.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
    }
}

impl Drop for QueuedDatagrams {
    fn drop(&mut self) {
        for task in self.tasks.drain(..) {
            task.abort();
        }
    }
}

fn broken() -> io::Error {
    io::Error::from(io::ErrorKind::BrokenPipe)
}

impl Datagrams for QueuedDatagrams {
    fn poll_writable(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        poll_room(&self.out, &self.send_ready, cx)
    }

    fn try_send(&self, transmit: &Transmit<'_>) -> io::Result<()> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(broken());
        }
        // A transmit of several segments goes whole or not at all, so that
        // a full queue never sends a part of it twice.
        let size = transmit.segment_size.filter(|s| *s > 0).unwrap_or(transmit.contents.len().max(1));
        let chunks = transmit.contents.chunks(size);
        if chunks.len() > self.out.capacity() {
            return match self.out.is_closed() {
                true => Err(broken()),
                false => Err(io::Error::from(io::ErrorKind::WouldBlock)),
            };
        }
        for chunk in chunks {
            match self.out.try_send((transmit.destination, chunk.to_vec())) {
                Ok(()) => {
                    self.sent.fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::error::TrySendError::Full(_)) => return Err(io::Error::from(io::ErrorKind::WouldBlock)),
                Err(mpsc::error::TrySendError::Closed(_)) => return Err(broken()),
            }
        }
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
        loop {
            match self.pop() {
                Some((data, from)) => {
                    let len = data.len().min(buf.len());
                    buf[..len].copy_from_slice(&data[..len]);
                    *m = RecvMeta { addr: from, len, stride: len, dst_ip: None };
                    self.received.fetch_add(1, Ordering::Relaxed);
                    return Poll::Ready(Ok(1));
                }
                None => {
                    // The link is gone and the queue is empty: a receive can
                    // never succeed again.
                    if self.closed.load(Ordering::Relaxed) {
                        return Poll::Ready(Err(broken()));
                    }
                    // Register before the check again, so that a datagram
                    // that comes between the pop and the register still
                    // wakes the task.
                    self.recv_waker.register(cx.waker());
                    if self.inbound.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
                        && !self.closed.load(Ordering::Relaxed)
                    {
                        return Poll::Pending;
                    }
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local)
    }
}

/// The wake-up of a poller that waits for room in the send queue (podnet's
/// T-225). An `AtomicWaker` keeps the waker of the task between two polls; a
/// `Notified` future made in each poll drops out of the waiters at
/// `Pending`, and a later drain then wakes nobody. It keeps one waker: the
/// socket of QUIC gives one waker for each of its senders
/// ([`crate::socket`]).
#[derive(Clone, Default, Debug)]
struct SendReady(Arc<AtomicWaker>);

impl SendReady {
    fn register(&self, cx: &Context<'_>) {
        self.0.register(cx.waker());
    }

    fn wake(&self) {
        self.0.wake();
    }
}

/// Ready when `out` has room, or when nobody drains it (the send then fails
/// at once).
fn poll_room(out: &Out, ready: &SendReady, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    if out.capacity() > 0 {
        return Poll::Ready(Ok(()));
    }
    // Register before the check again, so that a drain between the first
    // check and the register still wakes the task.
    ready.register(cx);
    if out.capacity() > 0 || out.is_closed() {
        return Poll::Ready(Ok(()));
    }
    Poll::Pending
}

#[cfg(test)]
#[path = "queued_tests.rs"]
mod tests;
