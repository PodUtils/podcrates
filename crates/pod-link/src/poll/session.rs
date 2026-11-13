//! The sessions of a listener of the poll carriers: the table by id, the
//! offset of the bytes up, the bytes down that the dialer did not
//! acknowledge, and the end of a session with no request.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use pod_identity::PublicKey;
use tokio::io::{AsyncReadExt as _, DuplexStream};
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::pipes::{Ends, Pipes, State, pipes, until};

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The bytes up of a session: the far end of the pipe into the link, none
/// after the `fin` of the dialer, and the next offset.
pub(super) struct Up {
    pub pipe: Option<DuplexStream>,
    pub next: u64,
}

/// The bytes down that the dialer did not acknowledge.
struct Kept {
    /// The offset of the first byte of `buf`.
    base: u64,
    buf: VecDeque<u8>,
    /// The link ended its bytes down after `buf`.
    fin: bool,
    /// The highest offset sent: a byte below it goes again.
    sent: u64,
    /// The count of down requests: the last one is the live one.
    turn: u64,
}

/// The bytes down of a session.
pub(super) struct Down {
    kept: Mutex<Kept>,
    more: Notify,
    room: Notify,
    window: usize,
}

/// Some bytes down from an offset.
pub(super) struct Slice {
    pub bytes: Vec<u8>,
    /// The end of the stream follows these bytes.
    pub fin: bool,
    /// The bytes that went before.
    pub resent: u64,
}

/// Why a wait for bytes down ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Waited {
    Ready,
    Timeout,
    /// A later request of the down lane came.
    Superseded,
    /// The session ended.
    Ended,
}

impl Down {
    /// Drop the bytes before `to`: the dialer has them. An offset past
    /// the bytes kept is an error; one below them is stale (false): a
    /// request that a cut held back.
    pub fn ack(&self, to: u64) -> Result<bool, ()> {
        let mut kept = locked(&self.kept);
        let end = kept.base + kept.buf.len() as u64;
        if to > end {
            return Err(());
        }
        if to < kept.base {
            return Ok(false);
        }
        let n = (to - kept.base) as usize;
        kept.buf.drain(..n);
        kept.base = to;
        drop(kept);
        if n > 0 {
            self.room.notify_one();
        }
        Ok(true)
    }

    /// A new request of the down lane: the one before it ends.
    pub fn turn(&self) -> u64 {
        let mut kept = locked(&self.kept);
        kept.turn += 1;
        let turn = kept.turn;
        drop(kept);
        self.more.notify_waiters();
        turn
    }

    /// The bytes from `from` (not below the base), at most `max`.
    pub fn slice(&self, from: u64, max: usize) -> Slice {
        let mut kept = locked(&self.kept);
        let end = kept.base + kept.buf.len() as u64;
        let at = from.saturating_sub(kept.base).min(kept.buf.len() as u64) as usize;
        let n = (kept.buf.len() - at).min(max);
        let bytes: Vec<u8> = kept.buf.range(at..at + n).copied().collect();
        let to = from + n as u64;
        let resent = kept.sent.min(to).saturating_sub(from);
        kept.sent = kept.sent.max(to);
        Slice { bytes, fin: kept.fin && to == end, resent }
    }

    /// Wait until bytes from `from` or the end are here, a later request
    /// came, the session ended, or `deadline` passed.
    pub async fn wait(
        &self,
        from: u64,
        turn: u64,
        deadline: Option<Instant>,
        state: &mut watch::Receiver<State>,
    ) -> Waited {
        loop {
            let notified = self.more.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let kept = locked(&self.kept);
                if kept.turn != turn {
                    return Waited::Superseded;
                }
                if kept.fin || kept.base + kept.buf.len() as u64 > from {
                    return Waited::Ready;
                }
            }
            let at = deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(86400));
            tokio::select! {
                () = notified => {}
                () = tokio::time::sleep_until(at), if deadline.is_some() => return Waited::Timeout,
                () = until(state, &[State::Failed]) => return Waited::Ended,
            }
        }
    }
}

