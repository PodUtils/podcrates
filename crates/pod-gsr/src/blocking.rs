//! The blocking face (feature `blocking`), for a consumer with no tokio
//! runtime. It is the same code as the async face: each call runs the async
//! form on a private current-thread runtime, so that a change of the
//! handshake or of the stream comes to both faces at once (call C20).
//!
//! A call from a task of a tokio runtime fails with `Class::Usage`, and a
//! read or a write there fails with `io::ErrorKind::Unsupported`: the runtime
//! of this face cannot run inside another one.

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pod_core::entropy::Entropy;
use pod_core::{Class, Dial, Failure, Stage};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::runtime::Runtime;

use crate::{End, Role, Settings};

/// A runtime that shuts down in the background when the last value that
/// holds it drops: a dial that its deadline ended can leave a system name
/// lookup on a thread of the runtime, and a plain drop waits for it.
#[derive(Debug)]
struct Background(Option<Runtime>);

impl std::ops::Deref for Background {
    type Target = Runtime;

    fn deref(&self) -> &Runtime {
        self.0.as_ref().expect("the runtime lives until the drop")
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

/// True inside a task of a tokio runtime, where `block_on` panics.
fn in_runtime() -> bool {
    tokio::runtime::Handle::try_current().is_ok()
}

/// `pod_gsr::open`, blocking: the same arguments, and the deadline
/// of the dial and of the wait for the peer as a `std::time::Instant`. The
/// dial runs on the private runtime of the stream, so `dial` must be a dialer
/// that any tokio runtime can drive (`pod_proxy::blocking::Dialer::as_async`
/// gives one).
pub fn open(
    dial: &dyn Dial,
    settings: &Settings,
    role: Role,
    secret: &str,
    entropy: &mut (dyn Entropy + Send),
    deadline: Instant,
) -> Result<Stream, Failure> {
    if in_runtime() {
        return Err(Failure::new(
            Stage::Relay,
            Class::Usage,
            "pod_gsr::blocking::open runs on its own runtime: call it outside a tokio task, or call pod_gsr::open",
        ));
    }
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| Failure::io(Stage::Relay, e))?;
    let runtime = Arc::new(Background(Some(runtime)));
    let deadline = tokio::time::Instant::from_std(deadline);
    let inner = runtime.block_on(crate::session::open(dial, settings, role, secret, entropy, deadline))?;
    Ok(Stream { inner, runtime, limit: None })
}

/// The stream of the async face, blocking: `Read` and `Write` on the
/// private runtime of [`open`]. The stream drops before its runtime.
pub struct Stream {
    inner: crate::stream::Stream,
    runtime: Arc<Background>,
    /// The limit of each read and each write ([`Stream::set_timeout`]).
    limit: Option<Duration>,
}

impl std::fmt::Debug for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("blocking::Stream").field(&self.inner).finish()
    }
}

impl Stream {
    /// True once the peer's end of stream has been read.
    pub fn peer_finished(&self) -> bool {
        self.inner.peer_finished()
    }

    /// How the peer's side ended: none until a read gives the end of the
    /// stream ([`crate::End`]), as on the async face.
    pub fn end(&self) -> Option<End> {
        self.inner.end()
    }

    /// The limit of each read and each write, none for no limit. At the
    /// limit the call fails with `io::ErrorKind::TimedOut`. A read can be
    /// called again with no byte lost; a write that timed out must be called
    /// again with the same bytes, as OpenSSL asks of a write that it did not
    /// finish.
    pub fn set_timeout(&mut self, limit: Option<Duration>) {
        self.limit = limit;
    }

    /// End this side's writes: the close_notify goes out, and the
    /// transport closes once the peer's end was read (the async face's
    /// `poll_shutdown`).
    pub fn shutdown(&mut self) -> io::Result<()> {
        let (runtime, limit) = (Arc::clone(&self.runtime), self.limit);
        bounded(&runtime, limit, self.inner.shutdown())
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let (runtime, limit) = (Arc::clone(&self.runtime), self.limit);
        bounded(&runtime, limit, self.inner.read(buf))
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (runtime, limit) = (Arc::clone(&self.runtime), self.limit);
        bounded(&runtime, limit, self.inner.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        let (runtime, limit) = (Arc::clone(&self.runtime), self.limit);
        bounded(&runtime, limit, self.inner.flush())
    }
}

/// `io` on `runtime`, within `limit`. A read and a write of the TLS stream
/// keep their state in OpenSSL, so a call that the limit ends loses no byte
/// that was read.
fn bounded<T>(
    runtime: &Background,
    limit: Option<Duration>,
    io: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    if in_runtime() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "a pod_gsr::blocking::Stream runs on its own runtime: use it outside a tokio task",
        ));
    }
    let Some(limit) = limit else { return runtime.block_on(io) };
    // The timer is made on the runtime: it needs its clock.
    runtime
        .block_on(async { tokio::time::timeout(limit, io).await })
        .unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "the peer sent nothing within the limit")))
}
