//! The blocking face of the dial and of the client (feature `blocking`), for
//! a synchronous consumer. It is the same code as the async face: each call
//! runs the async form on a private current-thread runtime, so that each
//! proxy kind and each scheme of authentication comes to both faces at once.
//! A TCP connection comes back as the standard `TcpStream`, and I/O on it
//! needs no runtime.
//!
//! Do not call it from a task of a tokio runtime: `Runtime::block_on`
//! panics there.

use std::io::{self, IoSlice, Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Failure, Resolve};
use pod_tls::{Context, Trust};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::runtime::Runtime;

use crate::client::Request;
use crate::http::Response;
use crate::{ChildIo, DialError, Multipath, Proxy, Settings};

/// The private runtime of a blocking value. One thread, the caller's: the
/// I/O driver and the timers serve the dial, and the process driver serves a
/// proxy at a command.
fn runtime() -> io::Result<Arc<Background>> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    Ok(Arc::new(Background(Some(runtime))))
}

/// A runtime that shuts down in the background when the last value that
/// holds it drops: a dial that its time limit ended can leave a system name
/// lookup on a thread of the runtime, and a plain drop waits for it.
#[derive(Debug)]
pub(crate) struct Background(Option<Runtime>);

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

/// [`crate::Dialer`], blocking.
#[derive(Clone)]
pub struct Dialer {
    inner: Arc<crate::Dialer>,
    runtime: Arc<Background>,
}

impl std::fmt::Debug for Dialer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("blocking::Dialer").field(&self.inner).finish()
    }
}

impl Dialer {
    /// A dialer with `settings`, which asks `resolve` for the addresses of a
    /// direct connection ([`crate::Dialer::new`]).
    pub fn new(settings: Settings, resolve: Arc<dyn Resolve>) -> io::Result<Dialer> {
        Dialer::from_async(crate::Dialer::new(settings, resolve))
    }

    /// The blocking face of `inner`, with the resolver of the proxy and the
    /// observer that it has.
    pub fn from_async(inner: crate::Dialer) -> io::Result<Dialer> {
        Ok(Dialer { inner: Arc::new(inner), runtime: runtime()? })
    }

    /// The async dialer under this one.
    pub fn as_async(&self) -> &crate::Dialer {
        &self.inner
    }

    /// [`crate::Dialer::proxy_at`].
    pub fn proxy_at(&self, host: &str, port: u16) -> Result<Option<Proxy>, String> {
        self.inner.proxy_at(host, port)
    }

    /// [`crate::Dialer::proxy_for_scheme`].
    pub fn proxy_for_scheme(
        &self,
        scheme: crate::Scheme,
        host: &str,
        port: Option<u16>,
    ) -> Result<Option<Proxy>, String> {
        self.inner.proxy_for_scheme(scheme, host, port)
    }

    /// A TCP connection to `host:port` ([`crate::Dialer::tcp`]), in blocking
    /// mode, with no read or write limit set.
    pub fn tcp(&self, host: &str, port: u16, timeout: Duration) -> Result<std::net::TcpStream, DialError> {
        let tcp = self.runtime.block_on(self.inner.tcp(host, port, timeout))?;
        let _in_runtime = self.runtime.enter();
        to_std_tcp(tcp).map_err(|e| lost(host, port, e))
    }

    /// The stream to `host:port` of any kind ([`crate::Dialer::stream`]),
    /// and how Multipath TCP went.
    pub fn stream(&self, host: &str, port: u16, timeout: Duration) -> Result<(Stream, Multipath), DialError> {
        let (stream, how) = self.runtime.block_on(self.inner.stream(host, port, timeout))?;
        let _in_runtime = self.runtime.enter();
        let stream = match stream {
            crate::Stream::Tcp(tcp) => Stream::Tcp(to_std_tcp(tcp).map_err(|e| lost(host, port, e))?),
            #[cfg(unix)]
            crate::Stream::Unix(unix) => {
                let unix = unix.into_std().map_err(|e| lost(host, port, e))?;
                unix.set_nonblocking(false).map_err(|e| lost(host, port, e))?;
                Stream::Unix(unix)
            }
            crate::Stream::Exec(child) => {
                Stream::Exec(Exec { child, runtime: self.runtime.clone(), limit: Default::default() })
            }
        };
        Ok((stream, how))
    }
}

