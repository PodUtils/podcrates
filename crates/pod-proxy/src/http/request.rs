//! The head of an HTTP/1 request, as a server of a consumer reads it: the
//! CONNECT of podssh's `pipe http-listen:`, and the upgrade of its
//! `ws-listen:`. The request line and its headers within 16 KiB, as a
//! response's head is read. The parse, the bounds and the answers are
//! `pod_core::http1`'s; this module reads the bytes of a stream.

use tokio::io::{AsyncRead, AsyncReadExt};

pub use pod_core::http1::{
    Bounds, MAX_HEAD, Request, RequestError, answer, established, parse_request, reason, refusal,
};

/// Read a request's head from `stream`, whose first bytes may be in
/// `buffered`: the request, and the bytes that came after its head, which a
/// client of a tunnel may send before the answer, and which belong to it.
pub async fn read_request<S>(stream: &mut S, buffered: Vec<u8>) -> Result<(Request, Vec<u8>), RequestError>
where
    S: AsyncRead + Unpin,
{
    read_request_within(stream, buffered, Bounds::DEFAULT).await
}

/// [`read_request`] within `bounds`: a head of more bytes or more lines is
/// [`RequestError::TooLarge`].
pub async fn read_request_within<S>(
    stream: &mut S,
    mut buffered: Vec<u8>,
    bounds: Bounds,
) -> Result<(Request, Vec<u8>), RequestError>
where
    S: AsyncRead + Unpin,
{
    let mut chunk = [0u8; 4096];
    let (start, end) = loop {
        if let Some(found) = pod_core::http1::find_request(&buffered, bounds)? {
            break found;
        }
        let n = stream.read(&mut chunk).await.map_err(|e| RequestError::Gone(e.to_string()))?;
        if n == 0 {
            return Err(RequestError::Gone("the connection closed".into()));
        }
        buffered.extend_from_slice(&chunk[..n]);
    };
    let rest = buffered.split_off(end);
    Ok((parse_request(&buffered[start..])?, rest))
}

/// The target of `CONNECT HOST:PORT`, the authority form of RFC 9110 section
/// 9.3.6: a name or an IPv4 address, or an IPv6 address in brackets, and a
/// port from 1, the host checked as each target of a road is.
pub fn connect_target(target: &str) -> Result<(String, u16), String> {
    let (host, port) = match target.strip_prefix('[') {
        Some(rest) => {
            let (host, after) = rest.split_once(']').ok_or_else(|| format!("{target:?}: '[' is not closed"))?;
            (host, after.strip_prefix(':').ok_or_else(|| format!("{target:?} names no port"))?)
        }
        None => match target.split_once(':') {
            Some((host, port)) if !port.contains(':') => (host, port),
            Some(_) => return Err(format!("{target:?}: an IPv6 address goes in brackets, [ADDR]:PORT")),
            None => return Err(format!("{target:?} names no port: CONNECT takes HOST:PORT")),
        },
    };
    let port = port.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(|| format!("{target:?}: not a port"))?;
    if target.starts_with('[') && host.parse::<std::net::Ipv6Addr>().is_err() {
        return Err(format!("{target:?}: brackets hold an IPv6 address"));
    }
    pod_core::names::check_target(host)?;
    Ok((host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pod_core::http1::head_bounds;

    fn parse(text: &str) -> Result<Request, RequestError> {
        parse_request(text.as_bytes())
    }

    /// The CONNECT that curl 8 sends through `-x`, as `curl -v` shows it.
    #[test]
    fn a_connect_of_curl_reads_with_its_target_and_headers() {
        let head = "CONNECT example.org:443 HTTP/1.1\r\nHost: example.org:443\r\nUser-Agent: curl/8.11.1\r\n\
                    Proxy-Connection: Keep-Alive\r\n\r\n";
        let request = parse(head).unwrap();
        assert_eq!((request.method.as_str(), request.target.as_str()), ("CONNECT", "example.org:443"));
        assert_eq!(request.minor, 1);
        assert_eq!(request.header("HOST"), Some("example.org:443"));
        assert_eq!(request.header("user-agent"), Some("curl/8.11.1"));
        assert!(request.has("proxy-connection") && !request.has("proxy-authorization"));
        assert_eq!(connect_target(&request.target), Ok(("example.org".to_string(), 443)));
    }

    #[test]
    fn a_line_may_end_with_lf_alone_and_empty_lines_may_come_first() {
        let request = parse("CONNECT [2001:db8::1]:22 HTTP/1.0\nHost: x\n\n").unwrap();
        assert_eq!((request.minor, request.header("host")), (0, Some("x")));
        assert_eq!(head_bounds(b"\r\n\nGET / HTTP/1.1\r\n\r\nrest"), Some((3, 21)));
        assert_eq!(head_bounds(b"GET / HTTP/1.1\nHost: x\n\nrest"), Some((0, 24)));
        assert_eq!(head_bounds(b"GET / HTTP/1.1\r\nHost: x\r\n"), None, "the head has not ended");
        assert_eq!(connect_target("[2001:db8::1]:22"), Ok(("2001:db8::1".to_string(), 22)));
    }

    #[test]
    fn a_connect_target_is_host_and_port_and_checked_as_each_target() {
        for bad in ["example.org", "example.org:0", "example.org:x", "2001:db8::1:22", "[x]:22", "[::1", "a/b:22"] {
            assert!(connect_target(bad).is_err(), "{bad}");
        }
        assert!(connect_target("a b:22").is_err() && connect_target(":22").is_err());
        assert_eq!(connect_target("127.0.0.1:8080"), Ok(("127.0.0.1".to_string(), 8080)));
    }

    /// The bytes after the head are the tunnel's, kept for it; a client that
    /// leaves first has nobody to answer, and a head over 16 KiB gets 431.
    #[tokio::test]
    async fn the_bytes_after_the_head_are_kept_and_a_long_head_is_431() {
        let (mut ours, mut theirs) = tokio::io::duplex(64 * 1024);
        let sent = b"CONNECT a.test:443 HTTP/1.1\r\nHost: a.test:443\r\n\r\n\x16\x03\x01".to_vec();
        tokio::io::AsyncWriteExt::write_all(&mut theirs, &sent).await.unwrap();
        let (request, rest) = read_request(&mut ours, Vec::new()).await.unwrap();
        assert_eq!((request.target.as_str(), rest.as_slice()), ("a.test:443", &b"\x16\x03\x01"[..]));
        drop(theirs);
        let gone = read_request(&mut ours, Vec::new()).await.unwrap_err();
        assert_eq!(gone.status(), None, "{gone}");
        let long = format!("GET / HTTP/1.1\r\nX: {}\r\n", "y".repeat(MAX_HEAD));
        let (mut ours, mut theirs) = tokio::io::duplex(64 * 1024);
        tokio::io::AsyncWriteExt::write_all(&mut theirs, long.as_bytes()).await.unwrap();
        assert_eq!(read_request(&mut ours, Vec::new()).await.unwrap_err(), RequestError::TooLarge);
    }
}
