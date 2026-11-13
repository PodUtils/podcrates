//! KCP over a datagram carrier as one ordered byte stream: the handle of
//! the caller (`AsyncRead` and `AsyncWrite`), and the task that drives the
//! carrier and the timer of KCP.

use std::io::{self, Write};
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use kcp::Kcp;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;
use tokio::time::{Instant, sleep_until};

use super::settings::{KCP_HEADER, KcpSettings, KcpSettingsError};
use crate::datagram::Datagrams;

/// After the drop of the stream, the task answers the peer until it was
/// silent this long, so that the peer has the acknowledgement of its last
/// segments.
const QUIET: Duration = Duration::from_secs(3);

/// The most segments of one write: KCP takes fewer than 128 in one call.
const WRITE_SEGMENTS: usize = 64;

/// The datagrams that KCP wrote in one turn: each write is one datagram.
/// KCP owns its output, so the task reads them through a second handle.
#[derive(Default, Clone)]
struct Out(Arc<Mutex<Vec<Vec<u8>>>>);

impl Out {
    fn take(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Write for Out {
    fn write(&mut self, datagram: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(datagram.to_vec());
        Ok(datagram.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Inner {
    kcp: Kcp<Out>,
    out: Out,
    start: std::time::Instant,
    /// The bytes of a segment that the reader did not take.
    left: Vec<u8>,
    reader: Option<Waker>,
    writer: Option<Waker>,
    /// Why the stream ended.
    error: Option<(io::ErrorKind, String)>,
    /// The last datagram of the peer.
    heard: std::time::Instant,
    /// The drop of the stream, when it came.
    dropped: Option<std::time::Instant>,
    /// The segments that the writer can queue before it waits.
    queue: usize,
    segment: usize,
    idle: Duration,
}

impl Inner {
    fn wake(&mut self) {
        self.reader.take().into_iter().chain(self.writer.take()).for_each(Waker::wake);
    }

    fn now_ms(&self, now: std::time::Instant) -> u32 {
        // KCP's clock is 32 bits of milliseconds, compared with wrap.
        now.duration_since(self.start).as_millis() as u32
    }

    fn fail(&mut self, kind: io::ErrorKind, text: String) {
        self.error.get_or_insert((kind, text));
        self.wake();
    }

    fn error(&self) -> Option<io::Error> {
        self.error.as_ref().map(|(kind, text)| io::Error::new(*kind, text.clone()))
    }
}

struct Shared {
    inner: Mutex<Inner>,
    /// Wakes the task: a write, a read, a drop.
    sender: Notify,
}

impl Shared {
    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        f(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

fn now() -> std::time::Instant {
    Instant::now().into_std()
}

/// One ordered byte stream over a datagram carrier: KCP in stream mode in
/// a task of the current tokio runtime, with the wire of KCP. The stream
/// has no end of its own: the TLS above it gives one (`close_notify`). A
/// shutdown returns when the peer acknowledged each byte. A drop lets the
/// task send the bytes in flight and answer the peer until it is silent
/// for 3 s (at most `idle`). A silence of the peer for `idle`, or a segment
/// sent `dead_link` times (20), ends the stream with `TimedOut`.
pub struct KcpStream {
    shared: Arc<Shared>,
}

impl KcpStream {
    /// Start KCP over `carrier` with the conversation `conv`, or with the
    /// conversation of the first datagram when `conv` is `None`. It must run
    /// in a tokio runtime with the time driver on.
    pub fn new<D: Datagrams>(
        carrier: D,
        conv: Option<u32>,
        settings: KcpSettings,
    ) -> Result<KcpStream, KcpSettingsError> {
        settings.check()?;
        let segment = settings.segment.min(carrier.mtu().saturating_sub(KCP_HEADER));
        if segment < 32 {
            return Err(KcpSettingsError::Segment(segment, carrier.mtu()));
        }
        let out = Out::default();
        let mut kcp = Kcp::new_stream(conv.unwrap_or(0), out.clone());
        if conv.is_none() {
            kcp.input_conv();
        }
        kcp.set_mtu(segment + KCP_HEADER).map_err(|_| KcpSettingsError::Segment(segment, carrier.mtu()))?;
        kcp.set_wndsize(settings.window, settings.window);
        kcp.set_nodelay(settings.nodelay, settings.interval_ms() as i32, settings.resend as i32, settings.nodelay);
        // The first update starts the clock of KCP; a flush needs it.
        let _ = kcp.update(0);
        let start = now();
        let inner = Inner {
            kcp,
            out,
            start,
            left: Vec::new(),
            reader: None,
            writer: None,
            error: None,
            heard: start,
            dropped: None,
            queue: 2 * usize::from(settings.window),
            segment,
            idle: settings.idle,
        };
        let shared = Arc::new(Shared { inner: Mutex::new(inner), sender: Notify::new() });
        tokio::spawn(drive(shared.clone(), carrier));
        Ok(KcpStream { shared })
    }

    /// The conversation of the stream: the one given, or the one of the
    /// first datagram.
    pub fn conv(&self) -> u32 {
        self.shared.with(|i| i.kcp.conv())
    }
}

impl std::fmt::Debug for KcpStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.shared.with(|i| f.debug_tuple("KcpStream").field(&i.kcp.conv()).field(&i.kcp.wait_snd()).finish())
    }
}

/// The datagram has a segment that KCP's stream mode does not send: a
/// fragment of a message (`frg` not 0). The reader of KCP adds 1 to `frg`
/// in a `u8` (an overflow at 255) and waits for the other fragments for
/// ever, so such a datagram of a stranger does not go in.
fn foreign(datagram: &[u8]) -> bool {
    let mut rest = datagram;
    while rest.len() >= KCP_HEADER {
        if rest[5] != 0 {
            return true;
        }
        let len = u32::from_le_bytes([rest[20], rest[21], rest[22], rest[23]]) as usize;
        rest = rest.get(KCP_HEADER.saturating_add(len)..).unwrap_or_default();
    }
    false
}

impl AsyncRead for KcpStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let polled = self.shared.with(|i| {
            let before = buf.filled().len();
            while buf.remaining() > 0 {
                if i.left.is_empty() {
                    // A segment of the peer can be larger than one of this
                    // end (a larger setting, or a stranger's): no stall.
                    let Ok(size) = i.kcp.peeksize() else { break };
                    let mut segment = vec![0u8; size];
                    match i.kcp.recv(&mut segment) {
                        Ok(n) => i.left.extend_from_slice(&segment[..n]),
                        Err(_) => break,
                    }
                }
                let n = i.left.len().min(buf.remaining());
                buf.put_slice(&i.left[..n]);
                i.left.drain(..n);
            }
            if buf.filled().len() > before {
                return Poll::Ready(Ok(true));
            }
            if let Some(e) = i.error() {
                return Poll::Ready(Err(e));
            }
            i.reader = Some(cx.waker().clone());
            Poll::Pending
        });
        // A read opens the window: the task tells the peer.
        polled.map_ok(|read| {
            if read {
                self.shared.sender.notify_one();
            }
        })
    }
}

impl AsyncWrite for KcpStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let polled = self.shared.with(|i| {
            if let Some(e) = i.error() {
                return Poll::Ready(Err(e));
            }
            if buf.is_empty() {
                return Poll::Ready(Ok(0));
            }
            if i.kcp.wait_snd() >= i.queue {
                i.writer = Some(cx.waker().clone());
                return Poll::Pending;
            }
            let n = buf.len().min(WRITE_SEGMENTS * i.segment);
            Poll::Ready(i.kcp.send(&buf[..n]).map_err(io::Error::from))
        });
        if matches!(polled, Poll::Ready(Ok(n)) if n > 0) {
            self.shared.sender.notify_one();
        }
        polled
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.shared.with(|i| i.error()).map_or(Ok(()), Err))
    }

