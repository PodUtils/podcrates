//! The client of podnet's requests: the reading of an answer (its bound of
//! the body, many headers, interim answers, chunks), and one request of any
//! method to an `http://` URL over the caller's dial, with its head, its
//! body, its own way out and its time limit.

mod closed;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Stage};
use pod_proxy::client::{MAX_BODY, MAX_HEAD, Request};
use pod_proxy::http::{Reading, Response, read_answer};
use pod_proxy::resolve::System;
use pod_proxy::{Client, Dialer, ProxyChoice, Settings};
use pod_tls::{Context, Words};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The reading of podnet: a head of [`MAX_HEAD`] at most.
async fn read_response(io: &mut &[u8], head_only: bool, max_body: usize) -> std::io::Result<Response> {
    read_answer(io, Vec::new(), Reading { max_head: MAX_HEAD, max_body, head_only }).await
}

async fn parse(raw: &[u8]) -> std::io::Result<Response> {
    let mut io = raw;
    read_response(&mut io, false, MAX_BODY).await
}

#[tokio::test]
async fn the_body_limit_is_set_for_each_request() {
    let sized = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n0123456789";
    let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\na\r\n0123456789\r\n0\r\n\r\n";
    let to_end = b"HTTP/1.1 200 OK\r\n\r\n0123456789";
    for raw in [&sized[..], &chunked[..], &to_end[..]] {
        let mut io = raw;
        assert!(read_response(&mut io, false, 9).await.is_err());
        let mut io = raw;
        let answer = read_response(&mut io, false, 10).await.unwrap();
        assert_eq!(answer.body, b"0123456789");
    }
    let request = Request::new("GET", "https://example.com/");
    assert_eq!(request.max_body, MAX_BODY);
    assert_eq!(request.max_body(1 << 30).max_body, 1 << 30);
}

/// An answer with more than 64 headers passes.
#[tokio::test]
async fn an_answer_with_many_headers_passes() {
    let mut raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n".to_vec();
    for i in 0..100 {
        raw.extend_from_slice(format!("X-H{i}: v\r\n").as_bytes());
    }
    raw.extend_from_slice(b"\r\nok");
    let r = parse(&raw).await.unwrap();
    assert_eq!((r.headers.len(), r.body.as_slice()), (101, &b"ok"[..]));
}

/// A few interim answers pass; an endless row of them is an error.
#[tokio::test]
async fn interim_answers_have_a_limit() {
    let three = "HTTP/1.1 100 Continue\r\n\r\n".repeat(3) + "HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n";
    assert_eq!(parse(three.as_bytes()).await.unwrap().status, 201);
    let many = "HTTP/1.1 100 Continue\r\n\r\n".repeat(10) + "HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n";
    let err = parse(many.as_bytes()).await.unwrap_err();
    assert!(err.to_string().contains("interim"), "{err:#}");
}

#[tokio::test]
async fn a_chunk_size_that_overflows_is_an_error() {
    for size in ["ffffffffffffffff", "fffffffffffffffe", "fffffffffffffff0"] {
        let raw = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n{size}\r\n");
        let err = parse(raw.as_bytes()).await.unwrap_err();
        assert!(format!("{err:#}").contains("larger than"), "{err:#}");
    }
}

