//! The little HTTP/1.1 that the consumers need besides the WebSocket
//! upgrade: one request and one response over an already-open stream, with
//! bounded sizes. It reads `Content-Length`, `Transfer-Encoding: chunked`,
//! and read-until-close bodies, passes over a few interim (1xx) answers, and
//! takes the end of a TLS stream with no close_notify as the end of the
//! stream. [`read_head`] and [`Body`] read a body as it comes, with a limit
//! for each wait ([`Idle`]); [`read_answer`] reads it whole through them.
//! The other side, a server's reading of a request, is [`request`].

mod body;
mod large;
pub mod request;

pub use body::{Body, Framing, Idle};
pub use large::{BodySize, KEPT_HEAD, TooLarge};
pub use pod_core::http1::{Head, parse_head, parse_status_line};
pub use request::{Request, RequestError, read_request};

use pod_core::http1::{Exact, find};
use pod_core::text::one_line;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The longest response head accepted by [`read_response`].
pub const MAX_HEAD: usize = 16 * 1024;

/// The most interim (1xx) answers before the answer.
pub const MAX_INTERIM: usize = 5;

/// The bounds of the reading of one response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// The longest head, of each interim answer and of the answer.
    pub max_head: usize,
    /// The longest body.
    pub max_body: usize,
    /// The answer to a `HEAD`: a head with no body.
    pub head_only: bool,
}

/// One HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    /// Header names lower-cased, values trimmed, in arrival order.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The URL that gave the answer of [`crate::Client::send`]: after
    /// redirects, the last one. Empty for the answer of an exchange over a
    /// stream that the caller opened.
    pub url: String,
}

impl Response {
    /// A status of 2xx.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The body as lossy UTF-8.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The first value of a header, by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }

    /// The body as text, for error messages: lossy UTF-8 made safe for a
    /// terminal (see [`pod_core::text::one_line`]), cut to `max` characters.
    pub fn body_text(&self, max: usize) -> String {
        cut(one_line(&String::from_utf8_lossy(&self.body)), max)
    }

    /// A header's first value as text for a line, as [`Self::body_text`]
    /// makes the body: a peer's header reaches the terminal too.
    pub fn header_text(&self, name: &str, max: usize) -> Option<String> {
        self.header(name).map(|value| cut(one_line(value), max))
    }
}

/// `text` cut to `max` characters, with an ellipsis where it was cut.
fn cut(text: String, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text,
    }
}

/// Send one request and read the response, closing nothing. `user_agent`
/// (`podssh/VERSION`) goes in `User-Agent`, and none goes with an empty
/// one. `extra_headers` must not contain CR or LF (they are refused). The
/// response body is capped at `max_body` bytes.
#[allow(clippy::too_many_arguments)]
pub async fn exchange<S>(
    stream: &mut S,
    user_agent: &str,
    method: &str,
    host: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
    max_body: usize,
) -> std::io::Result<Response>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = request_text(user_agent, method, host, path, extra_headers, body, false)?;
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    read_response(stream, Vec::new(), max_body).await
}

/// [`exchange`] over a stream that the call takes, with `Connection:
/// keep-alive`: the answer, and the stream when it can carry the next
/// request: its body was read to its end by its length or its chunks, with
/// no byte after it, and the server did not say `Connection: close`.
#[allow(clippy::too_many_arguments)]
pub async fn exchange_keep<S>(
    mut stream: S,
    user_agent: &str,
    method: &str,
    host: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
    max_body: usize,
) -> std::io::Result<(Response, Option<S>)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = request_text(user_agent, method, host, path, extra_headers, body, true)?;
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    let (mut response, rest) = read_head(&mut stream, Vec::new(), MAX_HEAD).await?;
    let framing = Framing::of(&response, method == "HEAD");
    let limit = u64::try_from(max_body).unwrap_or(u64::MAX);
    let mut reading = Body::new(stream, rest, framing, Some(limit))?.of_answer(&response);
    response.body = read_whole(&mut reading).await?;
    let closes = response.header("connection").is_some_and(|v| v.to_ascii_lowercase().contains("close"));
    Ok((response, if closes { None } else { reading.into_reusable() }))
}