    /// Ready when the peer acknowledged each byte.
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.shared.with(|i| {
            if let Some(e) = i.error() {
                return Poll::Ready(Err(e));
            }
            if i.kcp.wait_snd() == 0 {
                return Poll::Ready(Ok(()));
            }
            i.writer = Some(cx.waker().clone());
            Poll::Pending
        })
    }
}

impl Drop for KcpStream {
    fn drop(&mut self) {
        self.shared.with(|i| i.dropped = Some(now()));
        self.shared.sender.notify_one();
    }
}

/// The end of the task, also by a panic: the stream fails, and its reader
/// and its writer wake.
struct Ended(Arc<Shared>);

impl Drop for Ended {
    fn drop(&mut self) {
        self.0.with(|i| i.fail(io::ErrorKind::BrokenPipe, "KCP: its task ended".into()));
    }
}

/// The task of KCP: the receive loop, and the send loop, which also runs
/// the timer. It ends at an error, or after the drop when nothing waits and
/// the peer is quiet. A panic of the code of KCP (a datagram of a stranger)
/// ends the stream with an error, as each end does.
async fn drive<D: Datagrams>(shared: Arc<Shared>, carrier: D) {
    let _ended = Ended(shared.clone());
    let receive = async {
        loop {
            let datagram = match carrier.recv().await {
                Ok(d) => d,
                Err(e) => {
                    shared.with(|i| i.fail(e.kind(), format!("the datagrams of KCP: {e}")));
                    return;
                }
            };
            // A datagram of another form or conversation is dropped: a
            // carrier of datagrams can give bytes of a stranger.
            let taken = shared.with(|i| {
                let taken = !foreign(&datagram) && i.kcp.input(&datagram).is_ok();
                if taken {
                    i.heard = now();
                    i.wake();
                }
                taken
            });
            if taken {
                shared.sender.notify_one();
            }
        }
    };
    let send = async {
        loop {
            let mut wake = pin!(shared.sender.notified());
            wake.as_mut().enable();
            let (out, deadline, stop) = shared.with(step);
            for d in out {
                if let Err(e) = carrier.send(&d).await {
                    shared.with(|i| i.fail(e.kind(), format!("the datagrams of KCP: {e}")));
                    return;
                }
            }
            if stop {
                return;
            }
            tokio::select! {
                () = sleep_until(Instant::from_std(deadline)) => {}
                () = wake => {}
            }
        }
    };
    tokio::select! {
        () = receive => {}
        () = send => {}
    }
}

