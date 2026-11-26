//! The stream of a dial: TCP, TLS to an `https://` proxy over TCP, or a
//! stream through a proxy that is on a Unix socket or at the stdin and the
//! stdout of a command ([`ChildIo`]); and the opening of the stream to a
//! proxy by its [`Carrier`] and its kind.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use pod_core::Stage;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

pub use crate::child::ChildIo;
use crate::error::DialError;
use crate::host::check_name;
use crate::proxy::{Carrier, Proxy, ProxyKind};
use crate::race::{Leg, connect_direct};

/// The ALPN of the TLS to an `https://` proxy: its CONNECT is HTTP/1.1.
const HTTP1: &[&[u8]] = &[b"http/1.1"];

/// The stream of a dial.
pub enum Stream {
    Tcp(TcpStream),
    /// Through an `https://` proxy: the TLS to the proxy, over TCP, and the
    /// tunnel inside it (boxed: a TLS session holds far more than a socket).
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
    /// Through a proxy on a Unix socket.
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    /// Through a proxy at a command (boxed: a child holds far more than a
    /// socket).
    Exec(Box<ChildIo>),
}

impl std::fmt::Debug for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stream::Tcp(tcp) => f.debug_tuple("Tcp").field(&tcp.peer_addr().ok()).finish(),
            Stream::Tls(tls) => f.debug_tuple("Tls").field(&tls.get_ref().0.peer_addr().ok()).finish(),
            #[cfg(unix)]
            Stream::Unix(_) => f.write_str("Unix"),
            Stream::Exec(_) => f.write_str("Exec"),
        }
    }
}

impl Stream {
    /// The TCP connection, when the stream is plain TCP. The TCP under the
    /// TLS of an `https://` proxy is not given: a write on it would break
    /// the TLS.
    pub fn tcp(&self) -> Option<&TcpStream> {
        match self {
            Stream::Tcp(tcp) => Some(tcp),
            _ => None,
        }
    }

    /// The TCP connection, or the stream back when it is not one.
    pub fn into_tcp(self) -> Result<TcpStream, Stream> {
        match self {
            Stream::Tcp(tcp) => Ok(tcp),
            other => Err(other),
        }
    }

    /// The address of the far end of a TCP connection (a proxy's, through a
    /// proxy; the TCP under the TLS of an `https://` proxy too).
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.socket().and_then(|tcp| tcp.peer_addr().ok())
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.socket().and_then(|tcp| tcp.local_addr().ok())
    }

    /// The TCP socket of the stream, plain or under TLS: for its options
    /// and its addresses, never for its bytes.
    pub(crate) fn socket(&self) -> Option<&TcpStream> {
        match self {
            Stream::Tcp(tcp) => Some(tcp),
            Stream::Tls(tls) => Some(tls.get_ref().0),
            _ => None,
        }
    }
}

macro_rules! each {
    ($self:ident, $s:ident => $e:expr) => {
        match $self.get_mut() {
            Stream::Tcp($s) => $e,
            Stream::Tls($s) => {
                let $s = &mut **$s;
                $e
            }
            #[cfg(unix)]
            Stream::Unix($s) => $e,
            Stream::Exec($s) => {
                let $s = &mut **$s;
                $e
            }
        }
    };
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        each!(self, s => Pin::new(s).poll_read(cx, buf))
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        each!(self, s => Pin::new(s).poll_write(cx, data))
    }

    // rustls writes its records with one vectored write; the default sends
    // only the first buffer of each call, and an upload over TLS stalls.
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        each!(self, s => Pin::new(s).poll_write_vectored(cx, bufs))
    }

    fn is_write_vectored(&self) -> bool {
        match self {
            Stream::Tcp(s) => s.is_write_vectored(),
            Stream::Tls(s) => s.is_write_vectored(),
            #[cfg(unix)]
            Stream::Unix(s) => s.is_write_vectored(),
            Stream::Exec(s) => s.is_write_vectored(),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        each!(self, s => Pin::new(s).poll_flush(cx))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        each!(self, s => Pin::new(s).poll_shutdown(cx))
    }
}

