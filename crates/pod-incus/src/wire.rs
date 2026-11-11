//! HTTP/1.1 on the Unix socket of incus, with no TLS: one connection for
//! each call, `Connection: close`. A request body is bytes or a stream
//! ([`crate::Payload`]): a stream of known length goes with
//! `Content-Length`, another with `Transfer-Encoding: chunked`. The answer
//! is read by the HTTP reader of `pod-proxy` (by length, by chunks or to the
//! end), within a bound.

use std::io;
use std::time::Duration;

use pod_core::BoxedIo;
use pod_proxy::http::Response;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{Error, ErrorKind, Result};
use crate::upload::{Part, Upload};

/// The Host of each request: the name that the client of incus gives its
/// Unix socket.
pub const HOST: &str = "unix.socket";

/// The size of each chunk of a stream body of unknown length.
const CHUNK: usize = 64 * 1024;

/// The body of a request.
pub(crate) enum Body {
    Empty,
    /// JSON or raw bytes, with their type.
    Bytes {
        content_type: &'static str,
        data: Vec<u8>,
    },
    /// A stream: one payload, or the parts of a multipart body.
    Stream(Upload),
}

/// One request: the method, the target (path and query), the extra
/// headers and the body.
pub(crate) struct Request {
    pub method: &'static str,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

/// Open a connection to the socket at `socket`, within `limit`.
#[cfg(unix)]
pub(crate) async fn connect(call: &str, socket: &std::path::Path, limit: Duration) -> Result<BoxedIo> {
    let no_server = |cause: io::Error| Error::new(call, ErrorKind::NoServer { socket: socket.to_path_buf(), cause });
    match tokio::time::timeout(limit, tokio::net::UnixStream::connect(socket)).await {
        Ok(Ok(stream)) => Ok(Box::new(stream)),
        Ok(Err(e)) => Err(no_server(e)),
        Err(_) => Err(no_server(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("no connection within {} ms", limit.as_millis()),
        ))),
    }
}

/// No Unix socket on this system: each call is refused by name.
#[cfg(not(unix))]
pub(crate) async fn connect(call: &str, socket: &std::path::Path, _limit: Duration) -> Result<BoxedIo> {
    let cause = io::Error::new(io::ErrorKind::Unsupported, "this system has no Unix socket");
    Err(Error::new(call, ErrorKind::NoServer { socket: socket.to_path_buf(), cause }))
}

/// Write `request` with `user_agent` (none when empty).
pub(crate) async fn send<S>(stream: &mut S, user_agent: &str, request: Request) -> io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let named = request.headers.iter().flat_map(|(name, value)| [name.as_str(), value.as_str()]);
    if [request.method, request.target.as_str(), user_agent]
        .into_iter()
        .chain(named)
        .any(|part| part.contains(['\r', '\n', '\0']))
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a request line or header holds CR, LF or NUL"));
    }
    let mut head = format!("{} {} HTTP/1.1\r\nHost: {HOST}\r\n", request.method, request.target);
    if !user_agent.is_empty() {
        head.push_str(&format!("User-Agent: {user_agent}\r\n"));
    }
    head.push_str("Connection: close\r\n");
    for (name, value) in &request.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    match &request.body {
        Body::Empty if matches!(request.method, "POST" | "PUT" | "PATCH") => head.push_str("Content-Length: 0\r\n"),
        Body::Empty => {}
        Body::Bytes { content_type, data } => {
            head.push_str(&format!("Content-Type: {content_type}\r\nContent-Length: {}\r\n", data.len()));
        }
        Body::Stream(upload) => {
            head.push_str(&format!("Content-Type: {}\r\n", upload.content_type));
            match upload.length() {
                Some(length) => head.push_str(&format!("Content-Length: {length}\r\n")),
                None => head.push_str("Transfer-Encoding: chunked\r\n"),
            }
        }
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    match request.body {
        Body::Empty => {}
        Body::Bytes { data, .. } => stream.write_all(&data).await?,
        Body::Stream(upload) => write_upload(stream, upload).await?,
    }
    stream.flush().await
}

/// Each part of `upload`: as it is when its length is known, else in chunks.
async fn write_upload<S: AsyncWrite + Unpin>(stream: &mut S, upload: Upload) -> io::Result<()> {
    let chunked = upload.length().is_none();
    let mut buf = vec![0u8; CHUNK];
    for part in upload.parts {
        match part {
            Part::Bytes(bytes) => write_piece(stream, &bytes, chunked).await?,
            Part::Payload(mut payload) => {
                let mut sent = 0u64;
                loop {
                    let want = match payload.length {
                        Some(length) => usize::try_from(length - sent).unwrap_or(usize::MAX).min(CHUNK),
                        None => CHUNK,
                    };
                    if want == 0 {
                        break;
                    }
                    let n = payload.reader.read(&mut buf[..want]).await?;
                    if n == 0 {
                        break;
                    }
                    sent += n as u64;
                    write_piece(stream, &buf[..n], chunked).await?;
                }
                if let Some(length) = payload.length {
                    if sent < length {
                        let why = format!("the payload ended after {sent} of its {length} bytes");
                        return Err(io::Error::new(io::ErrorKind::InvalidInput, why));
                    }
                    if payload.reader.read(&mut buf[..1]).await? > 0 {
                        let why = format!("the payload holds more than its {length} bytes");
                        return Err(io::Error::new(io::ErrorKind::InvalidInput, why));
                    }
                }
            }
        }
    }
    if chunked {
        stream.write_all(b"0\r\n\r\n").await?;
    }
    Ok(())
}

