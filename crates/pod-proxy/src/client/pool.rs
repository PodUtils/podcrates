//! The idle connections of a [`super::Client`] (keep-alive), shared by its
//! clones. A connection goes back to the pool only after an answer of
//! `HTTP/1.1` whose body was read to its end by its length or its chunks,
//! and whose server did not say `Connection: close`. A connection serves
//! only a request with the same key: the scheme, the host, the port, the way
//! out (the same `Arc<dyn Dial>`) and the trust.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use pod_core::{BoxedIo, Dial};
use pod_tls::Trust;
use tokio::io::{AsyncRead, ReadBuf};
use tokio_rustls::rustls::ClientConfig;

use super::url::Target;

/// The longest time that a connection stays idle in the pool. Servers
/// close an idle connection after 5 s to 120 s; a stale one costs a retry.
pub const IDLE: Duration = Duration::from_secs(30);

/// The most idle connections for one key.
pub const PER_KEY: usize = 4;

/// The most idle connections of a client.
pub const TOTAL: usize = 16;

/// What a pooled connection is for.
#[derive(Clone)]
pub(super) struct Key {
    tls: bool,
    /// In lower case.
    host: String,
    port: u16,
    /// The way out. The key holds it, so that its address is not given to
    /// another dial while the connection waits.
    dial: Arc<dyn Dial>,
    /// `None` for `http://`.
    trust: Option<TrustKey>,
}

/// The trust of a TLS connection. A caller's configuration is compared by
/// its `Arc`: two configurations with the same roots are two keys.
#[derive(Clone)]
enum TrustKey {
    Default,
    File(PathBuf),
    Caller(Arc<ClientConfig>),
}

impl TrustKey {
    fn of(trust: &Trust) -> TrustKey {
        match trust {
            Trust::Default => TrustKey::Default,
            Trust::File(path) => TrustKey::File(path.clone()),
            Trust::Caller(caller) => TrustKey::Caller(caller.config().clone()),
        }
    }

    fn same(&self, other: &TrustKey) -> bool {
        match (self, other) {
            (TrustKey::Default, TrustKey::Default) => true,
            (TrustKey::File(a), TrustKey::File(b)) => a == b,
            (TrustKey::Caller(a), TrustKey::Caller(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Key {
    pub(super) fn new(target: &Target, dial: &Arc<dyn Dial>, trust: Option<&Trust>) -> Key {
        Key {
            tls: target.tls,
            host: target.host.to_ascii_lowercase(),
            port: target.port,
            dial: dial.clone(),
            trust: trust.map(TrustKey::of),
        }
    }

    fn same(&self, other: &Key) -> bool {
        let trust = match (&self.trust, &other.trust) {
            (None, None) => true,
            (Some(a), Some(b)) => a.same(b),
            _ => false,
        };
        self.tls == other.tls
            && self.port == other.port
            && self.host == other.host
            && Arc::ptr_eq(&self.dial, &other.dial)
            && trust
    }
}

struct Parked {
    key: Key,
    io: BoxedIo,
    since: Instant,
}

/// The idle connections.
#[derive(Default)]
pub(super) struct Pool {
    idle: Mutex<Vec<Parked>>,
}

impl Pool {
    /// The most recent idle connection for `key` that is still open.
    pub(super) fn take(&self, key: &Key) -> Option<BoxedIo> {
        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        idle.retain(|parked| parked.since.elapsed() < IDLE);
        while let Some(at) = idle.iter().rposition(|parked| parked.key.same(key)) {
            let mut parked = idle.remove(at);
            if open(&mut parked.io) {
                return Some(parked.io);
            }
        }
        None
    }

    /// Keep `io` for a next request with `key`. The oldest connection goes
    /// when the key or the pool is full.
    pub(super) fn put(&self, key: Key, io: BoxedIo) {
        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        idle.retain(|parked| parked.since.elapsed() < IDLE);
        if idle.iter().filter(|parked| parked.key.same(&key)).count() >= PER_KEY
            && let Some(oldest) = idle.iter().position(|parked| parked.key.same(&key))
        {
            idle.remove(oldest);
        }
        if idle.len() >= TOTAL {
            idle.remove(0);
        }
        idle.push(Parked { key, io, since: Instant::now() });
    }
}

/// An idle connection is open when a read has nothing to give: the end of
/// the stream, an error or a byte that no request asked for make it unusable.
fn open(io: &mut BoxedIo) -> bool {
    let mut cx = Context::from_waker(Waker::noop());
    let mut byte = [0u8; 1];
    let mut buf = ReadBuf::new(&mut byte);
    matches!(Pin::new(io).poll_read(&mut cx, &mut buf), Poll::Pending)
}
