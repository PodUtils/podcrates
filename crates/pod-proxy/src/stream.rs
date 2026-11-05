//! The stream of a dial: TCP, or a stream through a proxy that is on a Unix
//! socket or at the stdin and the stdout of a command; and the opening of
//! the stream to a proxy by its [`Carrier`].

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use pod_core::Stage;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use crate::error::DialError;
use crate::host::check_name;
use crate::proxy::{Carrier, Proxy};
use crate::race::{Leg, connect_direct};

/// The stream of a dial.
pub enum Stream {
    Tcp(TcpStream),
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
            #[cfg(unix)]
            Stream::Unix(_) => f.write_str("Unix"),
            Stream::Exec(_) => f.write_str("Exec"),
        }
    }
}

impl Stream {
    /// The TCP connection, when the stream is one.
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
    /// proxy).
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.tcp().and_then(|tcp| tcp.peer_addr().ok())
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.tcp().and_then(|tcp| tcp.local_addr().ok())
    }
}

macro_rules! each {
    ($self:ident, $s:ident => $e:expr) => {
        match $self.get_mut() {
            Stream::Tcp($s) => $e,
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

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        each!(self, s => Pin::new(s).poll_flush(cx))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        each!(self, s => Pin::new(s).poll_shutdown(cx))
    }
}

/// The stdin and the stdout of a command as one byte stream. The command
/// runs with `sh -c`; its stderr is the stderr of the process, as the
/// ProxyCommand of SSH has it. The end of the stream closes its stdin; a drop
/// kills it.
pub struct ChildIo {
    child: tokio::process::Child,
    stdin: Option<tokio::process::ChildStdin>,
    stdout: tokio::process::ChildStdout,
}

impl ChildIo {
    pub fn spawn(command: &str) -> io::Result<ChildIo> {
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or_else(|| io::Error::other("the command has no stdout"))?;
        Ok(ChildIo { child, stdin, stdout })
    }

    /// The exit status, when the command has ended.
    pub fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }
}

impl AsyncRead for ChildIo {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdout).poll_read(cx, buf)
    }
}

impl AsyncWrite for ChildIo {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        match self.stdin.as_mut() {
            Some(stdin) => Pin::new(stdin).poll_write(cx, data),
            None => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.stdin.as_mut() {
            Some(stdin) => Pin::new(stdin).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stdin = None;
        Poll::Ready(Ok(()))
    }
}

/// The stream to `proxy` by its carrier, within the deadline of `leg`. The
/// host of a proxy on TCP is resolved with the resolver of the proxy's own
/// name ([`Leg::proxy_resolve`]): a resolver that asks DNS over HTTPS through
/// this same proxy would otherwise call itself without end.
pub(crate) async fn open_proxy(leg: Leg<'_>, proxy: &Proxy) -> Result<Stream, DialError> {
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
        Carrier::Exec(command) => ChildIo::spawn(command)
            .map(|child| Stream::Exec(Box::new(child)))
            .map_err(|e| unreachable(format!("starting the proxy command (sh -c): {e}"))),
    }
}