/// Read the bytes of the link into the bytes kept, while they are fewer
/// than the window.
async fn pump(down: Arc<Down>, mut from: DuplexStream) {
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let room = loop {
            let notified = down.room.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let room = down.window.saturating_sub(locked(&down.kept).buf.len());
            if room > 0 {
                break room;
            }
            notified.await;
        };
        let max = room.min(buf.len());
        let n = from.read(&mut buf[..max]).await.unwrap_or(0);
        let mut kept = locked(&down.kept);
        if n == 0 {
            kept.fin = true;
        } else {
            kept.buf.extend(&buf[..n]);
        }
        drop(kept);
        down.more.notify_waiters();
        if n == 0 {
            return;
        }
    }
}

/// One session of a listener.
pub(super) struct Session {
    pub key: PublicKey,
    pub ends: Arc<Ends>,
    pub up: tokio::sync::Mutex<Up>,
    pub down: Arc<Down>,
    pump: JoinHandle<()>,
    seen: Mutex<Instant>,
    busy: AtomicUsize,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.pump.abort();
    }
}

/// A request of a session in progress: the session does not end for
/// silence while it lasts.
pub(super) struct Visit(Arc<Session>);

impl Visit {
    pub fn session(&self) -> &Session {
        &self.0
    }
}

impl Drop for Visit {
    fn drop(&mut self) {
        *locked(&self.0.seen) = Instant::now();
        self.0.busy.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The result of an open.
pub(super) enum Opened {
    /// A new session, and the byte stream of its link.
    New(Pipes),
    /// The session of this id and key exists: an open again after a lost
    /// answer.
    Again,
    /// The listener holds its most sessions.
    Busy,
    /// The id is the one of another key.
    Stranger,
}

/// The sessions of a listener by id.
pub(super) struct Table {
    sessions: Mutex<HashMap<u128, Arc<Session>>>,
    max: usize,
    window: usize,
    pipe: usize,
}

impl Table {
    pub fn new(max: usize, window: usize, pipe: usize) -> Table {
        Table { sessions: Mutex::new(HashMap::new()), max, window, pipe }
    }

    pub fn open(&self, id: u128, key: PublicKey) -> Opened {
        let mut sessions = locked(&self.sessions);
        if let Some(session) = sessions.get(&id) {
            return if session.key == key { Opened::Again } else { Opened::Stranger };
        }
        if sessions.len() >= self.max {
            return Opened::Busy;
        }
        let ends = Ends::new();
        let (io, far) = pipes(self.pipe, ends.clone());
        let kept = Kept { base: 0, buf: VecDeque::new(), fin: false, sent: 0, turn: 0 };
        let down =
            Arc::new(Down { kept: Mutex::new(kept), more: Notify::new(), room: Notify::new(), window: self.window });
        let session = Session {
            key,
            ends,
            up: tokio::sync::Mutex::new(Up { pipe: Some(far.from_peer), next: 0 }),
            down: down.clone(),
            pump: tokio::spawn(pump(down, far.to_peer)),
            seen: Mutex::new(Instant::now()),
            busy: AtomicUsize::new(0),
        };
        sessions.insert(id, Arc::new(session));
        Opened::New(io)
    }

    /// A request of the session `id` from `key`: none for an id that is not
    /// here or that another key opened.
    pub fn visit(&self, id: u128, key: &PublicKey) -> Option<Visit> {
        let session = locked(&self.sessions).get(&id).filter(|s| s.key == *key).cloned()?;
        session.busy.fetch_add(1, Ordering::SeqCst);
        Some(Visit(session))
    }

    /// End the session `id` with an error of `kind`.
    pub fn end(&self, id: u128, kind: io::ErrorKind, text: impl Into<String>) {
        if let Some(session) = locked(&self.sessions).remove(&id) {
            session.ends.fail(kind, text);
        }
    }

    /// End each session with no request for `idle`.
    pub fn expire(&self, idle: Duration) {
        locked(&self.sessions).retain(|_, s| {
            let dead = s.busy.load(Ordering::SeqCst) == 0 && locked(&s.seen).elapsed() >= idle;
            if dead {
                s.ends.fail(io::ErrorKind::TimedOut, format!("the dialer sent no request for {idle:?}"));
            }
            !dead
        });
    }
}

impl Drop for Table {
    fn drop(&mut self) {
        for (_, s) in locked(&self.sessions).drain() {
            s.ends.fail(io::ErrorKind::BrokenPipe, "the listener of the session closed");
        }
    }
}
