//! A plain request (`GET http://HOST/PATH`): one request on one connection.
//! The broker sends the checked path in origin form, its own `Host`, no
//! hop-by-hop header and no `Proxy-Authorization`, and `Connection: close`,
//! then the body in the framing of the request, then the answer back as it
//! comes. A second request on the connection is never read: it would pass
//! no check.

use pod_core::BoxedIo;
use pod_proxy::http::{Body, Framing, Request};
use tokio::io::AsyncWriteExt;

use crate::target::HttpTarget;
use crate::tunnel::{Counts, pipe};

/// The headers that end at the broker (RFC 9110 section 7.6.1), and the
/// ones that the broker writes itself.
const DROPPED: &[&str] =
    &["connection", "proxy-connection", "keep-alive", "proxy-authorization", "te", "trailer", "upgrade", "host"];

/// The framing of a request's body, or why it is ambiguous: both lengths,
/// a coding other than `chunked`, or two different lengths are refused, as
/// two readers could read two bodies.
pub fn request_framing(request: &Request) -> Result<Framing, String> {
    let values = |name: &str| -> Vec<&str> {
        request.headers.iter().filter(|(n, _)| n == name).map(|(_, v)| v.as_str()).collect()
    };
    let codings = values("transfer-encoding");
    let lengths = values("content-length");
    if !codings.is_empty() {
        if !lengths.is_empty() {
            return Err("a request with Transfer-Encoding and Content-Length is ambiguous".into());
        }
        return match codings[..] {
            [one] if one.eq_ignore_ascii_case("chunked") => Ok(Framing::Chunked),
            _ => Err("a request body is chunked, or has a length".into()),
        };
    }
    let Some(first) = lengths.first() else { return Ok(Framing::Length(0)) };
    let digits = |v: &str| !v.is_empty() && v.len() <= 19 && v.bytes().all(|b| b.is_ascii_digit());
    if !lengths.iter().all(|v| v == first && digits(v)) {
        return Err("the Content-Length of a request is not one number".into());
    }
    first.parse().map(Framing::Length).map_err(|_| "the Content-Length is too large".into())
}

/// The head that goes to the far end.
pub fn head(request: &Request, target: &HttpTarget) -> String {
    let named: Vec<String> = request
        .headers
        .iter()
        .filter(|(n, _)| n == "connection")
        .flat_map(|(_, v)| v.split(',').map(|t| t.trim().to_ascii_lowercase()))
        .collect();
    let host = match target.authority.port {
        80 => target.authority.host.uri_text(),
        port => format!("{}:{port}", target.authority.host.uri_text()),
    };
    let mut out = format!("{} {} HTTP/1.1\r\nHost: {host}\r\n", request.method, target.origin_form());
    for (name, value) in &request.headers {
        if DROPPED.contains(&name.as_str()) || named.contains(name) {
            continue;
        }
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("Connection: close\r\n\r\n");
    out
}

/// Send `head` and the body of `framing` from `client`, and the answer of
/// `upstream` back, each byte counted in `counts`.
pub async fn exchange(
    client: BoxedIo,
    upstream: BoxedIo,
    head: String,
    framing: Framing,
    counts: &Counts,
) -> std::io::Result<()> {
    let (client_read, client_write) = tokio::io::split(client);
    let (upstream_read, mut upstream_write) = tokio::io::split(upstream);
    let send = async {
        upstream_write.write_all(head.as_bytes()).await?;
        counts.up.fetch_add(head.len() as u64, std::sync::atomic::Ordering::Relaxed);
        let chunked = framing == Framing::Chunked;
        let mut body = Body::new(client_read, Vec::new(), framing, None)?;
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = tokio::io::AsyncReadExt::read(&mut body, &mut buf).await?;
            if n == 0 {
                break;
            }
            let mut out = Vec::with_capacity(n + 16);
            if chunked {
                out.extend_from_slice(format!("{n:x}\r\n").as_bytes());
            }
            out.extend_from_slice(&buf[..n]);
            if chunked {
                out.extend_from_slice(b"\r\n");
            }
            upstream_write.write_all(&out).await?;
            counts.up.fetch_add(out.len() as u64, std::sync::atomic::Ordering::Relaxed);
        }
        if chunked {
            upstream_write.write_all(b"0\r\n\r\n").await?;
            counts.up.fetch_add(5, std::sync::atomic::Ordering::Relaxed);
        }
        upstream_write.flush().await
    };
    let receive = pipe(upstream_read, client_write, &counts.down);
    let (sent, received) = tokio::join!(send, receive);
    sent.and(received)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(text: &str) -> Request {
        pod_proxy::http::request::parse_request(text.as_bytes()).unwrap()
    }

    #[test]
    fn a_body_has_one_framing() {
        let framing = |text: &str| request_framing(&request(text));
        assert_eq!(framing("POST http://a/ HTTP/1.1\r\nContent-Length: 5\r\n\r\n"), Ok(Framing::Length(5)));
        assert_eq!(framing("POST http://a/ HTTP/1.1\r\nTransfer-Encoding: Chunked\r\n\r\n"), Ok(Framing::Chunked));
        assert_eq!(framing("GET http://a/ HTTP/1.1\r\n\r\n"), Ok(Framing::Length(0)));
        for bad in [
            "POST http://a/ HTTP/1.1\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n",
            "POST http://a/ HTTP/1.1\r\nContent-Length: 5\r\nContent-Length: 6\r\n\r\n",
            "POST http://a/ HTTP/1.1\r\nContent-Length: -5\r\n\r\n",
            "POST http://a/ HTTP/1.1\r\nTransfer-Encoding: gzip, chunked\r\n\r\n",
        ] {
            assert!(framing(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_head_drops_the_hop_headers_and_the_credentials_of_the_proxy() {
        let req = request(
            "GET http://a.example:8080/x?y HTTP/1.1\r\nHost: a.example:8080\r\nProxy-Authorization: Basic eDp5\r\n\
             Connection: keep-alive, X-Drop\r\nX-Drop: 1\r\nAccept: */*\r\n\r\n",
        );
        let target = crate::target::parse_absolute(&req.target).unwrap();
        assert_eq!(
            head(&req, &target),
            "GET /x?y HTTP/1.1\r\nHost: a.example:8080\r\naccept: */*\r\nConnection: close\r\n\r\n"
        );
    }
}
