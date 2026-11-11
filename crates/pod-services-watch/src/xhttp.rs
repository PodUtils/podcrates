//! VLESS over XHTTP ("SplitHTTP" of Xray) for `proxy-banner`: the client of
//! the mode `packet-up`. A streamed GET of `PATH/SESSION` carries the
//! downloads; a POST of `PATH/SESSION/SEQ` carries each upload, with the
//! sequence numbers from 0. The first upload holds the VLESS request. Each
//! request carries the padding of Xray's client: a `Referer` of the URL of
//! the path with the query `x_padding=` and 100 to 1000 `X`. The session id
//! is a random UUID.
//!
//! The form is the one of XTLS/Xray-core at commit
//! c43ee98ec7c5a9d34b32cedf891b4c8707e9c8b0, `transport/internet/splithttp/`:
//! `dialer.go` (`Dial`, `decideHTTPVersion`), `client.go` (`OpenStream`,
//! `PostPacket`), `config.go` (the path, the placements of the session and
//! the sequence number, the session id), `xpadding.go` (the padding) and
//! `hub.go` (what the server checks).
//!
//! * The mode `auto` of a client with TLS or with no security is
//!   `packet-up`. REALITY makes it `stream-one`; the watch speaks no
//!   REALITY. The modes `stream-up` and `stream-one`, HTTP/3 and the `extra`
//!   settings of a link are not spoken: the watch skips the link.
//! * TLS offers the `alpn` of the link, else `h2` and `http/1.1` (the
//!   default of Xray's TLS). The HTTP version is the protocol that the
//!   server chose by ALPN. With no choice, it is the one of Xray's client:
//!   HTTP/1.1 with no TLS or with the ALPN `http/1.1` alone, else HTTP/2.
//! * HTTP/1.1: the GET and the POST each on a connection of their own;
//!   HTTP/2: both as streams of one connection.

use std::pin::Pin;
use std::task::{Context, Poll, ready};

use bytes::Bytes;
use pod_core::BoxedIo;
use pod_core::entropy::{Entropy, OsEntropy};
use pod_proxy::http::{Body, Framing, MAX_HEAD, exchange, read_head};
use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf};

use crate::proxy::Link;
use crate::{Finding, Net};

/// The ALPN of the connection of an upload on HTTP/1.1.
const HTTP1: &[&[u8]] = &[b"http/1.1"];

/// The downloads of a session, as a reader.
pub type Down = Box<dyn AsyncRead + Unpin + Send>;

/// The HTTP of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    H1,
    H2,
}

impl Version {
    pub fn name(self) -> &'static str {
        match self {
            Version::H1 => "HTTP/1.1",
            Version::H2 => "HTTP/2",
        }
    }
}

/// What a link asks of XHTTP, read before a connection opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The path of each request, with a `/` at each end.
    pub path: String,
    /// The query of the path of the link, with no `?`; empty for none.
    pub query: String,
    /// The `Host` of each request, the `:authority` on HTTP/2.
    pub host: String,
    /// The server name of TLS; none with no security.
    pub sni: Option<String>,
    /// The protocols that TLS offers by ALPN.
    pub alpn: Vec<String>,
    /// The version of Xray's client, for a server that chose no protocol.
    pub version: Version,
}

impl Plan {
    /// The plan of `link` with `security` (`tls` or `none`); else why the
    /// watch does not speak it.
    pub fn of(link: &Link, security: &str) -> Result<Plan, String> {
        let mode = link.param("mode").unwrap_or_default();
        if !matches!(mode, "" | "auto" | "packet-up") {
            return Err(format!("the watch speaks no XHTTP mode {mode}"));
        }
        if link.param("extra").is_some_and(|e| !e.is_empty()) {
            return Err("the watch speaks no XHTTP with extra settings".into());
        }
        let tls = security == "tls";
        let alpn: Vec<String> = link
            .param("alpn")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect();
        let version = match alpn.as_slice() {
            _ if !tls => Version::H1,
            [one] if one == "http/1.1" => Version::H1,
            [one] if one == "h3" => return Err("the watch speaks no XHTTP over HTTP/3".into()),
            _ => Version::H2,
        };
        let alpn = if alpn.is_empty() { vec!["h2".into(), "http/1.1".into()] } else { alpn };
        let raw = link.param("path").unwrap_or_default();
        let (path, query) = raw.split_once('?').unwrap_or((raw, ""));
        let mut path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
        if !path.ends_with('/') {
            path.push('/');
        }
        let sni = tls.then(|| link.param("sni").filter(|s| !s.is_empty()).unwrap_or(&link.host).to_string());
        let host = match link.param("host").filter(|h| !h.is_empty()) {
            Some(host) => host.to_string(),
            None => sni.clone().unwrap_or_else(|| link.host.clone()),
        };
        Ok(Plan { path, query: query.to_string(), host, sni, alpn, version })
    }