fn to_std_tcp(tcp: tokio::net::TcpStream) -> io::Result<std::net::TcpStream> {
    let tcp = tcp.into_std()?;
    tcp.set_nonblocking(false)?;
    Ok(tcp)
}

/// A connection that was made, and that the move out of the runtime lost.
fn lost(host: &str, port: u16, e: io::Error) -> DialError {
    DialError::Connect {
        target: crate::authority(host, port),
        detail: format!("the connection could not leave the runtime: {e}"),
        cut: None,
        kind: Some(e.kind()),
    }
}

/// The stream of a blocking dial.
#[derive(Debug)]
pub enum Stream {
    Tcp(std::net::TcpStream),
    /// Through a proxy on a Unix socket.
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
    /// Through a proxy at a command.
    Exec(Exec),
}

impl Stream {
    /// The TCP connection, when the stream is one.
    pub fn tcp(&self) -> Option<&std::net::TcpStream> {
        match self {
            Stream::Tcp(tcp) => Some(tcp),
            _ => None,
        }
    }

    /// The address of the far end of a TCP connection (a proxy's, through a
    /// proxy).
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.tcp().and_then(|tcp| tcp.peer_addr().ok())
    }

    /// The limit of each read and each write. At the limit the call fails
    /// with the kind that a socket gives (`WouldBlock` on Unix, `TimedOut`
    /// elsewhere), on each kind of stream.
    pub fn set_timeouts(&self, limit: Option<Duration>) -> io::Result<()> {
        match self {
            Stream::Tcp(tcp) => {
                tcp.set_read_timeout(limit)?;
                tcp.set_write_timeout(limit)
            }
            #[cfg(unix)]
            Stream::Unix(unix) => {
                unix.set_read_timeout(limit)?;
                unix.set_write_timeout(limit)
            }
            Stream::Exec(exec) => {
                *exec.limit.lock().unwrap_or_else(|e| e.into_inner()) = limit;
                Ok(())
            }
        }
    }
}

/// The stream through a proxy at a command, which the runtime of its dialer
/// drives.
pub struct Exec {
    child: Box<ChildIo>,
    runtime: Arc<Background>,
    /// The limit of each read and each write ([`Stream::set_timeouts`]).
    limit: std::sync::Mutex<Option<Duration>>,
}

impl std::fmt::Debug for Exec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Exec")
    }
}

impl Exec {
    /// The exit status, when the command has ended.
    pub fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.child.exited()
    }
}

impl Read for Exec {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let child = &mut self.child;
        let limit = *self.limit.lock().unwrap_or_else(|e| e.into_inner());
        bounded_on(&self.runtime, limit, child.read(buf))
    }
}

impl Write for Exec {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let limit = *self.limit.lock().unwrap_or_else(|e| e.into_inner());
        bounded_on(&self.runtime, limit, self.child.write(buf))
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        let limit = *self.limit.lock().unwrap_or_else(|e| e.into_inner());
        bounded_on(&self.runtime, limit, self.child.write_vectored(bufs))
    }

    fn flush(&mut self) -> io::Result<()> {
        let limit = *self.limit.lock().unwrap_or_else(|e| e.into_inner());
        bounded_on(&self.runtime, limit, self.child.flush())
    }
}

