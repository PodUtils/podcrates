//! The bounded packet queue of a link: the device and its pump share it.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use super::DeviceStats;

struct QueueInner {
    packets: VecDeque<Vec<u8>>,
    waker: Option<Waker>,
    capacity: usize,
    dropped: u64,
    /// Where to count a drop too (`rx_dropped` of a device).
    sink: Option<Arc<DeviceStats>>,
}

/// A bounded packet queue with a waker, shared by the device and its pump.
#[derive(Clone)]
pub struct PacketQueue {
    inner: Arc<Mutex<QueueInner>>,
}

impl std::fmt::Debug for PacketQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.lock();
        f.debug_struct("PacketQueue")
            .field("len", &inner.packets.len())
            .field("capacity", &inner.capacity)
            .field("dropped", &inner.dropped)
            .finish()
    }
}

impl PacketQueue {
    /// A queue of at most `capacity` packets. A push to a full queue drops
    /// the packet and counts it: a link that is faster than the stack then
    /// makes TCP send again, and the heap does not grow without a limit.
    pub fn bounded(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(QueueInner {
                packets: VecDeque::new(),
                waker: None,
                capacity: capacity.max(1),
                dropped: 0,
                sink: None,
            })),
        }
    }

    pub fn unbounded() -> Self {
        Self::bounded(usize::MAX)
    }

    /// A poisoned lock still holds a consistent queue: each change under it
    /// is one push or one pop.
    fn lock(&self) -> MutexGuard<'_, QueueInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Count each drop of this queue in `rx_dropped` of `stats`. Call it
    /// before the queue is shared.
    pub(super) fn counting_in(self, stats: Arc<DeviceStats>) -> Self {
        self.lock().sink = Some(stats);
        self
    }

    /// Push a packet and wake a reader. False when the queue was full and
    /// dropped the packet.
    pub fn push(&self, packet: Vec<u8>) -> bool {
        let waker = {
            let mut inner = self.lock();
            if inner.packets.len() >= inner.capacity {
                inner.dropped += 1;
                if let Some(stats) = &inner.sink {
                    stats.rx_dropped.fetch_add(1, Ordering::Relaxed);
                }
                return false;
            }
            inner.packets.push_back(packet);
            inner.waker.take()
        };
        // Wake with no lock held: a waker that calls `push` or `pop` must not
        // lock the mutex again.
        if let Some(waker) = waker {
            waker.wake();
        }
        true
    }

    pub fn pop(&self) -> Option<Vec<u8>> {
        self.lock().packets.pop_front()
    }

    /// Read the next packet with `f`; the packet stays in the queue. The
    /// reactor reads the head of each inbound packet before smoltcp takes
    /// it.
    pub fn peek<R>(&self, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
        self.lock().packets.front().map(|packet| f(packet))
    }

    pub fn len(&self) -> usize {
        self.lock().packets.len()
    }

    /// True when a push now drops the packet.
    pub fn is_full(&self) -> bool {
        let inner = self.lock();
        inner.packets.len() >= inner.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn dropped(&self) -> u64 {
        self.lock().dropped
    }

    pub fn clear(&self) {
        self.lock().packets.clear();
    }

    /// Take the next packet, or keep the waker of `cx` for the next push.
    pub fn poll_recv(&self, cx: &mut Context<'_>) -> Poll<Vec<u8>> {
        let mut inner = self.lock();
        if let Some(packet) = inner.packets.pop_front() {
            return Poll::Ready(packet);
        }
        inner.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    /// Wait for the next packet and take it.
    pub async fn recv(&self) -> Vec<u8> {
        std::future::poll_fn(|cx| self.poll_recv(cx)).await
    }

    /// Ready when the queue has a packet; the packet stays in the queue.
    pub fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
        let mut inner = self.lock();
        if !inner.packets.is_empty() {
            return Poll::Ready(());
        }
        inner.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    /// Wait until a packet is in the queue (it stays for the device).
    pub async fn wait(&self) {
        std::future::poll_fn(|cx| self.poll_ready(cx)).await
    }

    /// Wait for a packet, or `None` when `should_stop` becomes true first:
    /// a pump stops at the shutdown.
    pub async fn recv_or<F: Fn() -> bool>(&self, should_stop: F) -> Option<Vec<u8>> {
        std::future::poll_fn(|cx| {
            if should_stop() {
                return Poll::Ready(None);
            }
            self.poll_recv(cx).map(Some)
        })
        .await
    }
}
