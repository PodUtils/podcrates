//! The bytes on the wire of the stand-ins: the head of an HTTP message,
//! the server name of a TLS ClientHello, and a TCP reset.

use std::io::Read;
use std::net::TcpStream;
use std::os::fd::AsRawFd;

/// The longest head of an HTTP message that a stand-in reads.
const HEAD_MAX: usize = 16 * 1024;

/// Read the head of an HTTP message, to the empty line, one byte at a time
/// (`pod_core::http1::read_head_exact`). The bytes after it stay in the
/// stream only where the peer waits for an answer, as each peer of the lab
/// does.
pub fn read_head(s: &mut impl Read) -> Result<String, String> {
    let head = pod_core::http1::read_head_exact(s, HEAD_MAX).map_err(|e| format!("a read of the head: {e}"))?;
    String::from_utf8(head).map_err(|_| "the head is not text".to_string())
}

/// The first line of a head, in its three words.
pub fn first_line(head: &str) -> (&str, &str, &str) {
    let line = head.lines().next().unwrap_or("");
    let mut w = line.splitn(3, ' ');
    (w.next().unwrap_or(""), w.next().unwrap_or(""), w.next().unwrap_or(""))
}

/// The value of the header `name` (with no case) in a head.
pub fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// The status of an answer of HTTP/1.1: `HTTP/1.1 200 OK` gives 200.
pub fn status(head: &str) -> Option<u16> {
    pod_core::http1::parse_status_line(head).ok().map(|(status, _)| status)
}

/// The host and the port of the target of `CONNECT`: `name:443` or
/// `[::1]:443`.
pub fn target(text: &str) -> Option<(String, u16)> {
    let (host, port) = text.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    Some((host.to_ascii_lowercase(), port.parse().ok()?))
}

/// The TLS records of a ClientHello: the content type and the type of the
/// first handshake message.
const HANDSHAKE: u8 = 0x16;
const CLIENT_HELLO: u8 = 1;
const SERVER_NAME: u16 = 0;

/// Wait for the whole first record of `s` with `peek`, and give its
/// bytes. `None` where the first byte is not a TLS handshake record.
pub fn peek_record(s: &TcpStream) -> Result<Option<Vec<u8>>, String> {
    let mut buf = vec![0u8; 5 + (1 << 14)];
    let mut want = 1;
    // A peer that sends a part of a record and then waits gets no answer.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if std::time::Instant::now() > end {
            return Err("the first record did not come whole".to_string());
        }
        let n = s.peek(&mut buf).map_err(|e| format!("a peek of the first record: {e}"))?;
        if n == 0 {
            return Err("the peer closed before the first record".to_string());
        }
        if buf[0] != HANDSHAKE {
            return Ok(None);
        }
        if n >= 5 {
            want = 5 + usize::from(u16::from_be_bytes([buf[3], buf[4]]));
        }
        if n >= want.max(5) {
            buf.truncate(want);
            return Ok(Some(buf));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// The server name of a ClientHello record, or `None` where it has no
/// such extension. `Err` where the record is not a ClientHello.
pub fn server_name(record: &[u8]) -> Result<Option<String>, String> {
    let bad = || "not a ClientHello".to_string();
    let body = record.get(5..).ok_or_else(bad)?;
    if body.first() != Some(&CLIENT_HELLO) {
        return Err(bad());
    }
    // The type (1), the length (3), the version (2) and the random (32).
    let mut at = 1 + 3 + 2 + 32;
    let byte = |at: usize| body.get(at).copied().map(usize::from).ok_or_else(bad);
    let pair = |at: usize| Ok::<usize, String>(byte(at)? << 8 | byte(at + 1)?);
    at += 1 + byte(at)?; // the session id
    at += 2 + pair(at)?; // the cipher suites
    at += 1 + byte(at)?; // the compression methods
    if at >= body.len() {
        return Ok(None);
    }
    let end = (at + 2 + pair(at)?).min(body.len());
    at += 2;
    while at + 4 <= end {
        let (kind, len) = (pair(at)? as u16, pair(at + 2)?);
        at += 4;
        if kind == SERVER_NAME {
            // The list (2), the type of the name (1), its length (2).
            let n = pair(at + 3)?;
            let name = body.get(at + 5..at + 5 + n).ok_or_else(bad)?;
            return Ok(Some(String::from_utf8_lossy(name).to_ascii_lowercase()));
        }
        at += len;
    }
    Ok(None)
}

/// Close `s` with a TCP reset: `SO_LINGER` with a time of 0.
pub fn reset(s: TcpStream) {
    const SOL_SOCKET: u64 = 1;
    const SO_LINGER: u64 = 13;
    let linger: [i32; 2] = [1, 0];
    // SAFETY: the pointer is to a live array of the size given.
    let _ = unsafe {
        pod_hostprobe::sys::sys(
            linux_raw_sys::general::__NR_setsockopt as i64,
            [s.as_raw_fd() as u64, SOL_SOCKET, SO_LINGER, linger.as_ptr() as u64, 8, 0],
        )
    };
    drop(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_gives_its_line_its_headers_and_its_target() {
        let head = "CONNECT Listed.TEST:443 HTTP/1.1\r\nProxy-Authorization: Basic eA==\r\n\r\n";
        assert_eq!(read_head(&mut head.as_bytes()).unwrap(), head);
        assert_eq!(first_line(head), ("CONNECT", "Listed.TEST:443", "HTTP/1.1"));
        assert_eq!(header(head, "proxy-authorization"), Some("Basic eA=="));
        assert_eq!(target("Listed.TEST:443"), Some(("listed.test".to_string(), 443)));
        assert_eq!(target("[::1]:22"), Some(("::1".to_string(), 22)));
        assert_eq!(status("HTTP/1.1 504 Gateway Timeout\r\n\r\n"), Some(504));
        assert!(read_head(&mut &b"GET / HTTP/1.1\r\n"[..]).is_err());
    }

    /// A ClientHello of rustls with a name and with none.
    #[test]
    fn the_server_name_of_a_client_hello() {
        for (name, want) in [(Some("listed.test"), Some("listed.test")), (None, None)] {
            let provider = std::sync::Arc::new(pod_tls::crypto::provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth();
            let mut config = config;
            config.enable_sni = name.is_some();
            let server = rustls_pki_types::ServerName::try_from(name.unwrap_or("192.0.2.1")).unwrap();
            let mut c = rustls::ClientConnection::new(std::sync::Arc::new(config), server.to_owned()).unwrap();
            let mut record = Vec::new();
            c.write_tls(&mut record).unwrap();
            assert_eq!(server_name(&record).unwrap().as_deref(), want);
        }
        assert!(server_name(&[0x16, 3, 1, 0, 1, 2]).is_err());
    }
}
