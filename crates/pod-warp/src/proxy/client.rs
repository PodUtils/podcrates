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
                let mut upstream = match dial(target).await {
                    Ok(upstream) => upstream,
                    Err(e) => {
                        let status = if e.class == Class::Policy { 403 } else { 502 };
                        let _ = stream.write_all(&refusal(status, &e.chain(), &[])).await;
                        return Err(e);
                    }
                };
                upstream.write_all(&head).await.map_err(|e| io_failure("the request to the origin", e))?;
                upstream.write_all(&opened.rest).await.map_err(|e| io_failure("the request to the origin", e))?;
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
                return Ok(());
            }
        }
    };
    tracing::debug!(%target, "WARP proxy request");
    let mut upstream = match dial(target).await {
        Ok(upstream) => upstream,
        Err(e) => {
            // RFC 1928: 0x02, not allowed by the ruleset.
            if is_socks {
                let _ = serve::socks5_reply(&mut stream, serve::reply_for(&e), None).await;
            } else {
                let status = if e.class == Class::Policy { 403 } else { 502 };
                let _ = stream.write_all(&refusal(status, &e.chain(), &[])).await;
            }
            return Err(e);
        }
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
