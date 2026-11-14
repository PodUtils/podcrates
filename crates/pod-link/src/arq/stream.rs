//! The ARQ over a datagram carrier as one ordered byte stream: the handle
//! of the caller (`AsyncRead` and `AsyncWrite`), and the task that drives
//! the carrier and the timers.

use std::io;
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until};

use super::error::ArqError;
use super::machine::Arq;
use super::settings::ArqSettings;
use super::stats::Stats;
use crate::datagram::Datagrams;

/// The most datagrams that the task takes from the ARQ under one lock.
const BATCH: usize = 64;

/// After the drop of the stream, the task ends when the peer was silent
/// for this many RTO ceilings: a frame of the peer comes again within one
/// ceiling, so two cover one lost frame or `ACK`.
const QUIET_RTOS: u32 = 2;

/// The most time that the task answers after the drop, in silences: a
/// peer that sends on to a dropped stream does not hold the task.
const LINGER_QUIETS: u32 = 4;

struct Inner {
    arq: Arq,
    reader: Option<Waker>,
    writer: Option<Waker>,
    /// The handle of the stream was dropped.
    dropped: bool,
    /// The last datagram of the peer.
    heard: std::time::Instant,
    /// After the drop, when nothing of this end waited any more.
    idle_since: Option<std::time::Instant>,
    /// The silence of the peer that ends the task after the drop.
    quiet: Duration,
    /// The task answers each rekey of the ARQ itself ([`ArqStream::wrap_freely`]).
    free_wrap: bool,
}

impl Inner {
    fn wake(&mut self) {
        self.reader.take().into_iter().chain(self.writer.take()).for_each(Waker::wake);
    }
}

struct Shared {
    inner: Mutex<Inner>,
    /// Wakes the send loop: a write, a read, a datagram.
    sender: Notify,
    /// Wakes the callers of [`ArqHandle::rekey_wanted`].
    rekey: Notify,
}

impl Shared {
    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        f(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

fn now() -> std::time::Instant {
    Instant::now().into_std()
}

/// One ordered byte stream over a datagram carrier: the selective-repeat
/// ARQ ([`Arq`]) in a task of the current tokio runtime. A
/// [`crate::Link`] runs over it as over any byte stream. A shutdown sends
/// the `FIN` and waits for its acknowledgement. A drop also sends the
/// `FIN`; after the drop, when each frame of this end is acknowledged, the
/// task answers the peer until the peer is silent for two RTO ceilings
/// (at most eight), then ends. An error of the ARQ also ends it.
pub struct ArqStream {
    shared: Arc<Shared>,
    task: Option<JoinHandle<()>>,
}

/// A handle of an [`ArqStream`] that stays when the stream moves into a
/// link: the error, the counters, and the rekey before the wrap.
#[derive(Clone)]
pub struct ArqHandle {
    shared: Arc<Shared>,
}

impl ArqStream {
    /// Start the ARQ over `carrier`. It must run in a tokio runtime with
    /// the time driver on.
    pub fn new<D: Datagrams>(carrier: D, settings: ArqSettings) -> Result<ArqStream, ArqError> {
        let arq = Arq::new(settings, carrier.mtu())?;
        let inner = Inner {
            arq,
            reader: None,
            writer: None,
            dropped: false,
            heard: now(),
            idle_since: None,
            quiet: settings.rto_ceiling * QUIET_RTOS,
            free_wrap: false,
        };
        let shared = Arc::new(Shared { inner: Mutex::new(inner), sender: Notify::new(), rekey: Notify::new() });
        let task = tokio::spawn(drive(shared.clone(), carrier));
        Ok(ArqStream { shared, task: Some(task) })
    }

    /// A handle that stays when the stream moves.
    pub fn handle(&self) -> ArqHandle {
        ArqHandle { shared: self.shared.clone() }
    }

    /// The task calls [`Arq::rekeyed`] at each ask, so the sequence wraps
    /// with no wait. Only for a layer above that seals each byte with
    /// nonces of its own (TLS): a sequence of the ARQ used again then
    /// opens no nonce again.
    #[cfg_attr(not(any(feature = "dns", feature = "icmp")), allow(dead_code))]
    pub(crate) fn wrap_freely(&self) {
        self.shared.with(|i| i.free_wrap = true);
        self.shared.sender.notify_one();
    }
}

impl ArqHandle {
    /// Why the ARQ ended, or `None` while it runs.
    pub fn error(&self) -> Option<ArqError> {
        self.shared.with(|i| i.arq.error().cloned())
    }

    pub fn stats(&self) -> Stats {
        self.shared.with(|i| i.arq.stats())
    }

    /// The smoothed round trip, after the first sample.
    pub fn srtt(&self) -> Option<Duration> {
        self.shared.with(|i| i.arq.srtt())
    }

    /// The frames in flight.
    pub fn in_flight(&self) -> usize {
        self.shared.with(|i| i.arq.in_flight())
    }

    /// The sequence nears its wrap: rekey, then call [`ArqHandle::rekeyed`].
    pub fn rekey_due(&self) -> bool {
        self.shared.with(|i| i.arq.rekey_due())
    }

    /// The caller rekeyed: the sequence can wrap to 0 one time.
    pub fn rekeyed(&self) {
        self.shared.with(|i| i.arq.rekeyed());
        self.shared.sender.notify_one();
    }

    /// Wait until the ARQ asks for a rekey.
    pub async fn rekey_wanted(&self) {
        loop {
            let mut notice = pin!(self.shared.rekey.notified());
            notice.as_mut().enable();
            if self.rekey_due() {
                return;
            }
            notice.await;
        }
    }
}

impl std::fmt::Debug for ArqHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.shared.with(|i| f.debug_tuple("ArqHandle").field(&i.arq).finish())
    }
}

