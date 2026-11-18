//! One HTTP/1.1 request on plain TCP: the webhooks of the fake GitHub, and
//! the client side of a test of a stand-in. One request on each connection
//! (`Connection: close`), so the answer ends where the server closes.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// The bound of the connect and of each read and write: a server that stops
/// does not hold the caller.
const LIMIT: Duration = Duration::from_secs(20);

/// Send `method` to `url` (`http://HOST:PORT/PATH`) with the extra `headers`
/// and `body`; the status and the body of the answer. The body is the bytes
/// after the head, cut at `Content-Length` when the answer gives one. A
/// chunked body comes as its raw bytes.
pub fn call(method: &str, url: &str, headers: &[(&str, &str)], body: &[u8]) -> io::Result<(u16, Vec<u8>)> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidInput, format!("{url}: {what}"));
    let rest = url.strip_prefix("http://").ok_or_else(|| bad("not an http:// URL"))?;
    let (host, path) = rest.find('/').map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
    let addr = host.to_socket_addrs()?.next().ok_or_else(|| bad("no address"))?;
    let mut tcp = TcpStream::connect_timeout(&addr, LIMIT)?;
    tcp.set_read_timeout(Some(LIMIT))?;
    tcp.set_write_timeout(Some(LIMIT))?;
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    tcp.write_all(&out)?;
    let mut answer = Vec::new();
    tcp.read_to_end(&mut answer)?;
    let end = answer.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| bad("an answer with no head"))?;
    let text = String::from_utf8_lossy(&answer[..end]).into_owned();
    let status = text.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| bad("no status"))?;
    let mut rest = answer.split_off(end + 4);
    let length = text
        .split("\r\n")
        .filter_map(|l| l.split_once(':'))
        .find(|(n, _)| n.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok());
    if let Some(n) = length {
        rest.truncate(n);
    }
    Ok((status, rest))
}