async fn read_whole<S: AsyncRead + Unpin>(body: &mut Body<S>) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    body.read_to_end(&mut out).await?;
    Ok(out)
}

/// The head of a request; CR, LF and NUL in a part are refused.
fn request_text(
    user_agent: &str,
    method: &str,
    host: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
    keep: bool,
) -> std::io::Result<String> {
    let mut headers = vec![("Host", host)];
    if !user_agent.is_empty() {
        headers.push(("User-Agent", user_agent));
    }
    headers.push(("Connection", if keep { "keep-alive" } else { "close" }));
    headers.extend_from_slice(extra_headers);
    let length = body.len().to_string();
    if !body.is_empty() || method != "GET" {
        headers.push(("Content-Length", &length));
    }
    pod_core::http1::request_head(method, path, &headers).map_err(invalid)
}

/// Read a response whose first bytes may already be in `buffered`, with a
/// head of [`MAX_HEAD`] at most and a body of `max_body`.
pub async fn read_response<S>(stream: &mut S, buffered: Vec<u8>, max_body: usize) -> std::io::Result<Response>
where
    S: AsyncRead + Unpin,
{
    read_answer(stream, buffered, Reading { max_head: MAX_HEAD, max_body, head_only: false }).await
}

/// Read a response whose first bytes may already be in `buffered`, within
/// the bounds of `reading`. Up to [`MAX_INTERIM`] interim answers (1xx but
/// 101) come before it and are passed over; 101 is the answer.
pub async fn read_answer<S>(stream: &mut S, buffered: Vec<u8>, reading: Reading) -> std::io::Result<Response>
where
    S: AsyncRead + Unpin,
{
    let (mut response, rest) = read_head(stream, buffered, reading.max_head).await?;
    let framing = Framing::of(&response, reading.head_only);
    let limit = u64::try_from(reading.max_body).unwrap_or(u64::MAX);
    response.body = Body::new(stream, rest, framing, Some(limit))?.of_answer(&response).to_end().await?;
    Ok(response)
}

/// Read the head of a response whose first bytes may already be in
/// `buffered`, each head of `max_head` bytes at most, past up to
/// [`MAX_INTERIM`] interim answers. The answer comes with no body, and with
/// the bytes after its head that were read.
pub async fn read_head<S>(stream: &mut S, buffered: Vec<u8>, max_head: usize) -> std::io::Result<(Response, Vec<u8>)>
where
    S: AsyncRead + Unpin,
{
    read_head_versioned(stream, buffered, max_head).await.map(|(answer, rest, _)| (answer, rest))
}

/// [`read_head`], and whether the answer is of `HTTP/1.1`: only such a
/// connection can carry a next request.
pub(crate) async fn read_head_versioned<S>(
    stream: &mut S,
    mut buffered: Vec<u8>,
    max_head: usize,
) -> std::io::Result<(Response, Vec<u8>, bool)>
where
    S: AsyncRead + Unpin,
{
    let (response, http11) = read_head_in(stream, &mut buffered, max_head).await?;
    Ok((response, buffered, http11))
}