impl std::fmt::Debug for ArqStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.shared.with(|i| f.debug_tuple("ArqStream").field(&i.arq).finish())
    }
}

impl AsyncRead for ArqStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let shared = &self.shared;
        let polled = shared.with(|i| match i.arq.read(buf.initialize_unfilled()) {
            Err(e) => Poll::Ready(Err(io::Error::from(e))),
            Ok(0) if buf.remaining() > 0 && !i.arq.read_finished() => {
                i.reader = Some(cx.waker().clone());
                Poll::Pending
            }
            Ok(n) => {
                buf.advance(n);
                Poll::Ready(Ok(()))
            }
        });
        if polled.is_ready() {
            // A read can open the window: the task sends its `ACK`.
            shared.sender.notify_one();
        }
        polled
    }
}

impl AsyncWrite for ArqStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let polled = self.shared.with(|i| match i.arq.write(buf) {
            Err(e) => Poll::Ready(Err(io::Error::from(e))),
            Ok(0) if !buf.is_empty() => {
                i.writer = Some(cx.waker().clone());
                Poll::Pending
            }
            Ok(n) => Poll::Ready(Ok(n)),
        });
        if matches!(polled, Poll::Ready(Ok(n)) if n > 0) {
            self.shared.sender.notify_one();
        }
        polled
    }

    /// Each buffer in one call, in order. rustls writes the alert of a
    /// failed handshake with one vectored write and no second try; a write
    /// of the first buffer alone would leave the alert out (T-557).
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let polled = self.shared.with(|i| {
            let mut n = 0;
            for buf in bufs.iter().filter(|b| !b.is_empty()) {
                match i.arq.write(buf) {
                    Err(e) if n == 0 => return Poll::Ready(Err(io::Error::from(e))),
                    Err(_) => break,
                    Ok(k) => {
                        n += k;
                        if k < buf.len() {
                            break;
                        }
                    }
                }
            }
            if n == 0 && bufs.iter().any(|b| !b.is_empty()) {
                i.writer = Some(cx.waker().clone());
                return Poll::Pending;
            }
            Poll::Ready(Ok(n))
        });
        if matches!(polled, Poll::Ready(Ok(n)) if n > 0) {
            self.shared.sender.notify_one();
        }
        polled
    }

    fn is_write_vectored(&self) -> bool {
        true
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.shared.with(|i| i.arq.error().cloned()).map_or(Ok(()), |e| Err(e.into())))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let (polled, first) = self.shared.with(|i| {
            if let Some(e) = i.arq.error() {
                return (Poll::Ready(Err(e.clone().into())), false);
            }
            let first = !i.arq.write_closed();
            i.arq.close();
            if i.arq.closed() {
                return (Poll::Ready(Ok(())), first);
            }
            i.writer = Some(cx.waker().clone());
            (Poll::Pending, first)
        });
        // Only the poll that queues the `FIN` wakes the task: a notice on
        // each poll would make a loop with the task's wake of this writer.
        if first {
            self.shared.sender.notify_one();
        }
        polled
    }
}

