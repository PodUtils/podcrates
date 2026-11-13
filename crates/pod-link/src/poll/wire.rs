//! The HTTP/1.1 of the poll carriers: a buffered connection that reads a
//! head, a body of a known length and the chunks of a chunked body; the
//! heads that each side writes; the query of a target; the events of SSE.

use std::io;

use base64::Engine as _;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

/// The longest head of a request or an answer.
pub const MAX_HEAD: usize = 4 << 10;

/// The most bytes of one event of SSE, decoded.
pub const EVENT: usize = 8 << 10;

/// The comment of SSE that keeps a stream alive, and that opens it.
pub(super) const PING: &[u8] = b": ping\n\n";

/// The event of SSE at the end of the stream.
pub(super) const FIN_EVENT: &[u8] = b"event: fin\ndata\n\n";

/// The last chunk of a chunked body, with no trailer.
pub(super) const LAST_CHUNK: &[u8] = b"0\r\n\r\n";

fn bad(text: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, text.into())
}

/// The head of a request or of an answer.
#[derive(Debug)]
pub struct Head {
    /// The request line or the status line.
    pub line: String,
    fields: Vec<(String, String)>,
}

impl Head {
    pub fn parse(text: &str) -> io::Result<Head> {
        let mut lines = text.split("\r\n");
        let line = lines.next().unwrap_or_default().to_string();
        let mut fields = Vec::new();
        for each in lines.filter(|l| !l.is_empty()) {
            if each.starts_with([' ', '\t']) {
                return Err(bad("a folded header line"));
            }
            let (name, value) = each.split_once(':').ok_or_else(|| bad("a header line with no colon"))?;
            fields.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
        Ok(Head { line, fields })
    }

    /// The value of the field `name` (in lower case).
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// The status of an answer.
    pub fn status(&self) -> Option<u16> {
        let mut parts = self.line.splitn(3, ' ');
        let version = parts.next()?;
        if !version.starts_with("HTTP/1.") {
            return None;
        }
        parts.next()?.parse().ok()
    }

    /// The method and the target of a request.
    pub fn request(&self) -> Option<(&str, &str)> {
        let mut parts = self.line.split(' ');
        let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
        (parts.next().is_none() && version.starts_with("HTTP/1.")).then_some((method, target))
    }

    /// The Content-Length, when it has one.
    pub fn length(&self) -> io::Result<Option<usize>> {
        self.field("content-length").map(|v| v.parse::<usize>().map_err(|_| bad("a bad Content-Length"))).transpose()
    }

    /// The body is chunked.
    pub fn chunked(&self) -> bool {
        self.field("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    }

    /// The other side closes the connection after this message.
    pub fn close(&self) -> bool {
        self.field("connection").is_some_and(|v| v.eq_ignore_ascii_case("close"))
    }
}

/// A connection with the bytes that it read past the last message.
pub struct Conn<S> {
    pub io: S,
    buf: Vec<u8>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Conn<S> {
    pub fn new(io: S) -> Conn<S> {
        Conn { io, buf: Vec::new() }
    }

    async fn fill(&mut self) -> io::Result<usize> {
        let mut more = [0u8; 16 << 10];
        let n = self.io.read(&mut more).await?;
        self.buf.extend_from_slice(&more[..n]);
        Ok(n)
    }

    /// The bytes read past the last message.
    #[cfg(feature = "fuzzing")]
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// The next head of at most [`MAX_HEAD`] bytes with its blank line;
    /// none at an end of the stream before its first byte.
    pub async fn head(&mut self) -> io::Result<Option<Head>> {
        loop {
            // A read can bring more than the longest head: look in that
            // part alone.
            let within = &self.buf[..self.buf.len().min(MAX_HEAD)];
            if let Some(at) = within.windows(4).position(|w| w == b"\r\n\r\n") {
                let text = std::str::from_utf8(&self.buf[..at]).map_err(|_| bad("a head that is not UTF-8"))?;
                let head = Head::parse(text)?;
                self.buf.drain(..at + 4);
                return Ok(Some(head));
            }
            if self.buf.len() >= MAX_HEAD {
                return Err(bad(format!("a head of more than {MAX_HEAD} bytes")));
            }
            if self.fill().await? == 0 {
                return match self.buf.is_empty() {
                    true => Ok(None),
                    false => Err(io::ErrorKind::UnexpectedEof.into()),
                };
            }
        }
    }

    /// A body of `len` bytes.
    pub async fn body(&mut self, len: usize) -> io::Result<Vec<u8>> {
        while self.buf.len() < len {
            if self.fill().await? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
        Ok(self.buf.drain(..len).collect())
    }

    async fn line(&mut self) -> io::Result<String> {
        loop {
            if let Some(at) = self.buf.windows(2).position(|w| w == b"\r\n") {
                let line = String::from_utf8_lossy(&self.buf[..at]).into_owned();
                self.buf.drain(..at + 2);
                return Ok(line);
            }
            if self.buf.len() > 256 {
                return Err(bad("a chunk line of more than 256 bytes"));
            }
            if self.fill().await? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
    }

    /// The next chunk of a chunked body, at most `max` bytes; none at the
    /// last chunk.
    pub async fn chunk(&mut self, max: usize) -> io::Result<Option<Vec<u8>>> {
        let line = self.line().await?;
        let size = line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size, 16).map_err(|_| bad("a bad chunk size"))?;
        if size == 0 {
            while !self.line().await?.is_empty() {}
            return Ok(None);
        }
        if size > max {
            return Err(bad(format!("a chunk of {size} bytes, more than {max}")));
        }
        let data = self.body(size).await?;
        if !self.line().await?.is_empty() {
            return Err(bad("no end of line after a chunk"));
        }
        Ok(Some(data))
    }

    /// Write `bytes` and flush them.
    pub async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.io.write_all(bytes).await?;
        self.io.flush().await
    }
}

/// The head of a request. A body has a Content-Length; a GET has none.
pub(super) fn request(method: &str, target: &str, host: &str, body: Option<usize>, accept: Option<&str>) -> String {
    let mut head = format!("{method} {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: pod-link\r\n");
    if let Some(len) = body {
        head.push_str(&format!("Content-Type: application/octet-stream\r\nContent-Length: {len}\r\n"));
    }
    if let Some(accept) = accept {
        head.push_str(&format!("Accept: {accept}\r\n"));
    }
    head.push_str("\r\n");
    head
}

/// The body of an answer.
pub(super) enum Body {
    Length(usize),
    Chunked(&'static str),
}

/// The head of an answer with `fields`.
pub(super) fn answer(status: u16, fields: &[(&str, String)], body: Body) -> String {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        406 => "Not Acceptable",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Content Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        _ => "Status",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\nCache-Control: no-store\r\n");
    for (name, value) in fields {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    match body {
        Body::Length(_) if status == 204 => {}
        Body::Length(len) => head.push_str(&format!("Content-Length: {len}\r\n")),
        Body::Chunked(kind) => head.push_str(&format!("Content-Type: {kind}\r\nTransfer-Encoding: chunked\r\n")),
    }
    head.push_str("\r\n");
    head
}

/// One chunk of a chunked body (not empty).
pub(super) fn chunk(data: &[u8]) -> Vec<u8> {
    let mut out = format!("{:x}\r\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\r\n");
    out
}

/// The path and the query of a target.
pub(super) fn split(target: &str) -> (&str, &str) {
    target.split_once('?').unwrap_or((target, ""))
}

/// The value of `name` in a query.
pub(super) fn param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

/// One event of SSE with the bytes of the link, and the offset after them.
pub(super) fn event(end: u64, data: &[u8]) -> Vec<u8> {
    format!("id: {end}\ndata: {}\n\n", base64::engine::general_purpose::STANDARD.encode(data)).into_bytes()
}

/// What a block of SSE gives.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// Comments alone.
    Ping,
    /// The bytes of the link.
    Data(Vec<u8>),
    /// The end of the stream.
    Fin,
}

/// The reader of the events of SSE from the chunks of a body.
#[derive(Default)]
pub struct Events {
    text: Vec<u8>,
}

impl Events {
    pub fn push(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.text.extend_from_slice(bytes);
        // The base64 of the largest event and its id, with room.
        if self.text.len() > 2 * EVENT + 256 && !self.text.windows(2).any(|w| w == b"\n\n") {
            return Err(bad("an event of SSE with no end"));
        }
        Ok(())
    }

    // A read that can fail, not an iterator.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<Option<Event>> {
        let Some(at) = self.text.windows(2).position(|w| w == b"\n\n") else { return Ok(None) };
        let block: Vec<u8> = self.text.drain(..at + 2).collect();
        let block = std::str::from_utf8(&block).map_err(|_| bad("an event of SSE that is not UTF-8"))?;
        let (mut data, mut fin, mut any) = (String::new(), false, false);
        for line in block.lines().filter(|l| !l.is_empty() && !l.starts_with(':')) {
            let (name, value) = line.split_once(':').map_or((line, ""), |(n, v)| (n, v.trim_start()));
            match name {
                "data" => {
                    data.push_str(value);
                    any = true;
                }
                "event" => fin = value == "fin",
                _ => {}
            }
        }
        if fin {
            return Ok(Some(Event::Fin));
        }
        if !any {
            return Ok(Some(Event::Ping));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.as_bytes())
            .map_err(|_| bad("an event of SSE whose data is not base64"))?;
        if bytes.len() > EVENT {
            return Err(bad(format!("an event of SSE of {} bytes, more than {EVENT}", bytes.len())));
        }
        Ok(Some(Event::Data(bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_head_a_body_and_chunks_are_read_in_turn() {
        let (near, mut far) = tokio::io::duplex(1 << 16);
        let mut wire = answer(200, &[("Pod-Fin", "1".into())], Body::Length(3)).into_bytes();
        wire.extend_from_slice(b"abc");
        wire.extend_from_slice(answer(200, &[], Body::Chunked("application/octet-stream")).as_bytes());
        wire.extend_from_slice(&chunk(b"hello"));
        wire.extend_from_slice(LAST_CHUNK);
        far.write_all(&wire).await.unwrap();
        drop(far);
        let mut conn = Conn::new(near);
        let head = conn.head().await.unwrap().unwrap();
        assert_eq!((head.status(), head.field("pod-fin"), head.length().unwrap()), (Some(200), Some("1"), Some(3)));
        assert_eq!(conn.body(3).await.unwrap(), b"abc");
        let head = conn.head().await.unwrap().unwrap();
        assert!(head.chunked());
        assert_eq!(conn.chunk(64).await.unwrap().unwrap(), b"hello");
        assert_eq!(conn.chunk(64).await.unwrap(), None);
        assert!(conn.head().await.unwrap().is_none(), "a clean end");
    }

    #[tokio::test]
    async fn a_long_head_and_a_large_chunk_are_refused() {
        let (near, mut far) = tokio::io::duplex(1 << 16);
        far.write_all(&vec![b'a'; MAX_HEAD + 10]).await.unwrap();
        let e = Conn::new(near).head().await.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        // A head that comes whole in one read is refused as well.
        let (near, mut far) = tokio::io::duplex(1 << 16);
        let mut long = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD)).into_bytes();
        long.extend_from_slice(b"GET / HTTP/1.1\r\n\r\n");
        far.write_all(&long).await.unwrap();
        let e = Conn::new(near).head().await.expect_err("a head over 4 KiB in one read");
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        let (near, mut far) = tokio::io::duplex(1 << 16);
        let mut most = format!("GET / HTTP/1.1\r\nX: {}", "a".repeat(MAX_HEAD - 23)).into_bytes();
        most.extend_from_slice(b"\r\n\r\n");
        assert_eq!(most.len(), MAX_HEAD);
        far.write_all(&most).await.unwrap();
        assert!(Conn::new(near).head().await.unwrap().is_some(), "a head of 4 KiB with its blank line");
        let (near, mut far) = tokio::io::duplex(1 << 16);
        far.write_all(&chunk(&[0; 100])).await.unwrap();
        assert!(Conn::new(near).chunk(99).await.is_err());
    }

    #[test]
    fn a_request_line_and_a_query_are_read() {
        let head = Head::parse(&request("POST", "/l/up?s=ab&seq=7", "h:1", Some(0), None)[..]).unwrap();
        let (method, target) = head.request().unwrap();
        let (path, query) = split(target);
        assert_eq!((method, path, param(query, "seq"), param(query, "s")), ("POST", "/l/up", Some("7"), Some("ab")));
        assert_eq!(param(query, "x"), None);
        assert!(Head::parse("GET / HTTP/1.1\r\n folded").is_err());
    }

    #[test]
    fn events_of_sse_give_bytes_pings_and_the_end() {
        let mut events = Events::default();
        let mut wire = PING.to_vec();
        wire.extend_from_slice(&event(5, b"hello"));
        wire.extend_from_slice(FIN_EVENT);
        events.push(&wire[..7]).unwrap();
        events.push(&wire[7..]).unwrap();
        assert_eq!(events.next().unwrap(), Some(Event::Ping));
        assert_eq!(events.next().unwrap(), Some(Event::Data(b"hello".to_vec())));
        assert_eq!(events.next().unwrap(), Some(Event::Fin));
        assert_eq!(events.next().unwrap(), None);
        let mut events = Events::default();
        events.push(&event(1, &[0; EVENT + 1])).unwrap();
        assert!(events.next().is_err(), "an event over 8 KiB");
    }
}
