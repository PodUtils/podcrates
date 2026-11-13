//! The pump of the ws carrier: the bytes of the link in binary messages
//! each way, and the rules of the carrier on the session (PodUtils/podnet@T-269):
//!
//! * A text message is an error of protocol: the pump sends a Close 1003
//!   and ends the stream of the link with an error of `InvalidData`.
//! * A Ping each [`Beat::ping_every`]; when no frame comes within
//!   [`Beat::pong_within`] after a Ping, the session is dead, and the
//!   stream of the link ends with an error of `TimedOut`. Any frame counts,
//!   not only the Pong: data that comes behind the Pong proves the peer
//!   too. A message that waits for the link to read it counts as well, as
//!   the reads of the session stop until the link takes it.

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;

use pod_ws::RelaySession;
use pod_ws::close::{close_code_and_reason, code};
use pod_ws::frame;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, DuplexStream, ReadBuf};
use tokio::sync::Notify;
use tokio::time::{Instant, sleep_until};

use crate::carrier::BoxIo;

/// The default time between two Pings of the carrier.
pub const PING_EVERY: Duration = Duration::from_secs(20);

/// The default time within which a frame comes after a Ping.
pub const PONG_WITHIN: Duration = Duration::from_secs(10);

/// The bytes that wait in each direction between the link and the
/// messages of its session.
const BUFFER: usize = 64 << 10;

/// The size of one read of the link, and so of one message.
const CHUNK: usize = 16 << 10;

/// The Pings of a session and the time within which the peer answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    /// The time between two Pings; none sends no Ping.
    pub ping_every: Option<Duration>,
    /// The time within which a frame comes after a Ping. A value of
    /// `ping_every` or more sends the next Ping at the end of the wait.
    pub pong_within: Duration,
}

impl Default for Beat {
    /// A Ping each 20 s, a frame within 10 s.
    fn default() -> Beat {
        Beat { ping_every: Some(PING_EVERY), pong_within: PONG_WITHIN }
    }
}

impl Beat {
    /// Each time more than zero.
    pub fn check(&self) -> Result<(), String> {
        if self.ping_every == Some(Duration::ZERO) || self.pong_within.is_zero() {
            return Err("ping_every and pong_within are more than zero".into());
        }
        Ok(())
    }
}

/// Why the pump ended the stream of the link, when it was an error: the
/// first one is kept.
#[derive(Default)]
struct Fault(OnceLock<(io::ErrorKind, String)>);

impl Fault {
    fn set(&self, kind: io::ErrorKind, text: String) {
        let _ = self.0.set((kind, text));
    }

    fn error(&self) -> Option<io::Error> {
        self.0.get().map(|(kind, text)| io::Error::new(*kind, text.clone()))
    }
}

/// The byte stream of the link: the end of the pump's buffer, which gives
/// the fault of the pump in place of the end of the bytes.
struct Carried {
    near: DuplexStream,
    fault: Arc<Fault>,
}

impl AsyncRead for Carried {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let read = Pin::new(&mut self.near).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = read
            && buf.filled().len() == before
            && buf.remaining() > 0
            && let Some(e) = self.fault.error()
        {
            return Poll::Ready(Err(e));
        }
        read
    }
}

impl AsyncWrite for Carried {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let written = Pin::new(&mut self.near).poll_write(cx, buf);
        match written {
            Poll::Ready(Err(e)) => Poll::Ready(Err(self.fault.error().unwrap_or(e))),
            other => other,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.near).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.near).poll_shutdown(cx)
    }
}

/// The byte stream of a link over `session`, a WebSocket that the caller
/// opened: a task carries each read of the link in one binary message, and
/// each binary message in one write, under the rules of `beat` and of the
/// carrier (a text message ends the stream with an error).
pub fn carry<S>(session: RelaySession<S>, beat: Beat) -> BoxIo
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (near, far) = tokio::io::duplex(BUFFER);
    let fault = Arc::new(Fault::default());
    let set = fault.clone();
    tokio::spawn(async move { pump(session, far, beat, &set).await });
    Box::new(Carried { near, fault })
}

/// Copy the bytes both ways between `session` and `io` until one side
/// ends, the peer breaks a rule of the carrier, or the peer goes silent.
/// The fault is set before `io` drops, so the link reads the fault, not an
/// end.
async fn pump<S>(session: RelaySession<S>, io: DuplexStream, beat: Beat, fault: &Fault)
where
    S: AsyncRead + AsyncWrite + Send,
{
    let (mut io_r, mut io_w) = tokio::io::split(io);
    let session = &session;
    let ws_ended = Notify::new();
    let writing = AtomicBool::new(false);

    let ws_to_io = async {
        loop {
            let message = match session.read_frame().await {
                Ok(message) => message,
                Err(e) => {
                    if let pod_ws::SessionError::Protocol(_) | pod_ws::SessionError::TooLarge(_) = e {
                        fault.set(io::ErrorKind::InvalidData, format!("the ws carrier: {e}"));
                    }
                    tracing::debug!(error = %e, "the session of a ws carrier ended");
                    break;
                }
            };
            match message.opcode {
                frame::OPCODE_CLOSE => {
                    let (code, reason) = close_code_and_reason(&message.payload);
                    tracing::debug!(code, reason = %reason, "the peer of a ws carrier closed");
                    break;
                }
                frame::OPCODE_TEXT => {
                    fault.set(
                        io::ErrorKind::InvalidData,
                        "the peer of the ws carrier sent a text message: an error of protocol".into(),
                    );
                    let _ = session.send_close(code::UNSUPPORTED, "binary messages only").await;
                    break;
                }
                _ => {
                    writing.store(true, Ordering::SeqCst);
                    let written = io_w.write_all(&message.payload).await;
                    writing.store(false, Ordering::SeqCst);
                    if written.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = io_w.shutdown().await;
        ws_ended.notify_one();
    };

    let io_to_ws = async {
        let mut buf = vec![0u8; CHUNK];
        loop {
            let read = tokio::select! {
                read = io_r.read(&mut buf) => read,
                () = ws_ended.notified() => break,
            };
            match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if session.send_binary(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = session.send_close(code::NORMAL, "").await;
    };

    tokio::select! {
        _ = async { tokio::join!(ws_to_io, io_to_ws) } => {}
        (kind, text) = pulse(session, beat, &writing) => fault.set(kind, text),
    }
}

/// Ping the peer each `ping_every`, and return when no frame came within
/// `pong_within` after a Ping, while no message waits for the link. After
/// this end's Close, the wait bounds the answer of the peer: no Ping goes.
async fn pulse<S>(session: &RelaySession<S>, beat: Beat, writing: &AtomicBool) -> (io::ErrorKind, String)
where
    S: AsyncRead + AsyncWrite,
{
    let Some(every) = beat.ping_every else {
        return std::future::pending().await;
    };
    let mut next = Instant::now() + every;
    let mut sent: u64 = 0;
    loop {
        sleep_until(next).await;
        let heard = session.frames_received();
        if !session.close_sent() {
            sent += 1;
            if let Err(e) = session.send_ping(&sent.to_be_bytes()).await {
                return (io::ErrorKind::BrokenPipe, format!("the ws carrier: a Ping to the peer: {e}"));
            }
        }
        let wait = Instant::now() + beat.pong_within;
        sleep_until(wait).await;
        next = (next + every).max(wait);
        if session.frames_received() == heard && !writing.load(Ordering::SeqCst) {
            return (
                io::ErrorKind::TimedOut,
                format!("the ws carrier: no frame from the peer within {:?} of a Ping", beat.pong_within),
            );
        }
    }
}
