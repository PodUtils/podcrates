//! One client of the proxy: its first byte chooses SOCKS5 or HTTP; the
//! request, the credentials, the dial, the answer, then the bytes both
//! ways until both ends close. The protocol is `pod_proxy::serve`.

use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use pod_core::{BoxedIo, Class, Dial, Failure, Report, Stage, Target};
use pod_proxy::Credentials;
use pod_proxy::http::request::{established, refusal};
use pod_proxy::serve::{self, HttpProxyRequest};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, ReadBuf};

use super::names::Names;
use super::udp::{self, UdpClient};

/// The settings of the opening of one client.
#[derive(Clone, Copy)]
pub struct Opening<'a> {
    /// The user and the password that the client must give.
    pub credentials: Option<&'a Credentials>,
    /// The time limit of the request.
    pub limit: Duration,
    /// The decision `warp.dispatch` of each client.
    pub report: &'a Report,
    /// The local and the peer address of a TCP client.
    pub control: Option<(IpAddr, IpAddr)>,
}

/// The UDP of a client: the session, and the names of its datagrams.
#[derive(Clone)]
pub struct Udp<'a> {
    pub net: Arc<dyn Dial>,
    pub names: &'a Names,
}

fn failure(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Proxy, class, text)
}

fn io_failure(what: &str, e: std::io::Error) -> Failure {
    Failure::caused(Stage::Proxy, Class::of_io(e.kind()), what.to_string(), e)
}

/// Read one SOCKS5 or HTTP request of `stream`, connect with `dial`, answer
/// the client, and copy the bytes both ways until both ends close. With
/// `udp`, the SOCKS5 commands `UDP ASSOCIATE` and 0x83 are served too; with
/// none, they get the reply 7. A dial that fails with the class `Policy`
/// (a rule) gets the SOCKS5 reply 2 or the HTTP status 403.
pub async fn serve_client<S, D, F>(
    stream: S,
    dial: D,
    udp: Option<Udp<'_>>,
    opening: &Opening<'_>,
) -> Result<(), Failure>
where
    S: AsyncRead + AsyncWrite + Unpin,
    D: FnOnce(Target) -> F,
    F: std::future::Future<Output = Result<BoxedIo, Failure>>,
{
    serve(stream, dial, udp, None, opening).await
}

/// Answer a client of a proxy that has no session (PodUtils/podnet#712):
/// read its request, send the SOCKS5 reply 1 or the HTTP status 503 with
/// `cause`, then close with no reset. A client that gives no SOCKS5 or
/// HTTP request gets the answer of [`serve_client`]. The error is `cause`.
pub async fn refuse_client<S>(stream: S, cause: Failure, opening: &Opening<'_>) -> Result<(), Failure>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let dial = |_: Target| std::future::ready(Err(failure(Class::Internal, "no dial with no session")));
    serve(stream, dial, None, Some(cause), opening).await
}

