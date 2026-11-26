//! The runtime that Arti runs on: tokio and rustls as Arti's own, with a
//! TCP provider whose connections are the dials of the caller's way out
//! ([`pod_ws::Way`]): through the proxy that the caller chose, else
//! directly. Arti asks for a relay by its address and port alone, which a
//! proxy's CONNECT carries as they are, with no name to resolve. It never
//! listens: a client of Tor needs no listener.
//!
//! A bridge's transport (PodUtils/podssh@T-385) is a sentinel address that Arti dials as a
//! SOCKS5 proxy; the provider gives it a pipe in memory whose far end is the
//! transport in this process ([`crate::transport`]). Any other address is a
//! relay, dialled as above.

use std::io::{Error, ErrorKind, Result as IoResult};
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::io::{AsyncRead, AsyncWrite};
use pod_core::{BoxedIo, Target};
use pod_egress::pt::Reach;
use pod_tls::Trust;
use pod_ws::Way;
use sync_wrapper::SyncWrapper;
use tokio::io::DuplexStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt as _};
use tor_rtcompat::{NetStreamListener, NetStreamProvider, StreamOps, TcpConnectOptions, TcpListenOptions};

use crate::transport::{Kind, Said, TRANSPORT_WAIT};

/// How long one connection to a relay may take, the proxy's CONNECT included.
pub const CONNECT_WAIT: Duration = Duration::from_secs(20);

/// The TCP provider: each connection through the dial of the way out.
#[derive(Clone, Debug)]
pub struct ProxiedTcp {
    way: Way,
    wait: Duration,
    trust: Trust,
    said: Said,
}

impl ProxiedTcp {
    /// Connections through `way`, each within `wait`.
    pub fn new(way: Way, wait: Duration) -> Self {
        ProxiedTcp { way, wait, trust: Trust::Default, said: Said::default() }
    }

    /// The roots that a bridge's transport checks its TLS with.
    pub fn with_trust(mut self, trust: Trust) -> Self {
        self.trust = trust;
        self
    }

    /// The last failure of a bridge's transport, shared with each clone.
    pub fn said(&self) -> Said {
        self.said.clone()
    }
}

/// A connection to a relay, or to a bridge's transport, as Arti reads and
/// writes it.
pub struct ProxiedStream {
    inner: Inner,
}

enum Inner {
    // Arti asks for a stream that is `Sync`; the stream of a dial is `Send`
    // alone, and the wrapper gives it only through `&mut`.
    Tcp(SyncWrapper<Compat<BoxedIo>>),
    Pipe(Compat<DuplexStream>),
}

impl AsyncRead for ProxiedStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<IoResult<usize>> {
        match &mut self.inner {
            Inner::Tcp(s) => Pin::new(s.get_mut()).poll_read(cx, buf),
            Inner::Pipe(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ProxiedStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<IoResult<usize>> {
        match &mut self.inner {
            Inner::Tcp(s) => Pin::new(s.get_mut()).poll_write(cx, buf),
            Inner::Pipe(s) => Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<IoResult<()>> {
        match &mut self.inner {
            Inner::Tcp(s) => Pin::new(s.get_mut()).poll_flush(cx),
            Inner::Pipe(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<IoResult<()>> {
        match &mut self.inner {
            Inner::Tcp(s) => Pin::new(s.get_mut()).poll_close(cx),
            Inner::Pipe(s) => Pin::new(s).poll_close(cx),
        }
    }
}

// The socket options of a stream through a proxy are the proxy's, and a
// pipe has none, so none is offered: the trait's defaults refuse each one.
impl StreamOps for ProxiedStream {}

/// The listener that is never made.
pub struct NoListener;

impl NetStreamListener<SocketAddr> for NoListener {
    type Stream = ProxiedStream;
    type Incoming = futures::stream::Pending<IoResult<(ProxiedStream, SocketAddr)>>;
    fn incoming(self) -> Self::Incoming {
        futures::stream::pending()
    }
    fn local_addr(&self) -> IoResult<SocketAddr> {
        Err(refused())
    }
}

fn refused() -> Error {
    Error::new(ErrorKind::Unsupported, "the road through Tor listens on no socket")
}

#[async_trait::async_trait]
impl NetStreamProvider<SocketAddr> for ProxiedTcp {
    type Stream = ProxiedStream;
    type Listener = NoListener;
    type ConnectOptions = TcpConnectOptions;
    type ListenOptions = TcpListenOptions;

    async fn connect(&self, addr: &SocketAddr, _: &TcpConnectOptions) -> IoResult<ProxiedStream> {
        // A transport's sentinel is a key, never dialled.
        if let Some(kind) = Kind::by_addr(addr) {
            let reach = Reach { way: self.way.clone(), trust: self.trust.clone(), wait: TRANSPORT_WAIT };
            let pipe = crate::transport::spawn(kind, reach, self.said.clone());
            return Ok(ProxiedStream { inner: Inner::Pipe(pipe.compat()) });
        }
        let deadline = tokio::time::Instant::now() + self.wait;
        let connected = self
            .way
            .dial
            .dial(&Target::Addr(*addr), deadline)
            .await
            .map_err(|e| Error::new(ErrorKind::ConnectionRefused, e.chain()))?;
        Ok(ProxiedStream { inner: Inner::Tcp(SyncWrapper::new(connected.io.compat())) })
    }

    async fn listen(&self, _: &SocketAddr, _: &TcpListenOptions) -> IoResult<NoListener> {
        Err(refused())
    }
}