/// One turn of the send loop under the lock: the timer and the flush of
/// KCP, the datagrams to send, the next deadline, and whether the task
/// stops.
fn step(i: &mut Inner) -> (Vec<Vec<u8>>, std::time::Instant, bool) {
    let now = now();
    let ms = i.now_ms(now);
    // The flush sends the acknowledgements and the segments that the
    // window allows at once, not at the next tick of the timer.
    let flushed = i.kcp.update(ms).and_then(|()| i.kcp.flush());
    if let Err(e) = flushed {
        i.fail(io::ErrorKind::Other, format!("KCP: {e}"));
    }
    if i.kcp.is_dead_link() {
        i.fail(io::ErrorKind::TimedOut, "KCP: the peer acknowledged no segment after 20 sends".into());
    }
    if now.duration_since(i.heard) >= i.idle {
        i.fail(io::ErrorKind::TimedOut, format!("KCP: the peer was silent for {:?}", i.idle));
    }
    let out = i.out.take();
    // A writer waits for room in the queue; a shutdown waits for the last
    // acknowledgement.
    if i.kcp.wait_snd() < i.queue
        && let Some(writer) = i.writer.take()
    {
        writer.wake();
    }
    if i.error.is_some() {
        return (out, now, true);
    }
    let next = Duration::from_millis(u64::from(i.kcp.check(ms)).max(1));
    let mut deadline = (now + next).min(i.heard + i.idle);
    if let Some(dropped) = i.dropped {
        let end = (i.heard + QUIET).min(dropped + i.idle);
        if i.kcp.wait_snd() == 0 && now >= end {
            return (out, now, true);
        }
        deadline = deadline.min(end.max(now + next));
    }
    (out, deadline, false)
}