/// The way of [`serve_client`]; with `no_session`, no dial is made and
/// each request gets the failure answer of that cause.
async fn serve<S, D, F>(
    stream: S,
    dial: D,
    udp: Option<Udp<'_>>,
    mut no_session: Option<Failure>,
    opening: &Opening<'_>,
) -> Result<(), Failure>
where
    S: AsyncRead + AsyncWrite + Unpin,
    D: FnOnce(Target) -> F,
    F: std::future::Future<Output = Result<BoxedIo, Failure>>,
{
    let (limit, report) = (opening.limit, opening.report);
    let mut stream = stream;
    let mut first = [0u8; 1];
    tokio::time::timeout(limit, stream.read_exact(&mut first))
        .await
        .map_err(|_| failure(Class::Timeout, format!("no request in {} s", limit.as_secs())))?
        .map_err(|e| io_failure("the first byte of a client", e))?;
    let is_socks = serve::looks_like_socks5(first[0]);
    let mut stream = Prefixed { first: Some(first[0]), inner: stream };
    if !is_socks && first[0] == 0x16 {
        // A TLS ClientHello: the client takes the proxy for https://.
        report.decision("warp.dispatch", "tls_client");
        return Err(failure(
            Class::Protocol,
            "a TLS client on the proxy (use http:// or socks5h:// for the proxy URL)",
        ));
    }
    // PodUtils/podnet issue 178: a SOCKS4 client gets its refusal at once;
    // another byte that cannot start an HTTP method closes at once.
    if !is_socks && first[0] == 0x04 {
        report.decision("warp.dispatch", "socks4");
        let _ = stream.write_all(&[0x00, 0x5b, 0, 0, 0, 0, 0, 0]).await;
        return Err(failure(Class::Protocol, "a SOCKS4 client on the proxy (use socks5h:// or http://)"));
    }
    if !is_socks && !first[0].is_ascii_alphabetic() {
        report.decision("warp.dispatch", "unknown");
        return Err(failure(
            Class::Protocol,
            format!("the first byte 0x{:02x} starts no SOCKS5 and no HTTP request", first[0]),
        ));
    }
    let (target, rest) = if is_socks {
        let read = tokio::time::timeout(limit, serve::read_socks5(&mut stream, opening.credentials));
        let (command, target) = read
            .await
            .map_err(|_| failure(Class::Timeout, format!("no SOCKS5 request in {} s", limit.as_secs())))?
            .map_err(|e| io_failure("the SOCKS5 request", e))?;
        let outcome = match command {
            serve::CMD_CONNECT => "socks_connect",
            serve::CMD_UDP_ASSOCIATE => "udp_associate",
            serve::CMD_UDP_IN_STREAM => "udp_in_stream",
            _ => "socks_unsupported",
        };
        report.decision("warp.dispatch", outcome);
        match (command, udp) {
            (serve::CMD_CONNECT, _) => (target, Vec::new()),
            (serve::CMD_UDP_ASSOCIATE, Some(udp)) => {
                let mut client = UdpClient::of(&target);
                if let Some((_, peer)) = opening.control
                    && !peer.is_loopback()
                {
                    // A remote client: only its own address.
                    client.ip.get_or_insert(peer);
                }
                let bind = udp::relay_bind_ip(opening.control.map(|(local, _)| local));
                return udp::associate(stream, udp, client, bind).await;
            }
            (serve::CMD_UDP_IN_STREAM, Some(udp)) => return udp::in_stream(stream, udp).await,
            (serve::CMD_UDP_ASSOCIATE | serve::CMD_UDP_IN_STREAM, None) if let Some(cause) = no_session => {
                return Err(answer_failure(&mut stream, true, cause, true).await);
            }
            (other, _) => {
                let _ = serve::socks5_reply(&mut stream, serve::REPLY_COMMAND_NOT_SUPPORTED, None).await;
                return Err(failure(Class::Unsupported, format!("the SOCKS5 command {other} is not served")));
            }
        }
    } else {
        let read = tokio::time::timeout(limit, serve::read_http_proxy(&mut stream));
        let opened = match read.await {
            Err(_) => return Err(failure(Class::Timeout, format!("no HTTP request in {} s", limit.as_secs()))),
            Ok(Err(e)) => {
                if let Some(status) = e.status() {
                    let _ = stream.write_all(&refusal(status, &e.to_string(), &[])).await;
                }
                return Err(failure(Class::Protocol, e.to_string()));
            }
            Ok(Ok(opened)) => opened,
        };
        if let Some(credentials) = opening.credentials
            && !credentials.check_basic(opened.authorization.as_deref())
        {
            let challenge = [("Proxy-Authenticate", "Basic realm=\"warp\"")];
            stream.write_all(&refusal(407, "", &challenge)).await.map_err(|e| io_failure("the answer 407", e))?;
            return Err(failure(Class::Refused, "an HTTP client with no or wrong credentials"));
        }
        match opened.request {
            HttpProxyRequest::Connect(target) => {
                report.decision("warp.dispatch", "http_connect");
                (target, opened.rest)
            }
            HttpProxyRequest::Forward { target, head } => {
                // An http:// URL (HTTP_PROXY): the request in origin form
                // to the origin, then the bytes both ways.
                report.decision("warp.dispatch", "http_forward");
                tracing::debug!(%target, "WARP proxy forward request");
                let gone = no_session.is_some();
                let dialed = match no_session.take() {
                    Some(cause) => Err(cause),
                    None => dial(target).await,
                };
                let mut upstream = match dialed {
                    Ok(upstream) => upstream,
                    Err(e) => return Err(answer_failure(&mut stream, false, e, gone).await),
                };
                upstream.write_all(&head).await.map_err(|e| io_failure("the request to the origin", e))?;
                upstream.write_all(&opened.rest).await.map_err(|e| io_failure("the request to the origin", e))?;
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
                return Ok(());
            }
        }
    };
    tracing::debug!(%target, "WARP proxy request");
    let gone = no_session.is_some();
    let dialed = match no_session.take() {
        Some(cause) => Err(cause),
        None => dial(target).await,
    };
    let mut upstream = match dialed {
        Ok(upstream) => upstream,
        Err(e) => return Err(answer_failure(&mut stream, is_socks, e, gone).await),
    };
    if is_socks {
        serve::socks5_reply(&mut stream, serve::REPLY_SUCCEEDED, None).await
    } else {
        stream.write_all(&established()).await
    }
    .map_err(|e| io_failure("the answer to the client", e))?;
    upstream.write_all(&rest).await.map_err(|e| io_failure("the first bytes of the client", e))?;
    let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
    Ok(())
}

/// The time that a refused client has to close its end before the drop.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// Send the answer of the failure `e` and give `e` back. RFC 1928: the
/// reply 2 for a rule (class `Policy`). With `gone` (no session), the
/// SOCKS5 reply 1 or the HTTP status 503, then [`close_clean`].
async fn answer_failure<S>(stream: &mut S, is_socks: bool, e: Failure, gone: bool) -> Failure
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if is_socks {
        let code = if gone { serve::REPLY_GENERAL_FAILURE } else { serve::reply_for(&e) };
        let _ = serve::socks5_reply(stream, code, None).await;
    } else {
        let (status, words) = match (gone, e.class) {
            (true, _) => (503, format!("no WARP session: {}", e.chain())),
            (false, Class::Policy) => (403, e.chain()),
            (false, _) => (502, e.chain()),
        };
        let _ = stream.write_all(&refusal(status, &words, &[])).await;
    }
    if gone {
        close_clean(stream).await;
    }
    e
}

/// Close `stream` with no reset: shut the write half, then read and drop
/// the bytes of the far end until it closes, for [`CLOSE_WAIT`] at most. A
/// drop with unread bytes makes the kernel send a reset, and the far end
/// can then lose the answer.
pub(crate) async fn close_clean<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) {
    let _ = stream.shutdown().await;
    let mut sink = [0u8; 512];
    let drain = async { while matches!(stream.read(&mut sink).await, Ok(n) if n > 0) {} };
    let _ = tokio::time::timeout(CLOSE_WAIT, drain).await;
}

/// A stream with one byte put back in front of it.
pub(crate) struct Prefixed<S> {
    first: Option<u8>,
    inner: S,
}

impl<S: AsyncRead + Unpin> AsyncRead for Prefixed<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        if let Some(byte) = self.first.take() {
            buf.put_slice(&[byte]);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Prefixed<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, data)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