/// [`read_head`] after the bytes in `buf`, which keeps the bytes after the
/// head; and whether the answer is of `HTTP/1.1`, for
/// [`pod_core::http1::keeps`]. The reader of an answer whose body comes
/// piece by piece reads its head so.
pub async fn read_head_in<S>(stream: &mut S, buf: &mut Vec<u8>, max_head: usize) -> std::io::Result<(Response, bool)>
where
    S: AsyncRead + Unpin,
{
    let mut chunk = [0u8; 4096];
    let mut interim = 0;
    loop {
        let head_end = loop {
            if let Some(end) = pod_core::http1::crlf_head_end(buf) {
                break end;
            }
            if buf.len() > max_head {
                let kib = max_head / 1024;
                return Err(invalid(format!("no end of the response headers within {kib} KiB")));
            }
            let n = read_some(stream, &mut chunk).await?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "the connection closed before the response headers ended",
                ));
            }
            buf.extend_from_slice(&chunk[..n]);
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        buf.drain(..head_end);
        let (status, reason, headers) = parse_head(&head).map_err(invalid)?;
        if (100..200).contains(&status) && status != 101 {
            interim += 1;
            if interim > MAX_INTERIM {
                return Err(invalid(format!("the server sent more than {MAX_INTERIM} interim answers")));
            }
            continue;
        }
        let http11 = head.starts_with("HTTP/1.1 ");
        return Ok((Response { status, reason, headers, body: Vec::new(), url: String::new() }, http11));
    }
}

/// Read a head one byte at a time, so that no byte after its empty line is
/// taken: those bytes belong to the tunnel or the TLS session that follows.
/// At most `max` bytes.
pub async fn read_head_exact<S: AsyncRead + Unpin>(stream: &mut S, max: usize) -> std::io::Result<String> {
    let mut head = Exact::new(max);
    let mut byte = [0u8; 1];
    loop {
        if stream.read(&mut byte).await? == 0 {
            return Err(Exact::closed());
        }
        if head.push(byte[0])? {
            return Ok(head.into_text());
        }
    }
}

/// Read some bytes. The end of a TLS stream with no close_notify is the end
/// of the stream: the length of a framed body is checked on its own.
async fn read_some<S: AsyncRead + Unpin>(stream: &mut S, buf: &mut [u8]) -> std::io::Result<usize> {
    match stream.read(buf).await {
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(0),
        other => other,
    }
}

/// Decode a complete chunked body. `Ok(None)` means more bytes are needed.
/// Trailers are ignored; chunk extensions are skipped. A body over
/// `max_body` is the text of a [`TooLarge`] with no status.
pub fn decode_chunked(raw: &[u8], max_body: usize) -> Result<Option<Vec<u8>>, String> {
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        let Some(line_end) = find(&raw[at..], b"\r\n") else { return Ok(None) };
        let size = large::chunk_size(&raw[at..at + line_end])?;
        at += line_end + 2;
        let refuse = |size| {
            let mut head = out[..out.len().min(KEPT_HEAD)].to_vec();
            let more = raw.len().saturating_sub(at).min(KEPT_HEAD - head.len());
            head.extend_from_slice(&raw[at..at + more]);
            let bound = max_body as u64;
            TooLarge { status: None, reason: String::new(), size, bound, head }.to_string()
        };
        let size = match size {
            Err(digits) => return Err(refuse(BodySize::PastAnyBound { digits })),
            // The peer chooses the size: no sum with it may wrap.
            Ok(size) if size > (max_body - out.len()) as u64 => {
                return Err(refuse(BodySize::Chunk { before: out.len() as u64, size }));
            }
            Ok(size) => size as usize,
        };
        if size == 0 {
            // Optional trailers, then the final CRLF.
            return match find(&raw[at..], b"\r\n\r\n") {
                Some(_) => Ok(Some(out)),
                None if raw[at..].starts_with(b"\r\n") => Ok(Some(out)),
                None => Ok(None),
            };
        }
        let Some(end) = at.checked_add(size).and_then(|end| end.checked_add(2)) else {
            return Err(format!("bad chunk size {size:#x}"));
        };
        if raw.len() < end {
            return Ok(None);
        }
        out.extend_from_slice(&raw[at..end - 2]);
        if &raw[end - 2..end] != b"\r\n" {
            return Err("a chunk is not followed by CRLF".into());
        }
        at = end;
    }
}

fn invalid(why: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, why.into())
}