    /// The target of a request for `tail` (the session, or the session and
    /// a sequence number), with the query of the path.
    fn target(&self, tail: &str) -> String {
        match self.query.as_str() {
            "" => format!("{}{tail}", self.path),
            query => format!("{}{tail}?{query}", self.path),
        }
    }

    /// The target of the upload of sequence number `seq` of the session
    /// `id`: the server puts the uploads in the order of these numbers.
    fn upload(&self, id: &str, seq: u64) -> String {
        self.target(&format!("{id}/{seq}"))
    }

    /// The host as an authority: an IPv6 address in brackets.
    fn authority(&self) -> String {
        if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() }
    }

    fn scheme(&self) -> &'static str {
        if self.sni.is_some() { "https" } else { "http" }
    }

    /// The padding of one request: a `Referer` of the URL of the path, with
    /// the query `x_padding=` and 100 to 1000 `X`.
    fn referer(&self) -> Result<String, Finding> {
        let mut two = [0u8; 2];
        OsEntropy.fill(&mut two).map_err(|e| Finding::failed(format!("the padding: {e}")))?;
        let length = 100 + usize::from(u16::from_be_bytes(two)) % 901;
        Ok(format!("{}://{}{}?x_padding={}", self.scheme(), self.authority(), self.path, "X".repeat(length)))
    }
}

/// One session of the mode `packet-up` to the node of `link`, with `first`
/// as the upload of sequence 0: the reader of its downloads, and the HTTP
/// version.
pub async fn session(net: &Net, link: &Link, plan: &Plan, first: Vec<u8>) -> Result<(Down, Version), Finding> {
    for part in [&plan.path, &plan.query, &plan.host] {
        if part.bytes().any(|b| !b.is_ascii_graphic()) {
            return Err(Finding::failed(format!("the XHTTP part {part:?} of the link is not visible ASCII")));
        }
    }
    let mut random = [0u8; 16];
    OsEntropy.fill(&mut random).map_err(|e| Finding::failed(format!("the session id: {e}")))?;
    let id = uuid(random);
    let alpn: Vec<&[u8]> = plan.alpn.iter().map(|p| p.as_bytes()).collect();
    let (io, version) = connect(net, link, plan, &alpn).await?;
    let down = match version {
        Version::H1 => h1(net, link, plan, io, &id, first).await?,
        Version::H2 => h2(net, plan, io, &id, first).await?,
    };
    Ok((down, version))
}

