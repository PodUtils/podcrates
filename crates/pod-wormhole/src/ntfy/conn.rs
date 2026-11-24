//! One HTTP connection to an ntfy server, through the caller's way out: TLS
//! offers `h2` then `http/1.1` (`http://` on the loopback of a test is
//! HTTP/1.1 with no TLS). On HTTP/2 the stream of a subscription and the
//! POSTs share the connection; on HTTP/1.1 the requests take turns on it,
//! each whole, and a connection that the server closed between two
//! requests is dialed again once.

use std::time::Duration;

use bytes::Bytes;
use pod_core::BoxedIo;
use tokio::task::JoinHandle;

use super::Point;
use super::wire::refusal;
use crate::config::Config;
use crate::{Failed, Missed};

/// The largest body of an answer that a request reads whole: a poll gives
/// the messages of one code, a few dozen at most.
pub const BODY_MOST: usize = 1 << 20;
/// How long the end of an HTTP/2 connection may take.
const CLOSE: Duration = Duration::from_secs(2);
const H2: &[u8] = b"h2";
const HTTP1: &[u8] = b"http/1.1";

/// The answer to a request, read whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The words of an answer that is no success.
    pub fn refusal(&self) -> String {
        refusal(self.status, &self.body)
    }
}

/// A connection, of the protocol that TLS chose.
pub enum Conn {
    /// HTTP/1.1: the connection between two requests, if it is still open.
    Http1(Option<BoxedIo>),
    Http2(Http2),
}

/// An HTTP/2 connection, driven in a task.
pub struct Http2 {
    /// None once the connection ended.
    send: Option<h2::client::SendRequest<Bytes>>,
    driver: Option<JoinHandle<()>>,
}

impl Drop for Http2 {
    fn drop(&mut self) {
        if let Some(driver) = self.driver.take() {
            driver.abort();
        }
    }
}

/// The body of a stream of HTTP/2, as its parts come.
pub struct Body(h2::RecvStream);

impl Body {
    /// The next part; `None` at the end of the body.
    pub async fn part(&mut self) -> Option<Result<Bytes, String>> {
        let part = self.0.data().await?;
        let flow = self.0.flow_control();
        Some(part.inspect(|bytes| drop(flow.release_capacity(bytes.len()))).map_err(|e| format!("HTTP/2: {e}")))
    }
}

/// Dial `point` through the way of `config`; with `h2`, TLS offers HTTP/2
/// first.
pub async fn dial(point: &Point, config: &Config, h2: bool) -> Result<Conn, Failed> {
    let unreachable = |why: String| Failed::Unreachable(format!("{}: {why}", point.shown()).into());
    // The failure of the dial stays as the source: its stage tells a proxy
    // or a name from the far end.
    let missed = |e: &dyn std::fmt::Display| format!("{}: {e}", point.shown());
    if !point.tls {
        let tcp = config.way.tcp(&point.host, point.port, config.connect_timeout).await;
        return Ok(Conn::Http1(Some(tcp.map_err(|e| Failed::Unreachable(Missed::caused(missed(&e), e)))?.io)));
    }
    let alpn: &[&[u8]] = if h2 { &[H2, HTTP1] } else { &[HTTP1] };
    let opened = config.way.open_tls(&point.host, point.port, &point.host, &config.trust, config.connect_timeout, alpn);
    let (tls, _) = opened.await.map_err(|e| Failed::Unreachable(Missed::caused(missed(&e), e)))?;
    if tls.get_ref().1.alpn_protocol() != Some(H2) {
        return Ok(Conn::Http1(Some(Box::new(tls))));
    }
    let shaken = tokio::time::timeout(config.connect_timeout, h2::client::handshake(tls)).await;
    let (send, connection) = match shaken {
        Ok(Ok(made)) => made,
        Ok(Err(e)) => return Err(unreachable(format!("HTTP/2's handshake: {e}"))),
        Err(_) => return Err(unreachable("HTTP/2's handshake did not end in time".into())),
    };
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(Conn::Http2(Http2 { send: Some(send), driver: Some(driver) }))
}

impl Conn {
    /// Whether the connection is of HTTP/2.
    pub fn is_h2(&self) -> bool {
        matches!(self, Conn::Http2(_))
    }