async fn write_piece<S: AsyncWrite + Unpin>(stream: &mut S, bytes: &[u8], chunked: bool) -> io::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    if chunked {
        stream.write_all(format!("{:x}\r\n", bytes.len()).as_bytes()).await?;
        stream.write_all(bytes).await?;
        stream.write_all(b"\r\n").await
    } else {
        stream.write_all(bytes).await
    }
}

/// The answer of a request, its body of `max_body` bytes at most.
pub(crate) async fn read(stream: &mut BoxedIo, max_body: usize) -> io::Result<Response> {
    pod_proxy::http::read_response(stream, Vec::new(), max_body).await
}

/// `text` with each byte that is not unreserved (RFC 3986 section 2.3)
/// escaped: a segment of a path or a value of a query.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `path?query`, each value escaped.
pub(crate) fn target(path: &str, query: &[(&str, &str)]) -> String {
    let mut out = path.to_string();
    for (i, (name, value)) in query.iter().enumerate() {
        out.push(if i == 0 && !path.contains('?') { '?' } else { '&' });
        out.push_str(name);
        out.push('=');
        out.push_str(&escape(value));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Payload;

    #[test]
    fn escape_keeps_only_unreserved_bytes() {
        assert_eq!(escape("web-1.a_b~"), "web-1.a_b~");
        assert_eq!(escape("/etc/hosts file"), "%2Fetc%2Fhosts%20file");
        assert_eq!(escape("a&b=c?d#e"), "a%26b%3Dc%3Fd%23e");
        assert_eq!(escape("ч"), "%D1%87");
        assert_eq!(target("/1.0/x", &[("path", "/a b"), ("project", "p")]), "/1.0/x?path=%2Fa%20b&project=p");
        assert_eq!(target("/1.0/x?recursion=1", &[("project", "p")]), "/1.0/x?recursion=1&project=p");
    }

    async fn sent(request: Request) -> io::Result<String> {
        let mut out = Vec::new();
        send(&mut out, "pod-incus-test", request).await?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    #[tokio::test]
    async fn a_stream_of_unknown_length_goes_in_chunks() {
        let upload = Upload::single("application/octet-stream", Payload::new(&b"abcdef"[..], None));
        let request =
            Request { method: "POST", target: "/1.0/images".into(), headers: Vec::new(), body: Body::Stream(upload) };
        let text = sent(request).await.unwrap();
        assert!(text.contains("Transfer-Encoding: chunked\r\n"), "{text}");
        assert!(text.ends_with("\r\n\r\n6\r\nabcdef\r\n0\r\n\r\n"), "{text}");
    }

    #[tokio::test]
    async fn a_stream_of_known_length_is_checked_against_it() {
        let upload = Upload::single("application/octet-stream", Payload::new(&b"abc"[..], Some(3)));
        let request =
            Request { method: "POST", target: "/1.0/images".into(), headers: Vec::new(), body: Body::Stream(upload) };
        let text = sent(request).await.unwrap();
        assert!(text.contains("Content-Length: 3\r\n") && text.ends_with("\r\n\r\nabc"), "{text}");

        let short = Upload::single("application/octet-stream", Payload::new(&b"ab"[..], Some(3)));
        let request = Request { method: "POST", target: "/".into(), headers: Vec::new(), body: Body::Stream(short) };
        let e = sent(request).await.unwrap_err();
        assert_eq!(e.to_string(), "the payload ended after 2 of its 3 bytes");

        let long = Upload::single("application/octet-stream", Payload::new(&b"abcd"[..], Some(3)));
        let request = Request { method: "POST", target: "/".into(), headers: Vec::new(), body: Body::Stream(long) };
        let e = sent(request).await.unwrap_err();
        assert_eq!(e.to_string(), "the payload holds more than its 3 bytes");
    }

    #[tokio::test]
    async fn a_header_with_a_line_break_is_refused() {
        let headers = vec![("X-Incus-filename".to_string(), "a\r\nX-Evil: 1".to_string())];
        let request = Request { method: "POST", target: "/".into(), headers, body: Body::Empty };
        assert_eq!(sent(request).await.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }
}