#[tokio::test]
async fn bodies_by_length_by_chunks_and_to_the_end() {
    let r = parse(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhelloEXTRA").await.unwrap();
    assert_eq!((r.status, r.body.as_slice()), (200, &b"hello"[..]));
    let r = parse(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\n\r\n5;x=y\r\nhello\r\n6\r\n world\r\n0\r\n\r\n")
        .await
        .unwrap();
    assert_eq!((r.status, r.text().as_str()), (201, "hello world"));
    let r = parse(b"HTTP/1.0 404 Not Found\r\nX-A: b\r\n\r\nall of it").await.unwrap();
    assert_eq!((r.status, r.text().as_str(), r.header("x-a")), (404, "all of it", Some("b")));
    assert!(parse(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort").await.is_err());
    assert!(parse(b"SSH-2.0-x\r\n\r\n").await.is_err());
}

/// The answer to a `HEAD` has no body, whatever its length says; 101 is an
/// answer, not an interim one.
#[tokio::test]
async fn a_head_has_no_body_and_101_is_the_answer() {
    let mut io = &b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n"[..];
    let r = read_response(&mut io, true, MAX_BODY).await.unwrap();
    assert_eq!((r.status, r.body.len(), r.is_success()), (200, 0, true));
    let r = parse(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n").await.unwrap();
    assert_eq!((r.status, r.is_success()), (101, false));
}

/// A server on the loopback that answers one request with `answer`, and
/// gives the request: its head and its body of `length` bytes.
async fn server(answer: &'static [u8], length: usize) -> (u16, tokio::task::JoinHandle<(String, Vec<u8>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            s.read_exact(&mut b).await.unwrap();
            head.push(b[0]);
        }
        let mut body = vec![0u8; length];
        s.read_exact(&mut body).await.unwrap();
        s.write_all(answer).await.unwrap();
        (String::from_utf8(head).unwrap(), body)
    });
    (port, seen)
}

/// A direct dial that resolves `name.test` to the loopback alone.
fn pinned() -> Dialer {
    let pins = vec![("name.test".into(), IpAddr::from([127, 0, 0, 1]))];
    Dialer::new(Settings::new("podnet", ProxyChoice::Direct), Arc::new(System::with_pins(pins)))
}

/// A client over `dialer`.
fn client(dialer: Dialer) -> Client {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podnet"))));
    Client::new(context, Arc::new(dialer)).with_user_agent("podnet/1.2.3")
}

/// A POST with a JSON body: the head of the request, its body, and a
/// chunked answer.
#[tokio::test]
async fn a_request_sends_its_head_and_its_body() {
    pod_testkit::need_loopback!();
    let body = r#"{"a":1}"#;
    let (port, seen) =
        server(b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n\r\n", body.len()).await;
    let request = Request::new("POST", format!("http://name.test:{port}/tunnel?token=x#f")).json(body);
    let answer = client(pinned()).send(request).await.expect("an answer");
    assert_eq!((answer.status, answer.text().as_str()), (201, "ok"));
    let (head, sent) = seen.await.unwrap();
    assert!(head.starts_with("POST /tunnel?token=x HTTP/1.1\r\n"), "{head}");
    for line in [
        format!("Host: name.test:{port}"),
        "Connection: close".into(),
        "Accept-Encoding: identity".into(),
        "Content-Type: application/json".into(),
        format!("Content-Length: {}", body.len()),
        "User-Agent: podnet/1.2.3".into(),
    ] {
        assert!(head.contains(&format!("\r\n{line}\r\n")), "{line} is not in {head}");
    }
    assert_eq!(sent, body.as_bytes());
}

/// A request's own user agent, and its own way out; a header with a line
/// break is refused before a connection; a failure names the method and
/// the URL with no query.
#[tokio::test]
async fn a_request_takes_its_own_way_out_and_its_own_user_agent() {
    pod_testkit::need_loopback!();
    let (port, seen) = server(b"HTTP/1.1 204 No Content\r\n\r\n", 0).await;
    let url = format!("http://name.test:{port}/x");
    // The client's own way: a proxy where nothing listens.
    let (_held, closed) = closed::port();
    let proxy = pod_proxy::Proxy::parse(&format!("http://{closed}")).unwrap();
    let blocked = || Dialer::new(Settings::new("podnet", ProxyChoice::Via(proxy.clone())), Arc::new(System::new()));
    let failed = client(blocked()).send(Request::new("GET", &url)).await.unwrap_err();
    assert_eq!(failed.stage, Stage::Proxy, "{failed}");
    assert!(failed.text().starts_with(&format!("GET http://name.test:{port}/x: ")), "{failed}");
    let request = Request::new("GET", &url).header("User-Agent", "other/1").via(Some(Arc::new(pinned())));
    assert_eq!(client(blocked()).send(request).await.expect("an answer").status, 204);
    let (head, _) = seen.await.unwrap();
    assert!(head.contains("\r\nUser-Agent: other/1\r\n") && !head.contains("podnet/1.2.3"), "{head}");
    let bad = Request::new("GET", &url).header("X-A", "b\r\nX-B: c");
    let refused = client(pinned()).send(bad).await.unwrap_err();
    assert_eq!(refused.class, Class::Usage, "{refused}");
}

/// A server that does not answer ends the request at its time limit, with
/// the method and the URL with no query.
#[tokio::test]
async fn a_silent_server_ends_at_the_time_limit() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let _hold = tokio::spawn(async move {
        let held = listener.accept().await;
        tokio::time::sleep(Duration::from_secs(30)).await;
        drop(held);
    });
    let request =
        Request::new("GET", format!("http://name.test:{port}/x?secret=1")).timeout(Duration::from_millis(300));
    let failed = client(pinned()).send(request).await.unwrap_err();
    assert_eq!(failed.class, Class::Timeout);
    assert_eq!(failed.text(), format!("GET http://name.test:{port}/x: no answer in 0 s"));
}

/// A server that ends the connection in the TLS handshake: the failure of
/// the request is the network's, not TLS's.
#[tokio::test]
async fn a_close_in_the_tls_handshake_is_the_network() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut hello = [0u8; 5];
        let _ = s.read_exact(&mut hello).await;
        drop(s);
    });
    let failed = client(pinned()).send(Request::new("GET", format!("https://name.test:{port}/"))).await.unwrap_err();
    assert_eq!(failed.stage, Stage::Network, "{failed}");
}
