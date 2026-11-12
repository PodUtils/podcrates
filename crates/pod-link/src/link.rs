//! A link: the handle of the caller, and the task that drives the carrier
//! (the reader, the writer and the keepalive).

use std::future::pending;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until, timeout};

use crate::error::{Error, GoAwayCode};
use crate::frame::{Decoder, Frame};
use crate::settings::Settings;
use crate::state::{Batch, Role, Shared, State};
use crate::stream::Stream;

/// How long the writer tries to give its last frame (a `GOAWAY`) to a
/// carrier that does not take it.
const LAST_WRITE: Duration = Duration::from_secs(1);

/// How long a link that sent its `GOAWAY` waits for the peer to end the
/// carrier.
const LINGER: Duration = Duration::from_secs(2);

/// The bytes of one read from the carrier.
const READ: usize = 64 << 10;

/// Many streams over one carrier byte stream. [`Link::new`] starts a task
/// on the current tokio runtime that reads and writes the carrier; the
/// handle opens and accepts streams. A drop of the handle ends the link
/// at once: its streams then fail. [`Link::close`] ends it with a `GOAWAY`
/// after the frames that wait.
pub struct Link {
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl Link {
    /// Start a link over `carrier` as `role`. The settings must pass
    /// [`Settings::check`]. It must run in a tokio runtime with the time
    /// driver on.
    pub fn new<C>(carrier: C, role: Role, settings: Settings) -> Result<Link, Error>
    where
        C: AsyncRead + AsyncWrite + Send + 'static,
    {
        settings.check()?;
        let shared = Arc::new(Shared::new(State::new(role, settings)));
        let task = tokio::spawn(drive(shared.clone(), carrier));
        Ok(Link { shared, task })
    }

    /// Open a stream. It does not wait for the peer: the bytes of the
    /// stream can go at once. A peer at its limit of streams resets it with
    /// [`crate::ResetCode::REFUSED`], and the stream's reads and writes then
    /// fail.
    pub fn open(&self) -> Result<Stream, Error> {
        let id = self.shared.with(|s| s.open())?;
        Ok(Stream::new(id, self.shared.clone()))
    }

    /// The next stream that the peer opened. An error when the link ended.
    pub async fn accept(&self) -> Result<Stream, Error> {
        loop {
            let mut notice = pin!(self.shared.accepts.notified());
            notice.as_mut().enable();
            let next = self.shared.with(|s| match s.accept.pop_front() {
                Some(id) => Ok(Some(id)),
                None => s.ended.clone().map_or(Ok(None), Err),
            })?;
            if let Some(id) = next {
                return Ok(Stream::new(id, self.shared.clone()));
            }
            notice.await;
        }
    }

    /// End the link: the frames that wait go first, then a `GOAWAY`. It
    /// returns when the carrier took the `GOAWAY`, or when the link ended
    /// for another reason. New streams fail at once.
    pub async fn close(&self) {
        self.shared.with(|s| {
            if s.ended.is_none() && !s.closing {
                s.closing = true;
                s.push_out(Frame::GoAway(GoAwayCode::NORMAL.0));
            }
        });
        self.closed().await;
    }

    /// Wait for the end of the link, and give its reason.
    pub async fn closed(&self) -> Error {
        loop {
            let mut notice = pin!(self.shared.ended.notified());
            notice.as_mut().enable();
            if let Some(e) = self.shared.ended() {
                return e;
            }
            notice.await;
        }
    }

    /// Why the link ended, or `None` while it runs.
    pub fn ended(&self) -> Option<Error> {
        self.shared.ended()
    }

    /// The streams of the link: open, or waiting to be accepted.
    pub fn streams(&self) -> usize {
        self.shared.with(|s| s.streams.len())
    }

    /// The time of the last `PING` and its `PONG`, when one came back.
    pub fn rtt(&self) -> Option<Duration> {
        self.shared.with(|s| s.rtt)
    }

    /// The role of this end.
    pub fn role(&self) -> Role {
        self.shared.with(|s| s.role)
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.shared.fail(Error::Closed { by_peer: false });
        self.task.abort();
    }
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").field("role", &self.role()).field("streams", &self.streams()).finish()
    }
}

