//! The capture sink of the HTTP exchanges (`crate::capture`) with a
//! stand-in origin.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

use super::copy::copy_tokio_to_futures_eof;
use super::*;
use crate::capture::{Capture, Direction, End, Head, Line, Refused, Tap};
use crate::cloudflare::stream::{ConnectionType, HTTP_STATUS_KEY};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Seen {
    Head(u64, Head),
    Body(u64, Direction, Vec<u8>),
    End(u64, End),
}

/// A sink that keeps each call. With `refuse_body`, it refuses the first
/// body part.
#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<Seen>>,
    refuse_body: bool,
}

impl Capture for Recorder {
    fn head(&self, exchange: u64, head: &Head) -> Result<(), Refused> {
        self.seen.lock().unwrap().push(Seen::Head(exchange, head.clone()));
        Ok(())
    }
    fn body(&self, exchange: u64, direction: Direction, bytes: &[u8]) -> Result<(), Refused> {
        if self.refuse_body {
            return Err(Refused);
        }
        self.seen.lock().unwrap().push(Seen::Body(exchange, direction, bytes.to_vec()));
        Ok(())
    }
    fn end(&self, exchange: u64, end: &End) {
        self.seen.lock().unwrap().push(Seen::End(exchange, end.clone()));
    }
}

impl Recorder {
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

fn heads(seen: &[Seen]) -> Vec<Head> {
    seen.iter().filter_map(|s| if let Seen::Head(_, h) = s { Some(h.clone()) } else { None }).collect()
}

fn body(seen: &[Seen], direction: Direction) -> Vec<u8> {
    seen.iter()
        .filter_map(|s| if let Seen::Body(_, d, b) = s { (*d == direction).then_some(b.clone()) } else { None })
        .flatten()
        .collect()
}

fn ends(seen: &[Seen]) -> Vec<End> {
    seen.iter().filter_map(|s| if let Seen::End(_, e) = s { Some(e.clone()) } else { None }).collect()
}

/// A stand-in origin. `/plain` answers with a length, `/chunked` with
/// chunks, `/upload` reads a chunked body and answers with chunks, and
/// `/ws` answers 101 and sends back each byte.
pub(super) async fn origin() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut got = Vec::new();
                let mut buf = [0u8; 4096];
                while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    got.extend_from_slice(&buf[..n]);
                }
                let text = String::from_utf8_lossy(&got).to_string();
                let path = text.split(' ').nth(1).unwrap_or_default().to_string();
                let answer: &[u8] = match path.as_str() {
                    "/plain" => {
                        let mut want = text.find("\r\n\r\n").unwrap() + 4 + 4;
                        while got.len() < want {
                            let n = s.read(&mut buf).await.unwrap_or(0);
                            if n == 0 {
                                want = 0;
                            }
                            got.extend_from_slice(&buf[..n]);
                        }
                        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nSet-Cookie: s=1\r\n\r\nhello"
                    }
                    "/chunked" => {
                        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n4\r\ndefg\r\n0\r\n\r\n"
                    }
                    "/upload" => {
                        while !got.ends_with(b"0\r\n\r\n") {
                            let n = s.read(&mut buf).await.unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            got.extend_from_slice(&buf[..n]);
                        }
                        b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n\r\n"
                    }
                    "/ws" => {
                        let _ = s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n").await;
                        let start = text.find("\r\n\r\n").unwrap() + 4;
                        let _ = s.write_all(&got[start..]).await;
                        let (mut r, mut w) = s.split();
                        let _ = tokio::io::copy(&mut r, &mut w).await;
                        let _ = w.shutdown().await;
                        return;
                    }
                    _ => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
                };
                let _ = s.write_all(answer).await;
            });
        }
    });
    at
}

pub(super) fn request(path: &str, extra: &[(&str, &str)], websocket: bool) -> cqstream::ConnectRequest {
    let mut metadata = vec![(HTTP_METHOD_KEY.to_string(), "POST".to_string()), (HTTP_HOST_KEY.into(), "x".into())];
    for (name, value) in extra {
        metadata.push((format!("{HTTP_HEADER_KEY}:{name}"), value.to_string()));
    }
    let conn_type = if websocket { ConnectionType::Websocket } else { ConnectionType::Http };
    cqstream::ConnectRequest { dest: format!("https://x{path}"), conn_type, metadata }
}

