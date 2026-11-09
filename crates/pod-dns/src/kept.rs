//! The connection that a resolver keeps for each server and way out: after
//! an answer, the connection waits for the next question and closes after a
//! short idle time. A question takes the kept connection, or opens one; one
//! reconnect follows a kept connection that failed, as the server may have
//! closed it.

use std::sync::Mutex;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use pod_core::BoxedIo;
use tokio::io::{AsyncRead, ReadBuf};
use tokio_rustls::client::TlsStream;

use crate::h2::H2;

/// How long a connection waits for the next question. Resolvers close an
/// idle connection after some seconds.
pub const IDLE: Duration = Duration::from_secs(5);

/// A connection to a resolver, after its handshake.
pub(crate) enum Conn {
    /// HTTP/1.1 over TLS.
    One(Box<TlsStream<BoxedIo>>),
    /// HTTP/2.
    Two(H2),
}

struct Slot {
    host: String,
    port: u16,
    since: Instant,
    conn: Conn,
}

/// The kept connections of one way out: at most one for each host and
/// port. A [`crate::Resolver`] owns one; a caller that makes a
/// [`crate::Way`] of its own gives one, for the way out that it gives.
#[derive(Default)]
pub struct Kept {
    slots: Mutex<Vec<Slot>>,
}

impl std::fmt::Debug for Kept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kept").field("connections", &self.len()).finish()
    }
}

impl Kept {
    pub fn new() -> Kept {
        Kept::default()
    }

    /// The connections that wait now.
    pub fn len(&self) -> usize {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Close each kept connection.
    pub fn clear(&self) {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// The connection for `host:port` that still waits, if any: one that is
    /// idle too long, or that the server closed, is dropped.
    pub(crate) fn take(&self, host: &str, port: u16) -> Option<Conn> {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots.retain(|slot| slot.since.elapsed() < IDLE);
        let at = slots.iter().position(|slot| slot.port == port && slot.host == host)?;
        let mut slot = slots.remove(at);
        open(&mut slot.conn).then_some(slot.conn)
    }

    /// Keep `conn` for the next question to `host:port`; an older
    /// connection of the same key closes.
    pub(crate) fn put(&self, host: &str, port: u16, conn: Conn) {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots.retain(|slot| slot.since.elapsed() < IDLE && !(slot.port == port && slot.host == host));
        slots.push(Slot { host: host.to_string(), port, since: Instant::now(), conn });
    }
}

/// An idle HTTP/1.1 connection is open when a read has nothing to give: the
/// end of the stream, an error or a byte that no question asked for make it
/// unusable. An HTTP/2 connection fails at its next question instead.
fn open(conn: &mut Conn) -> bool {
    let Conn::One(tls) = conn else { return true };
    let mut cx = Context::from_waker(Waker::noop());
    let mut byte = [0u8; 1];
    let mut buf = ReadBuf::new(&mut byte);
    matches!(std::pin::Pin::new(tls).poll_read(&mut cx, &mut buf), Poll::Pending)
}