    /// One request of `method` for `target`, with `body`, and its answer
    /// read whole, within `limit`.
    pub async fn request(
        &mut self,
        point: &Point,
        config: &Config,
        method: &str,
        target: &str,
        body: &[u8],
    ) -> Result<Answer, Failed> {
        let limit = config.answer_limit;
        let late = || Failed::Broken(format!("the ntfy server {} did not answer within {limit:?}", point.shown()));
        match self {
            Conn::Http1(kept) => {
                // A kept connection that the server closed fails at once:
                // the request goes again on a new one.
                for _ in 0..2 {
                    let reused = kept.is_some();
                    let stream = match kept.take() {
                        Some(stream) => stream,
                        None => match dial(point, config, false).await? {
                            Conn::Http1(Some(stream)) => stream,
                            _ => return Err(Failed::Broken(format!("{}: no HTTP/1.1", point.shown()))),
                        },
                    };
                    let asked = exchange(stream, point, config, method, target, body);
                    match tokio::time::timeout(limit, asked).await {
                        Ok(Ok((answer, stream))) => {
                            *kept = stream;
                            return Ok(answer);
                        }
                        Ok(Err(_)) if reused => {}
                        Ok(Err(e)) => return Err(Failed::Broken(format!("the ntfy server {}: {e}", point.shown()))),
                        Err(_) => return Err(late()),
                    }
                }
                Err(Failed::Broken(format!("the ntfy server {} closed each connection", point.shown())))
            }
            Conn::Http2(conn) => {
                let asked = async {
                    let mut body_in = conn.open(point, config, method, target, body).await?;
                    let mut out = Vec::new();
                    while let Some(part) = body_in.1.part().await {
                        let part =
                            part.map_err(|e| Failed::Broken(format!("the ntfy server {}: {e}", point.shown())))?;
                        if out.len() + part.len() > BODY_MOST {
                            return Err(Failed::Broken(format!(
                                "the ntfy server {} sent an answer over {BODY_MOST} bytes",
                                point.shown()
                            )));
                        }
                        out.extend_from_slice(&part);
                    }
                    Ok(Answer { status: body_in.0, body: out })
                };
                tokio::time::timeout(limit, asked).await.map_err(|_| late())?
            }
        }
    }

    /// The stream of the answer to a GET of `target` on HTTP/2: its status
    /// and its body as it comes, within the answer limit for the head.
    pub async fn stream(&mut self, point: &Point, config: &Config, target: &str) -> Result<(u16, Body), Failed> {
        let Conn::Http2(conn) = self else {
            return Err(Failed::Broken("a stream of a subscription needs HTTP/2".into()));
        };
        let limit = config.answer_limit;
        let late = || Failed::Broken(format!("the ntfy server {} did not answer within {limit:?}", point.shown()));
        tokio::time::timeout(limit, conn.open(point, config, "GET", target, b"")).await.map_err(|_| late())?
    }

    /// End the connection: GOAWAY on HTTP/2, a close of TLS on HTTP/1.1.
    pub async fn end(&mut self) {
        match self {
            Conn::Http1(kept) => {
                if let Some(mut stream) = kept.take() {
                    let _ = tokio::time::timeout(CLOSE, tokio::io::AsyncWriteExt::shutdown(&mut stream)).await;
                }
            }
            Conn::Http2(conn) => {
                // With the last handle of the connection gone, h2 sends
                // GOAWAY and ends; a server that takes no more bytes is cut.
                conn.send = None;
                if let Some(mut driver) = conn.driver.take()
                    && tokio::time::timeout(CLOSE, &mut driver).await.is_err()
                {
                    driver.abort();
                }
            }
        }
    }
}

impl Http2 {
    /// A request on a new stream, and the head of its answer.
    async fn open(
        &mut self,
        point: &Point,
        config: &Config,
        method: &str,
        target: &str,
        body: &[u8],
    ) -> Result<(u16, Body), Failed> {
        let broken = |e: h2::Error| Failed::Broken(format!("the ntfy server {}: HTTP/2: {e}", point.shown()));
        let Some(send) = self.send.clone() else {
            return Err(Failed::Broken(format!("the connection to the ntfy server {} ended", point.shown())));
        };
        let mut send = send.ready().await.map_err(broken)?;
        let mut request = ::http::Request::builder()
            .method(method)
            .uri(format!("https://{}{target}", point.host_header()))
            .header("user-agent", config.way.user_agent.as_str());
        for (name, value) in headers(method) {
            request = request.header(*name, *value);
        }
        let request = request.body(()).map_err(|e| Failed::Broken(format!("{}: {e}", point.shown())))?;
        let (response, mut stream) = send.send_request(request, body.is_empty()).map_err(broken)?;
        if !body.is_empty() {
            stream.send_data(Bytes::copy_from_slice(body), true).map_err(broken)?;
        }
        let response = response.await.map_err(broken)?;
        Ok((response.status().as_u16(), Body(response.into_body())))
    }
}

/// The headers of a request beside its host and its user agent: a POST asks
/// ntfy.sh to keep the message from Firebase, which would carry it to the
/// phones of the topic's subscribers.
fn headers(method: &str) -> &'static [(&'static str, &'static str)] {
    if method == "POST" {
        &[("x-firebase", "no"), ("content-type", "text/plain")]
    } else {
        &[("accept", "application/x-ndjson")]
    }
}

/// One exchange of HTTP/1.1 on `stream`, and the stream when it can carry
/// the next request.
async fn exchange(
    stream: BoxedIo,
    point: &Point,
    config: &Config,
    method: &str,
    target: &str,
    body: &[u8],
) -> std::io::Result<(Answer, Option<BoxedIo>)> {
    let host = point.host_header();
    let ua = config.way.user_agent.as_str();
    let (response, kept) =
        pod_proxy::http::exchange_keep(stream, ua, method, &host, target, headers(method), body, BODY_MOST).await?;
    Ok((Answer { status: response.status, body: response.body }, kept))
}