/// Serve `request` with the visitor body `sent`, and give the status and
/// the body that the visitor got.
pub(super) async fn visit(
    runtime: &Runtime,
    ingress: &Ingress,
    request: cqstream::ConnectRequest,
    sent: &[u8],
) -> (String, Vec<u8>) {
    let (ours, theirs) = tokio::io::duplex(1 << 16);
    let (mut visitor, input) = tokio::io::duplex(1 << 16);
    visitor.write_all(sent).await.unwrap();
    drop(visitor);
    let serve = serve_request_inner(runtime, ingress, request, Box::pin(input.compat()), Box::pin(ours.compat_write()));
    let read = async {
        let mut reader = theirs.compat();
        let (_, meta) = cqstream::read_connect_response(&mut reader).await.unwrap();
        let status = meta.iter().find(|(k, _)| k == HTTP_STATUS_KEY).map(|(_, v)| v.clone()).unwrap_or_default();
        let mut body = Vec::new();
        futures::io::AsyncReadExt::read_to_end(&mut reader, &mut body).await.unwrap();
        (status, body)
    };
    let (served, got) = tokio::time::timeout(std::time::Duration::from_secs(10), async { tokio::join!(serve, read) })
        .await
        .expect("the exchange ends");
    served.unwrap();
    got
}

pub(super) fn runtime(ingress: &Ingress, sink: Option<Arc<dyn Capture>>) -> Runtime {
    let mut host = crate::testing::host();
    host.capture = sink;
    Runtime::build(Arc::new(host), ingress, Metrics::new()).unwrap()
}

type Case = (&'static str, Vec<(&'static str, &'static str)>, bool, &'static [u8]);

fn cases() -> Vec<Case> {
    vec![
        ("/plain", vec![("Content-Length", "4"), ("Cookie", "v=1")], false, b"ping"),
        ("/chunked", vec![], false, b""),
        ("/upload", vec![("Transfer-Encoding", "chunked")], false, b"2\r\nup\r\n3\r\nend\r\n0\r\n\r\n"),
        ("/ws", vec![("Upgrade", "websocket")], true, b"frames"),
    ]
}

/// The sink sees each head and body of a plain, a chunked and a WebSocket
/// exchange, the payload of each chunked body with no size lines, and one
/// end with the status; a WebSocket gives its heads and no body.
#[tokio::test]
async fn the_sink_sees_each_exchange() {
    pod_testkit::need_loopback!();
    let ingress = Ingress::new().catch_all(Service::Http(origin().await));
    let recorder = Arc::new(Recorder::default());
    let runtime = runtime(&ingress, Some(recorder.clone()));
    let mut ids = Vec::new();
    for (path, extra, websocket, sent) in cases() {
        let before = recorder.seen().len();
        let (status, got) = visit(&runtime, &ingress, request(path, &extra, websocket), sent).await;
        let seen = recorder.seen().split_off(before);
        let id = match seen.first() {
            Some(Seen::Head(id, _)) => *id,
            other => panic!("{path}: the first call is not a head: {other:?}"),
        };
        assert!(seen.iter().all(|s| matches!(s, Seen::Head(i, _) | Seen::Body(i, _, _) | Seen::End(i, _) if *i == id)));
        ids.push(id);
        let heads = heads(&seen);
        assert_eq!(heads.len(), 2, "{path}: {heads:?}");
        assert_eq!(heads[0].line, Line::Request { method: "POST".into(), target: path.into() }, "{path}");
        assert_eq!(heads[1].line, Line::Response { status: status.parse().unwrap() }, "{path}");
        let end = ends(&seen);
        assert_eq!(end.len(), 1, "{path}: {seen:?}");
        assert_eq!(end[0].status, Some(status.parse().unwrap()), "{path}");
        assert!(end[0].complete, "{path}");
        assert!(matches!(seen.last(), Some(Seen::End(..))), "{path}: the end is not the last call");
        let (up, down) = (body(&seen, Direction::Request), body(&seen, Direction::Response));
        match path {
            "/plain" => {
                assert_eq!(
                    (up.as_slice(), down.as_slice(), got.as_slice()),
                    (&b"ping"[..], &b"hello"[..], &b"hello"[..])
                );
                assert_eq!(heads[0].header("cookie"), Some("v=1"));
                assert_eq!(heads[1].header("set-cookie"), Some("s=1"));
            }
            "/chunked" => assert_eq!(
                (up.as_slice(), down.as_slice(), got.as_slice()),
                (&b""[..], &b"abcdefg"[..], &b"abcdefg"[..])
            ),
            // The answer of a chunked request goes as its payload too.
            "/upload" => {
                assert_eq!((up.as_slice(), down.as_slice(), got.as_slice()), (&b"upend"[..], &b"ok"[..], &b"ok"[..]))
            }
            _ => {
                assert_eq!(status, "101");
                assert_eq!(got, b"frames", "the origin sends back each byte");
                assert!(up.is_empty() && down.is_empty(), "a WebSocket gives no body: {seen:?}");
            }
        }
    }
    ids.dedup();
    assert_eq!(ids.len(), 4, "each exchange has its own id: {ids:?}");
}