/// `io` on `runtime`, within `limit`. The reads and the writes of a pipe give
/// up nothing when they are cancelled, so a call that the limit ends loses no
/// byte. At the limit, the kind of a socket's limit, so that a caller's check
/// of it holds for a command too.
fn bounded_on<T>(
    runtime: &Background,
    limit: Option<Duration>,
    io: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    let Some(limit) = limit else { return runtime.block_on(io) };
    // The timer is made on the runtime: it needs its clock.
    runtime.block_on(async { tokio::time::timeout(limit, io).await }).unwrap_or_else(|_| {
        let kind = if cfg!(unix) { io::ErrorKind::WouldBlock } else { io::ErrorKind::TimedOut };
        Err(io::Error::new(kind, "the proxy command sent nothing within the limit"))
    })
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(s) => s.read(buf),
            #[cfg(unix)]
            Stream::Unix(s) => s.read(buf),
            Stream::Exec(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(s) => s.write(buf),
            #[cfg(unix)]
            Stream::Unix(s) => s.write(buf),
            Stream::Exec(s) => s.write(buf),
        }
    }

    // rustls writes each flight of records with one vectored write.
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        match self {
            Stream::Tcp(s) => s.write_vectored(bufs),
            #[cfg(unix)]
            Stream::Unix(s) => s.write_vectored(bufs),
            Stream::Exec(s) => s.write_vectored(bufs),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Tcp(s) => s.flush(),
            #[cfg(unix)]
            Stream::Unix(s) => s.flush(),
            Stream::Exec(s) => s.flush(),
        }
    }
}

/// [`crate::Client`], blocking: its requests go through a blocking
/// [`Dialer`], on that dialer's runtime.
#[derive(Clone, Debug)]
pub struct Client {
    inner: crate::Client,
    runtime: Arc<Background>,
}

impl Client {
    /// A client over `dialer`, with the TLS of `context` and no user agent.
    pub fn new(context: Arc<Context>, dialer: &Dialer) -> Client {
        Client { inner: crate::Client::new(context, dialer.inner.clone()), runtime: dialer.runtime.clone() }
    }

    /// [`crate::Client::with_user_agent`].
    pub fn with_user_agent(self, user_agent: impl Into<String>) -> Client {
        Client { inner: self.inner.with_user_agent(user_agent), runtime: self.runtime }
    }

    /// [`crate::Client::https_get`].
    pub fn https_get(
        &self,
        host: &str,
        port: u16,
        path: &str,
        max_body: usize,
        trust: &Trust,
        timeout: Duration,
    ) -> Result<Response, Failure> {
        self.runtime.block_on(self.inner.https_get(host, port, path, max_body, trust, timeout))
    }

    /// [`crate::Client::send`].
    pub fn send(&self, request: Request) -> Result<Response, Failure> {
        self.runtime.block_on(self.inner.send(request))
    }

    /// [`crate::Client::send_streamed`]: the head of the answer, and its
    /// body as a reader on the runtime of the client.
    pub fn send_streamed(&self, request: Request) -> Result<Streamed, Failure> {
        let inner = self.runtime.block_on(self.inner.send_streamed(request))?;
        Ok(Streamed { inner, runtime: self.runtime.clone() })
    }
}

/// [`crate::client::Streamed`], blocking: each read of the body runs on the
/// runtime of its client.
#[derive(Debug)]
pub struct Streamed {
    inner: crate::client::Streamed,
    runtime: Arc<Background>,
}

impl Streamed {
    pub fn status(&self) -> u16 {
        self.inner.status
    }

    pub fn reason(&self) -> &str {
        &self.inner.reason
    }

    /// Header names lower-cased, values trimmed, in arrival order.
    pub fn headers(&self) -> &[(String, String)] {
        &self.inner.headers
    }

    /// The first value of a header, by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.inner.header(name)
    }

    /// A status of 2xx.
    pub fn is_success(&self) -> bool {
        self.inner.is_success()
    }

    /// The URL that gave this answer: after redirects, the last one.
    pub fn url(&self) -> &str {
        &self.inner.url
    }
}

impl Read for Streamed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.runtime.block_on(self.inner.read(buf))
    }
}