/// The stream to `proxy` by its carrier, within the deadline of `leg`. The
/// host of a proxy on TCP is resolved with the resolver of the proxy's own
/// name ([`Leg::proxy_resolve`]): a resolver that asks DNS over HTTPS through
/// this same proxy would otherwise call itself without end. An `https://`
/// proxy gets TLS over the TCP, with the TLS of the leg, before a byte of
/// its exchange.
pub(crate) async fn open_proxy(leg: Leg<'_>, proxy: &Proxy) -> Result<Stream, DialError> {
    let stream = open_carrier(leg, proxy).await?;
    if proxy.kind() != ProxyKind::Https {
        return Ok(stream);
    }
    let Some(tls) = leg.proxy_tls else {
        return Err(DialError::BadProxy(format!(
            "{} is reached over TLS, and this dial has no TLS context for a proxy",
            proxy.label()
        )));
    };
    let Stream::Tcp(tcp) = stream else {
        return Err(DialError::BadProxy(format!("{} is reached over TLS, on TCP alone", proxy.label())));
    };
    let left = leg.deadline.saturating_duration_since(tokio::time::Instant::now());
    let tls = crate::tls_hop::handshake(tcp, &proxy.to_string(), &proxy.host, tls, HTTP1, left).await;
    // A time out names the whole bound of the dial, as each other step does.
    let tls = tls.map_err(|e| match e {
        DialError::Timeout { step, .. } => DialError::Timeout { step, after: leg.budget, stage: Stage::Proxy },
        other => other,
    })?;
    Ok(Stream::Tls(Box::new(tls)))
}

/// The stream to `proxy` by its carrier alone.
async fn open_carrier(leg: Leg<'_>, proxy: &Proxy) -> Result<Stream, DialError> {
    let unreachable = |detail: String| DialError::ProxyUnreachable { proxy: proxy.to_string(), detail };
    match proxy.carrier() {
        Carrier::Tcp => {
            check_name(&proxy.host)?;
            let own = Leg { resolve: leg.proxy_resolve, ..leg };
            connect_direct(own, &proxy.host, proxy.port).await.map(Stream::Tcp).map_err(|e| match e {
                DialError::Timeout { .. } => DialError::Timeout {
                    step: format!("connecting to the proxy {proxy}"),
                    after: leg.budget,
                    stage: Stage::Proxy,
                },
                DialError::Resolve { host, detail } => {
                    unreachable(format!("the proxy host {host} does not resolve: {detail}"))
                }
                // The proxy is named once, by `ProxyUnreachable`.
                DialError::Connect { target, detail, .. } => unreachable(
                    pod_core::failure::strip_lead(&detail, &format!("could not connect to {target}")).into(),
                ),
                other => unreachable(other.to_string()),
            })
        }
        #[cfg(unix)]
        Carrier::Unix(path) => match tokio::time::timeout_at(leg.deadline, tokio::net::UnixStream::connect(path)).await
        {
            Ok(Ok(stream)) => Ok(Stream::Unix(stream)),
            Ok(Err(e)) => Err(unreachable(e.to_string())),
            Err(_) => Err(DialError::Timeout {
                step: format!("connecting to the proxy {proxy}"),
                after: leg.budget,
                stage: Stage::Proxy,
            }),
        },
        #[cfg(not(unix))]
        Carrier::Unix(_) => Err(unreachable("a Unix socket needs a Unix host".into())),
        Carrier::Exec(command) => match crate::child::held(command) {
            // A command that the shell cannot run or does not find, again.
            Some(failure) => Err(failure),
            None => ChildIo::spawn(command)
                .map(|child| Stream::Exec(Box::new(child)))
                .map_err(|e| unreachable(format!("starting the proxy command (sh -c): {e}"))),
        },
    }
}