/// A sink that refuses is dropped for that exchange: it gets no other call
/// and no end, and the visitor gets the same bytes as with no sink.
#[tokio::test]
async fn a_sink_that_refuses_does_not_change_the_bytes() {
    pod_testkit::need_loopback!();
    let ingress = Ingress::new().catch_all(Service::Http(origin().await));
    let plain = runtime(&ingress, None);
    let refusing = Arc::new(Recorder { refuse_body: true, ..Recorder::default() });
    let refused = runtime(&ingress, Some(refusing.clone()));
    for (path, extra, websocket, sent) in cases() {
        let want = visit(&plain, &ingress, request(path, &extra, websocket), sent).await;
        let before = refusing.seen().len();
        let got = visit(&refused, &ingress, request(path, &extra, websocket), sent).await;
        assert_eq!(got, want, "{path}");
        let seen = refusing.seen().split_off(before);
        if path == "/ws" {
            assert_eq!(ends(&seen).len(), 1, "a WebSocket gives no body, so nothing refuses: {seen:?}");
        } else {
            assert!(ends(&seen).is_empty(), "{path}: {seen:?}");
            assert!(heads(&seen).len() <= 2, "{path}: {seen:?}");
        }
    }
}

/// A request that the credential in front of the tunnel refuses reaches no
/// sink; a request that it lets pass reaches the sink with no credential.
#[tokio::test]
async fn the_credential_of_the_tunnel_never_reaches_the_sink() {
    pod_testkit::need_loopback!();
    let mut ingress = Ingress::new().catch_all(Service::Http(origin().await));
    let creds = pod_proxy::Credentials::new("u", "secret").unwrap();
    ingress.auth = Some(creds.clone());
    let recorder = Arc::new(Recorder::default());
    let runtime = runtime(&ingress, Some(recorder.clone()));
    let wrong = "Basic d3Jvbmc6d3Jvbmc=";
    let (status, _) = visit(&runtime, &ingress, request("/chunked", &[("Authorization", wrong)], false), b"").await;
    assert_eq!(status, "401");
    assert!(recorder.seen().is_empty(), "{:?}", recorder.seen());
    let right = creds.basic_header();
    let (status, _) =
        visit(&runtime, &ingress, request("/chunked", &[("Authorization", right.as_str())], false), b"").await;
    assert_eq!(status, "200");
    let seen = format!("{:?}", recorder.seen());
    assert!(!seen.to_ascii_lowercase().contains("authorization") && !seen.contains(&right), "{seen}");
    assert_eq!(heads(&recorder.seen()).len(), 2);
}

/// A writer that keeps the bytes until a flush, like a buffered edge
/// stream: `committed` holds what a flush passed on.
struct Committed(Vec<u8>, Arc<Mutex<Vec<u8>>>);

impl futures::io::AsyncWrite for Committed {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.0.extend_from_slice(buf);
        std::task::Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let held = std::mem::take(&mut self.0);
        self.1.lock().unwrap().extend_from_slice(&held);
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.poll_flush(cx)
    }
}

/// A sink that notes how many bytes the visitor had at each body part.
struct AtEachPart {
    committed: Arc<Mutex<Vec<u8>>>,
    parts: Mutex<Vec<(usize, usize)>>,
}

impl Capture for AtEachPart {
    fn head(&self, _: u64, _: &Head) -> Result<(), Refused> {
        Ok(())
    }
    fn body(&self, _: u64, _: Direction, bytes: &[u8]) -> Result<(), Refused> {
        let committed = self.committed.lock().unwrap().len();
        self.parts.lock().unwrap().push((committed, bytes.len()));
        Ok(())
    }
    fn end(&self, _: u64, _: &End) {}
}

/// The sink gets each part after its write and its flush to the visitor:
/// at each call, the visitor has each byte that the sink got.
#[tokio::test]
async fn the_sink_gets_a_part_after_the_visitor() {
    let committed = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::new(AtEachPart { committed: committed.clone(), parts: Mutex::new(Vec::new()) });
    let tap = Tap::new(Some(sink.clone()), 1);
    let mut dst = Committed(Vec::new(), committed.clone());
    let (mut origin, mut src) = tokio::io::duplex(1 << 16);
    let copy = async {
        copy_tokio_to_futures_eof(&mut src, &mut dst, &Metrics::new(), &tap).await.unwrap();
    };
    let feed = async {
        for part in [&b"one"[..], b"two-two", b"three"] {
            origin.write_all(part).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        drop(origin);
    };
    tokio::join!(copy, feed);
    let parts = sink.parts.lock().unwrap().clone();
    assert!(!parts.is_empty());
    let mut given = 0;
    for (visitor_had, len) in parts {
        given += len;
        assert!(visitor_had >= given, "the sink got {given} bytes when the visitor had {visitor_had}");
    }
    assert_eq!(committed.lock().unwrap().as_slice(), b"onetwo-twothree");
}
