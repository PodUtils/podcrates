//! A stand-in resolver of DNS over HTTPS on the loopback, with the listener
//! of the DNS carrier behind it: it takes each GET of the wire form of RFC
//! 8484 (`?dns=`), over HTTP/2 or HTTP/1.1 by the ALPN that it chose, sends
//! the query over UDP to the listener and gives its answer. It counts its
//! connections, keeps the time of each request, and answers the next
//! requests with `429` when the test says so.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_dns::Doh;
use pod_link::dns::DnsOverHttps;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::time::Instant;

use super::cert::{acceptor_offering, client_config, provider};

/// The path of the wire form.
pub const PATH: &str = "/dns-query";

/// What the stand-in did, and the `429` answers that it gives next.
#[derive(Default)]
pub struct Seen {
    /// The connections that it accepted.
    pub connections: AtomicUsize,
    /// The connections that took HTTP/2.
    pub over_h2: AtomicUsize,
    /// The time of each request, and whether it got a `429`.
    pub requests: Mutex<Vec<(Instant, bool)>>,
    /// The next answers of `429`, each with its `Retry-After` when given.
    pub refuse: Mutex<VecDeque<Option<String>>>,
}

/// A stand-in at a free port of the loopback, before the listener of the
/// carrier at `listener`, that serves the protocols of `alpn` (HTTP/1.1
/// where the client offers none of them).
pub async fn stand_in(listener: SocketAddr, alpn: &[&[u8]]) -> (u16, Arc<Seen>) {
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = tcp.local_addr().unwrap().port();
    let seen = Arc::new(Seen::default());
    let acceptor = acceptor_offering(alpn);
    let state = seen.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = tcp.accept().await {
            state.connections.fetch_add(1, Ordering::SeqCst);
            let (acceptor, state) = (acceptor.clone(), state.clone());
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else { return };
                if tls.get_ref().1.alpn_protocol() == Some(b"h2") {
                    state.over_h2.fetch_add(1, Ordering::SeqCst);
                    serve_h2(tls, listener, state).await;
                } else {
                    serve_one(tls, listener, state).await;
                }
            });
        }
    });
    (port, seen)
}

/// The queries of a dialer to the stand-in at `port`, by the name
/// `localhost` of its certificate, over HTTP/2 alone when `http2`.
pub fn over(port: u16, http2: bool) -> DnsOverHttps {
    let resolver = Doh::new([127, 0, 0, 1].into(), "localhost").port(port).wire(PATH).http2(http2);
    let context = pod_tls::Context::new(provider(), pod_tls::Settings::new(pod_tls::Words::new("pod-link")));
    let mut doh = DnsOverHttps::new(resolver, Arc::new(context));
    doh.settings.trust = pod_tls::Trust::caller(client_config()).unwrap();
    doh
}

/// The status, the headers and the body of the answer to the request of
/// `target`.
async fn answer(target: &str, listener: SocketAddr, seen: &Seen) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let refused = seen.refuse.lock().unwrap().pop_front();
    seen.requests.lock().unwrap().push((Instant::now(), refused.is_some()));
    if let Some(retry) = refused {
        return (429, retry.map(|r| ("retry-after".to_string(), r)).into_iter().collect(), Vec::new());
    }
    let Some(query) = target.split_once("dns=").and_then(|(_, code)| base64url(code)) else {
        return (400, Vec::new(), Vec::new());
    };
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let _ = socket.send_to(&query, listener).await;
    let mut buf = vec![0u8; 1 << 16];
    match tokio::time::timeout(Duration::from_secs(3), socket.recv(&mut buf)).await {
        Ok(Ok(n)) => (200, vec![("content-type".into(), "application/dns-message".into())], buf[..n].to_vec()),
        _ => (504, Vec::new(), Vec::new()),
    }
}

/// The requests of one connection of HTTP/2, each in a task of its own.
async fn serve_h2<S>(tls: S, listener: SocketAddr, seen: Arc<Seen>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let Ok(mut connection) = h2::server::handshake(tls).await else { return };
    while let Some(Ok((request, mut respond))) = connection.accept().await {
        let seen = seen.clone();
        tokio::spawn(async move {
            let target = request.uri().path_and_query().map(|p| p.to_string()).unwrap_or_default();
            let (status, headers, body) = answer(&target, listener, &seen).await;
            let mut response = http::Response::builder().status(status);
            for (name, value) in headers {
                response = response.header(name, value);
            }
            let Ok(mut send) = respond.send_response(response.body(()).unwrap(), false) else { return };
            let _ = send.send_data(bytes::Bytes::from(body), true);
        });
    }
}

/// The requests of one connection of HTTP/1.1, one after the other.
async fn serve_one<S>(mut tls: S, listener: SocketAddr, seen: Arc<Seen>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        head.clear();
        while !head.ends_with(b"\r\n\r\n") {
            match tls.read(&mut byte).await {
                Ok(1) => head.push(byte[0]),
                _ => return,
            }
        }
        let text = String::from_utf8_lossy(&head).into_owned();
        let target = text.split(' ').nth(1).unwrap_or_default().to_string();
        let (status, headers, body) = answer(&target, listener, &seen).await;
        let mut out = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\n", body.len());
        for (name, value) in headers {
            out.push_str(&format!("{name}: {value}\r\n"));
        }
        out.push_str("\r\n");
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(&body);
        if tls.write_all(&bytes).await.is_err() {
            return;
        }
    }
}

/// The bytes of base64url with no padding (RFC 4648, section 5).
fn base64url(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    };
    let (mut out, mut word, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in text.bytes().take_while(|c| *c != b'&') {
        word = (word << 6 | u32::from(value(c)?)) & 0xfff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((word >> bits) as u8);
        }
    }
    Some(out)
}
