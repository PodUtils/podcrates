//! The little HTTP/1.1 that the consumers need besides the WebSocket
//! upgrade: one request and one response over an already-open stream, with
//! bounded sizes. It reads `Content-Length`, `Transfer-Encoding: chunked`,
//! and read-until-close bodies, passes over a few interim (1xx) answers, and
//! takes the end of a TLS stream with no close_notify as the end of the
//! stream. [`read_head`] and [`Body`] read a body as it comes, with a limit
//! for each wait ([`Idle`]); [`read_answer`] reads it whole through them.
//! The other side, a server's reading of a request, is [`request`].

mod body;
pub mod request;

pub use body::{Body, Framing, Idle};
pub use request::{Request, RequestError, read_request};

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
    let parts = [user_agent, method, host, path].into_iter();
    for part in parts.chain(extra_headers.iter().flat_map(|(n, v)| [*n, *v])) {
        if part.contains(['\r', '\n']) {
            return Err(invalid("a request line or header contains CR or LF"));
        }
    }
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n");
    if !user_agent.is_empty() {
        request.push_str(&format!("User-Agent: {user_agent}\r\n"));
    }
    request.push_str("Connection: close\r\n");
    for (name, value) in extra_headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if !body.is_empty() || method != "GET" {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    read_response(stream, Vec::new(), max_body).await
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
    response.body = Body::new(stream, rest, framing, Some(limit))?.to_end().await?;
    Ok(response)
}

/// Read the head of a response whose first bytes may already be in
/// `buffered`, each head of `max_head` bytes at most, past up to
/// [`MAX_INTERIM`] interim answers. The answer comes with no body, and with
/// the bytes after its head that were read.
pub async fn read_head<S>(
    stream: &mut S,
    mut buffered: Vec<u8>,
    max_head: usize,
) -> std::io::Result<(Response, Vec<u8>)>
where
    S: AsyncRead + Unpin,
{
    let mut chunk = [0u8; 4096];
    let mut interim = 0;
    loop {
        let head_end = loop {
            if let Some(end) = find(&buffered, b"\r\n\r\n") {
                break end + 4;
            }
            if buffered.len() > max_head {
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
            buffered.extend_from_slice(&chunk[..n]);
        };
        let rest = buffered.split_off(head_end);
        let head = String::from_utf8_lossy(&buffered).into_owned();
        let (status, reason, headers) = parse_head(&head).map_err(invalid)?;
        if (100..200).contains(&status) && status != 101 {
            interim += 1;
            if interim > MAX_INTERIM {
                return Err(invalid(format!("the server sent more than {MAX_INTERIM} interim answers")));
            }
            buffered = rest;
            continue;
        }
        return Ok((Response { status, reason, headers, body: Vec::new(), url: String::new() }, rest));
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

/// A response head: the status, the reason, and the headers, names in lower
/// case.
pub type Head = (u16, String, Vec<(String, String)>);

/// Parse a response head into status, reason and lower-cased headers.
pub fn parse_head(head: &str) -> Result<Head, String> {
    let (status, reason) = parse_status_line(head)?;
    let headers = head
        .split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Ok((status, reason, headers))
}

/// `HTTP/1.1 200 Connection Established` → `(200, "Connection Established")`.
pub fn parse_status_line(head: &str) -> Result<(u16, String), String> {
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/") {
        return Err(format!("not an HTTP status line: {line:?}"));
    }
    let status =
        parts.next().and_then(|s| s.parse::<u16>().ok()).ok_or_else(|| format!("no status code in {line:?}"))?;
    // The reason phrase is the proxy's text and ends up on the user's terminal.
    Ok((status, one_line(parts.next().unwrap_or_default())))
}

/// Decode a complete chunked body. `Ok(None)` means more bytes are needed.
/// Trailers are ignored; chunk extensions are skipped.
pub fn decode_chunked(raw: &[u8], max_body: usize) -> Result<Option<Vec<u8>>, String> {
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        let Some(line_end) = find(&raw[at..], b"\r\n") else { return Ok(None) };
        let size_line =
            std::str::from_utf8(&raw[at..at + line_end]).map_err(|_| "a chunk size line is not text".to_string())?;
        let size_hex = size_line.split(';').next().unwrap_or_default().trim();
        let size = match usize::from_str_radix(size_hex, 16) {
            Ok(size) => size,
            // Hex digits past the width of usize: a size larger than any bound.
            Err(_) if !size_hex.is_empty() && size_hex.bytes().all(|b| b.is_ascii_hexdigit()) => usize::MAX,
            Err(_) => return Err(format!("bad chunk size {size_hex:?}")),
        };
        at += line_end + 2;
        if size == 0 {
            // Optional trailers, then the final CRLF.
            return match find(&raw[at..], b"\r\n\r\n") {
                Some(_) => Ok(Some(out)),
                None if raw[at..].starts_with(b"\r\n") => Ok(Some(out)),
                None => Ok(None),
            };
        }
        // The peer chooses the size: no sum with it may wrap.
        if size > max_body.saturating_sub(out.len()) {
            return Err("the response body is larger than allowed".into());
        }
        let Some(end) = at.checked_add(size).and_then(|end| end.checked_add(2)) else {
            return Err(format!("bad chunk size {size_hex:?}"));
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

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn invalid(why: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, why.into())
}
