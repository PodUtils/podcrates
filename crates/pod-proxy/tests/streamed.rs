//! The streamed answer of the async client: a body larger than the bound of
//! a buffered one, by length and by chunks, read as it comes; a body cut
//! before its length; a peer that stops sending; a `HEAD`.

mod served;

use std::sync::Arc;
use std::time::{Duration, Instant};

use pod_proxy::client::{Request, Streamed};
use pod_proxy::{Client, Dialer, ProxyChoice, Settings};
use served::{Answer, LARGE, NAME, Served, body, pins, serve, sha256};
use tokio::io::AsyncReadExt;

const LIMIT: Duration = Duration::from_secs(10);

fn client(served: &Served) -> Client {
    let dialer = Dialer::new(Settings::new("podvm", ProxyChoice::Direct), Arc::new(pins()));
    Client::new(served.context.clone(), Arc::new(dialer)).with_user_agent("podvm/1.2.3")
}

fn request(served: &Served, method: &'static str) -> Request {
    Request::new(method, format!("https://{NAME}:{}/v2/blobs/x", served.port)).trust(served.trust.clone())
}

/// Read each byte of `streamed` in reads of an odd size.
async fn read_all(streamed: &mut Streamed) -> std::io::Result<Vec<u8>> {
    let mut got = Vec::new();
    let mut chunk = vec![0u8; 7919];
    loop {
        let n = streamed.read(&mut chunk).await?;
        if n == 0 {
            return Ok(got);
        }
        got.extend_from_slice(&chunk[..n]);
    }
}

#[tokio::test]
async fn a_large_body_streams_by_length_and_by_chunks() {
    pod_testkit::need_loopback!();
    let sent = body(LARGE);
    for answer in [Answer::Length, Answer::Chunked] {
        let served = serve(answer, sent.clone());
        let mut streamed = client(&served).send_streamed(request(&served, "GET")).await.expect("an answer");
        assert_eq!((streamed.status, streamed.is_success()), (200, true), "{answer:?}");
        let got = read_all(&mut streamed).await.expect("the body");
        assert_eq!(got.len(), LARGE, "{answer:?}");
        assert_eq!(sha256(&got), sha256(&sent), "{answer:?}");
        let head = served.heads.recv().expect("the request");
        assert!(head.starts_with("GET /v2/blobs/x HTTP/1.1\r\n"), "{head}");
    }
}

/// The buffered `send` keeps its bound over the same answer.
#[tokio::test]
async fn the_buffered_send_keeps_its_bound() {
    pod_testkit::need_loopback!();
    let served = serve(Answer::Length, body(LARGE));
    let failed = client(&served).send(request(&served, "GET")).await.unwrap_err();
    assert!(failed.text().ends_with("reading the answer: the response body is larger than allowed"), "{failed}");
}

#[tokio::test]
async fn a_body_cut_before_its_length_is_an_error() {
    pod_testkit::need_loopback!();
    let served = serve(Answer::Cut, body(1 << 20));
    let mut streamed = client(&served).send_streamed(request(&served, "GET")).await.expect("an answer");
    assert_eq!(streamed.header("Content-Length"), Some("1048576"));
    let cut = read_all(&mut streamed).await.unwrap_err();
    assert_eq!(cut.kind(), std::io::ErrorKind::UnexpectedEof, "{cut}");
}

/// A peer that stops in the middle of the body ends the read at the idle
/// limit, not at the time limit of the request.
#[tokio::test]
async fn a_body_that_stops_ends_at_the_idle_limit() {
    pod_testkit::need_loopback!();
    let served = serve(Answer::Stall, body(1 << 20));
    let request = request(&served, "GET").timeout(LIMIT).body_idle(Duration::from_millis(300));
    let mut streamed = client(&served).send_streamed(request).await.expect("an answer");
    let start = Instant::now();
    let stopped = read_all(&mut streamed).await.unwrap_err();
    assert_eq!(stopped.kind(), std::io::ErrorKind::TimedOut, "{stopped}");
    assert!(start.elapsed() < Duration::from_secs(5), "the read waited {:?}", start.elapsed());
}

/// The answer to a `HEAD` has no body, whatever its length says.
#[tokio::test]
async fn a_head_has_no_body() {
    pod_testkit::need_loopback!();
    let served = serve(Answer::HeadOnly, body(1 << 20));
    let mut streamed = client(&served).send_streamed(request(&served, "HEAD")).await.expect("an answer");
    let got = tokio::time::timeout(LIMIT, read_all(&mut streamed)).await.expect("no wait").expect("no body");
    assert!(got.is_empty());
}