/// A UUID of version 4 of `bytes`, in its text form.
fn uuid(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = crate::args::hex(&bytes);
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// A connection to the node: TCP, and TLS with its server name that offers
/// `alpn`; the version of its ALPN, else the one of the plan.
async fn connect(net: &Net, link: &Link, plan: &Plan, alpn: &[&[u8]]) -> Result<(BoxedIo, Version), Finding> {
    let Some(sni) = &plan.sni else {
        return Ok((net.tcp(&link.host, link.port).await?, plan.version));
    };
    let tls = net.tls_alpn(&link.host, link.port, sni, alpn).await?;
    let version = match tls.get_ref().1.alpn_protocol() {
        Some(b"h2") => Version::H2,
        Some(b"http/1.1") => Version::H1,
        _ => plan.version,
    };
    Ok((Box::new(tls), version))
}

fn failed(what: &str, e: impl std::fmt::Display) -> Finding {
    Finding::failed(format!("the XHTTP {what}: {e}"))
}

/// A status that is not 200: Xray's server answers 200 to each request
/// that it takes.
fn refused(what: &str, status: u16) -> Result<(), Finding> {
    match status {
        200 => Ok(()),
        status => Err(Finding::failed(format!("the XHTTP {what} answered HTTP {status}"))),
    }
}

/// HTTP/1.1: the GET on `down`, and the POST on a second connection that
/// offers `http/1.1` alone. Both go at once: a server can hold the head of
/// the GET until the first upload.
async fn h1(net: &Net, link: &Link, plan: &Plan, mut down: BoxedIo, id: &str, first: Vec<u8>) -> Result<Down, Finding> {
    let host = plan.authority();
    let get = format!(
        "GET {} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {}\r\nReferer: {}\r\nConnection: close\r\n\r\n",
        plan.target(id),
        net.user_agent,
        plan.referer()?
    );
    let referer = plan.referer()?;
    let download = async {
        down.write_all(get.as_bytes()).await?;
        down.flush().await?;
        read_head(&mut down, Vec::new(), MAX_HEAD).await
    };
    let upload = async {
        let (mut up, _) = connect(net, link, plan, HTTP1).await?;
        let target = plan.upload(id, 0);
        let headers = [("Referer", referer.as_str())];
        exchange(&mut up, &net.user_agent, "POST", &host, &target, &headers, &first, 4096)
            .await
            .map_err(|e| failed("upload", e))
    };
    let (download, upload) = tokio::join!(download, upload);
    refused("upload", upload?.status)?;
    let (head, rest) = download.map_err(|e| failed("download", e))?;
    refused("download", head.status)?;
    let body = Body::new(down, rest, Framing::of(&head, false), None).map_err(|e| failed("download", e))?;
    Ok(Box::new(body))
}

/// HTTP/2: the GET and the POST as two streams of the connection `io`.
async fn h2(net: &Net, plan: &Plan, io: BoxedIo, id: &str, first: Vec<u8>) -> Result<Down, Finding> {
    let (client, connection) = h2::client::handshake(io).await.map_err(|e| failed("HTTP/2", e))?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    let driver = Driver(driver);
    let request = |method: &str, target: String| -> Result<http::request::Builder, Finding> {
        let uri = format!("{}://{}{target}", plan.scheme(), plan.authority());
        Ok(http::Request::builder()
            .method(method)
            .uri(uri)
            .header("user-agent", net.user_agent.as_str())
            .header("referer", plan.referer()?))
    };
    let get = request("GET", plan.target(id))?.body(()).map_err(|e| failed("download", e))?;
    let post = request("POST", plan.upload(id, 0))?
        .header("content-length", first.len())
        .body(())
        .map_err(|e| failed("upload", e))?;
    let mut client = client.ready().await.map_err(|e| failed("HTTP/2", e))?;
    let (download, _) = client.send_request(get, true).map_err(|e| failed("download", e))?;
    let mut client = client.ready().await.map_err(|e| failed("HTTP/2", e))?;
    let (upload, mut body) = client.send_request(post, false).map_err(|e| failed("upload", e))?;
    body.send_data(Bytes::from(first), true).map_err(|e| failed("upload", e))?;
    let (download, upload) = tokio::join!(download, upload);
    refused("upload", upload.map_err(|e| failed("upload", e))?.status().as_u16())?;
    let download = download.map_err(|e| failed("download", e))?;
    refused("download", download.status().as_u16())?;
    Ok(Box::new(H2Down { recv: download.into_body(), buf: Bytes::new(), _client: client, _driver: driver }))
}

/// The task that drives an HTTP/2 connection; a drop ends it.
struct Driver(tokio::task::JoinHandle<()>);

impl Drop for Driver {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The body of the GET on HTTP/2 as a reader. It holds the connection: its
/// handle of requests and its driver.
struct H2Down {
    recv: h2::RecvStream,
    buf: Bytes,
    _client: h2::client::SendRequest<Bytes>,
    _driver: Driver,
}

impl AsyncRead for H2Down {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, out: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        while self.buf.is_empty() {
            match ready!(self.recv.poll_data(cx)) {
                None => return Poll::Ready(Ok(())),
                Some(Err(e)) => return Poll::Ready(Err(std::io::Error::other(e))),
                Some(Ok(chunk)) => {
                    let _ = self.recv.flow_control().release_capacity(chunk.len());
                    self.buf = chunk;
                }
            }
        }
        let n = out.remaining().min(self.buf.len());
        let taken = self.buf.split_to(n);
        out.put_slice(&taken);
        Poll::Ready(Ok(()))
    }
}