/// The task of a link. It ends when the writer stops: after the end of the
/// link, and after its last frame.
async fn drive<C>(shared: Arc<Shared>, carrier: C)
where
    C: AsyncRead + AsyncWrite + Send + 'static,
{
    let (reader, writer) = tokio::io::split(carrier);
    let read = async {
        read_loop(&shared, reader).await;
        pending::<()>().await
    };
    let keep = async {
        keepalive(&shared).await;
        pending::<()>().await
    };
    tokio::select! {
        () = write_loop(&shared, writer) => {}
        () = read => {}
        () = keep => {}
    }
}

async fn read_loop<C: AsyncRead>(shared: &Shared, mut reader: ReadHalf<C>) {
    let mut decoder = Decoder::with_preface();
    let mut buf = vec![0; READ];
    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) => return shared.with(|s| s.fail(s.after_goaway().unwrap_or(Error::CarrierEnded))),
            Ok(n) => n,
            Err(e) => return shared.with(|s| s.fail(s.after_goaway().unwrap_or_else(|| Error::io(&e)))),
        };
        decoder.push(&buf[..n]);
        let done = shared.with(|s| {
            s.heard();
            loop {
                match decoder.next() {
                    Ok(Some(frame)) => {
                        if let Err(e) = s.on_frame(frame) {
                            s.fail(e);
                        }
                    }
                    Ok(None) => break,
                    Err(e) => s.fail(e.into()),
                }
                if s.ended.is_some() {
                    break;
                }
            }
            s.ended.is_some()
        });
        if done {
            return;
        }
    }
}

async fn write_loop<C: AsyncWrite>(shared: &Shared, mut writer: WriteHalf<C>) {
    let mut out = Vec::with_capacity(READ);
    loop {
        out.clear();
        let mut wake = pin!(shared.writer.notified());
        wake.as_mut().enable();
        match shared.with(|s| s.take_batch(&mut out)) {
            Batch::Idle => wake.await,
            Batch::Write => {
                let mut end = pin!(shared.ended.notified());
                end.as_mut().enable();
                if shared.ended().is_some() {
                    continue;
                }
                let written = tokio::select! {
                    r = async {
                        writer.write_all(&out).await?;
                        writer.flush().await
                    } => r,
                    () = end => continue,
                };
                if let Err(e) = written {
                    shared.fail(Error::io(&e));
                }
            }
            Batch::Stop => {
                let clean = shared.ended().is_none();
                let _ = timeout(LAST_WRITE, async {
                    writer.write_all(&out).await?;
                    writer.shutdown().await
                })
                .await;
                if clean {
                    // The peer reads the GOAWAY, then ends its side: wait for
                    // that end, as a close of the carrier with bytes unread
                    // can reset it before the peer reads the GOAWAY.
                    let mut end = pin!(shared.ended.notified());
                    end.as_mut().enable();
                    shared.with(|s| s.goaway_sent = true);
                    let _ = timeout(LINGER, end).await;
                    shared.fail(Error::Closed { by_peer: false });
                }
                return;
            }
        }
    }
}

/// Send a `PING` after each `keepalive` of silence; end the link after
/// `dead_after` of silence.
async fn keepalive(shared: &Shared) {
    loop {
        let mut end = pin!(shared.ended.notified());
        end.as_mut().enable();
        let next = shared.with(|s| {
            if s.ended.is_some() {
                return None;
            }
            let now = Instant::now();
            let dead = s.last_heard + s.settings.dead_after;
            if now >= dead {
                s.fail(Error::Dead(s.settings.dead_after));
                return None;
            }
            let ping_at = s.last_heard.max(s.last_ping) + s.settings.keepalive;
            if now >= ping_at {
                let value = s.ping_seq;
                s.ping_seq += 1;
                s.ping = Some((value, now));
                s.last_ping = now;
                s.push_urgent(Frame::Ping(value));
                return Some(now);
            }
            Some(dead.min(ping_at))
        });
        let Some(at) = next else { return };
        tokio::select! {
            () = sleep_until(at) => {}
            () = end => return,
        }
    }
}