impl Drop for ArqStream {
    fn drop(&mut self) {
        self.shared.with(|i| {
            i.dropped = true;
            i.arq.close();
        });
        self.shared.sender.notify_one();
        // The task ends by itself; it is not aborted, so that the `FIN` and
        // the frames in flight reach the peer.
        drop(self.task.take());
    }
}

/// The task of the ARQ: the receive loop and the send loop, which also runs
/// the timers. It ends at an error of the ARQ or of the carrier, or after
/// the drop of the stream when nothing waits.
async fn drive<D: Datagrams>(shared: Arc<Shared>, carrier: D) {
    let receive = async {
        loop {
            let datagram = match carrier.recv().await {
                Ok(d) => d,
                Err(e) => {
                    shared.with(|i| fail(i, ArqError::io(&e)));
                    return;
                }
            };
            shared.with(|i| {
                i.heard = now();
                i.arq.on_datagram(i.heard, &datagram);
                if i.dropped {
                    while let Ok(Some(_)) = i.arq.recv_record() {}
                }
                i.wake();
            });
            shared.sender.notify_one();
        }
    };
    let send = async {
        let mut out = Vec::with_capacity(BATCH);
        loop {
            let mut wake = pin!(shared.sender.notified());
            wake.as_mut().enable();
            let (deadline, stop, rekey) = shared.with(|i| step(i, &mut out));
            if rekey {
                shared.rekey.notify_waiters();
            }
            for d in out.drain(..) {
                if let Err(e) = carrier.send(&d).await {
                    shared.with(|i| fail(i, ArqError::io(&e)));
                    return;
                }
            }
            if stop {
                return;
            }
            match deadline {
                Some(at) => tokio::select! {
                    () = sleep_until(Instant::from_std(at)) => {}
                    () = wake => {}
                },
                None => wake.await,
            }
        }
    };
    tokio::select! {
        () = receive => {}
        () = send => {}
    }
    shared.with(Inner::wake);
}

fn fail(i: &mut Inner, e: ArqError) {
    i.arq.fail(e);
    i.wake();
}

/// One turn of the send loop under the lock: the timers, the datagrams to
/// send, the next deadline, whether the task stops, and whether the ARQ
/// asks for a rekey.
fn step(i: &mut Inner, out: &mut Vec<Vec<u8>>) -> (Option<std::time::Instant>, bool, bool) {
    let now = now();
    if i.arq.poll_timeout().is_some_and(|at| at <= now) {
        i.arq.on_timeout(now);
    }
    if i.free_wrap && i.arq.rekey_due() {
        i.arq.rekeyed();
    }
    while out.len() < BATCH {
        let Some(d) = i.arq.poll_transmit(now) else { break };
        out.push(d);
    }
    if i.arq.error().is_some() {
        i.wake();
        return (None, true, false);
    }
    let rekey = i.arq.rekey_due();
    let mut deadline = i.arq.poll_timeout();
    if i.dropped && i.arq.idle() {
        // The peer may not have the `ACK` of its last frames: answer its
        // frames again until it is quiet, as TCP's TIME-WAIT does.
        let end = (i.heard + i.quiet).min(*i.idle_since.get_or_insert(now) + i.quiet * LINGER_QUIETS);
        if now >= end {
            return (None, true, rekey);
        }
        deadline = Some(deadline.map_or(end, |d| d.min(end)));
    }
    if out.len() == BATCH {
        // More datagrams wait: come back at once.
        deadline = Some(now);
    }
    (deadline, false, rekey)
}
