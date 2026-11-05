//! The bounds of a connection, each against a peer that does not stop at
//! it: what is read, and how the read ends. Each peer is in this process: an
//! in-memory stream, or a listener on the loopback for the proxy.

use std::sync::Arc;
use std::time::Duration;

use pod_proxy::http;
use pod_proxy::resolve::System;
use pod_proxy::{DialError, Dialer, HttpProxy, ProxyChoice, Settings};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// How long each read may take.
const LIMIT: Duration = Duration::from_secs(30);
/// The bound of a response head: of the upgrade, of HTTP, of the proxy.
const HEAD: usize = 16 * 1024;
/// What a peer offers past a bound: far more than any bound here.
const ENDLESS: usize = 1 << 20;

/// The dial of podssh, as a dialer of its settings gives it.
async fn dial(host: &str, port: u16, proxy: &ProxyChoice, timeout: Duration) -> Result<TcpStream, DialError> {
    Dialer::new(Settings::new("podssh", proxy.clone()), Arc::new(System::new())).tcp(host, port, timeout).await
}

/// Write `start`, then header lines and never the blank line, until the
/// reader goes or [`ENDLESS`] bytes went: the bytes written.
async fn endless_head<S: AsyncWrite + Unpin>(peer: &mut S, start: &[u8]) -> usize {
    let line = format!("X-Pad: {}\r\n", "a".repeat(100));
    let mut sent = 0;
    if peer.write_all(start).await.is_err() {
        return sent;
    }
    sent += start.len();
    while sent < ENDLESS {
        if peer.write_all(line.as_bytes()).await.is_err() {
            break;
        }
        sent += line.len();
    }
    sent
}

/// Read the request's head, to its blank line.
async fn request_head<S: AsyncRead + Unpin>(peer: &mut S) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match peer.read(&mut byte).await {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
}

#[tokio::test]
async fn an_http_head_that_never_ends_stops_at_16_kib() {
    let (mut client, mut peer) = tokio::io::duplex(4096);
    let server = tokio::spawn(async move { endless_head(&mut peer, b"HTTP/1.1 200 OK\r\n").await });
    let read = http::read_response(&mut client, Vec::new(), 1024);
    let err = tokio::time::timeout(LIMIT, read).await.expect("the read ends").unwrap_err();
    assert!(err.to_string().contains("no end of the response headers within 16 KiB"), "{err}");
    drop(client);
    let sent = server.await.unwrap();
    assert!(sent < HEAD + 8 * 1024, "the server wrote {sent} bytes before podssh stopped reading");
}

/// A body of `max` bytes is read and one of `max + 1` is refused, in each of
/// the three framings of HTTP/1.1: a length, chunks, and the close.
#[tokio::test]
async fn a_body_past_its_bound_is_refused_in_each_framing() {
    const MAX: usize = 1000;
    let length = |n: usize| format!("HTTP/1.1 200 OK\r\nContent-Length: {n}\r\n\r\n{}", "b".repeat(n));
    let chunked = |n: usize| {
        let half = n / 2;
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{half:x}\r\n{}\r\n{:x}\r\n{}\r\n0\r\n\r\n",
            "b".repeat(half),
            n - half,
            "b".repeat(n - half)
        )
    };
    let until_close = |n: usize| format!("HTTP/1.1 200 OK\r\n\r\n{}", "b".repeat(n));
    let framings: [(&str, &dyn Fn(usize) -> String); 3] =
        [("a length", &length), ("chunks", &chunked), ("the close", &until_close)];
    for (framing, response) in framings {
        for (n, fits) in [(MAX, true), (MAX + 1, false)] {
            let (mut client, mut peer) = tokio::io::duplex(64 * 1024);
            let text = response(n);
            let server = tokio::spawn(async move {
                let _ = peer.write_all(text.as_bytes()).await;
                // The close ends the third framing.
                drop(peer);
            });
            let read = tokio::time::timeout(LIMIT, http::read_response(&mut client, Vec::new(), MAX)).await;
            server.await.unwrap();
            match (read.expect("the read ends"), fits) {
                (Ok(r), true) => assert_eq!(r.body.len(), MAX, "{framing}"),
                (Err(e), false) => assert!(e.to_string().contains("larger than allowed"), "{framing}: {e}"),
                (Ok(r), false) => panic!("{framing}: a body of {} bytes passed a bound of {MAX}", r.body.len()),
                (Err(e), true) => panic!("{framing}: a body of {MAX} bytes was refused: {e}"),
            }
        }
    }
}

#[tokio::test]
async fn a_proxy_answer_that_never_ends_stops_at_16_kib() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let proxy = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        request_head(&mut sock).await;
        endless_head(&mut sock, b"HTTP/1.1 200 Connection established\r\n").await
    });
    let via = HttpProxy::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    let err = dial("relay.invalid", 443, &ProxyChoice::Via(via), LIMIT).await.unwrap_err();
    assert!(matches!(err, DialError::ProxyProtocol { .. }), "{err:?}");
    assert!(err.to_string().contains("no end of headers within 16384 bytes"), "{err}");
    proxy.abort();
}
