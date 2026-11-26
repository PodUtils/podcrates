use super::head::*;
use super::*;
use crate::ingress::OriginOptions;
use std::sync::Arc;

/// On a QUIC connection in memory: a stream with the RPC signature is
/// reset at once, and a data stream after it is served (the signature
/// that the crate read first goes back in front of the request).
#[tokio::test]
async fn an_rpc_stream_is_reset_and_a_data_stream_is_served() {
    pod_testkit::need_loopback!();
    use crate::cloudflare::schema::ConnectionType;
    use std::sync::Arc;
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
    let pair = crate::standin::quic::local_pair().await;
    let ingress = Arc::new(Ingress::new().catch_all(Service::HelloWorld));
    let runtime = Arc::new(Runtime::build(Arc::new(crate::testing::host()), &ingress, Metrics::new()).unwrap());
    let client = pair.client.clone();
    let server = tokio::spawn(async move {
        while let Ok((send, recv)) = client.accept_bi().await {
            tokio::spawn(serve_stream(runtime.clone(), ingress.clone(), send, recv));
        }
    });

    let (mut send, mut recv) = pair.edge.open_bi().await.unwrap();
    send.write_all(&crate::cloudflare::RPC_STREAM_SIGNATURE).await.unwrap();
    send.write_all(b"01 a call").await.unwrap();
    let err = tokio::time::timeout(std::time::Duration::from_secs(5), recv.read_to_end(1024))
        .await
        .expect("an answer to the RPC stream")
        .unwrap_err();
    assert!(
        matches!(
            err,
            noq::ReadToEndError::Read(noq::ReadError::Reset(code))
                if code == noq::VarInt::from_u32(RPC_STREAM_REFUSED)
        ),
        "{err:?}"
    );

    let (send, recv) = pair.edge.open_bi().await.unwrap();
    let mut writer = send.compat_write();
    let mut reader = recv.compat();
    let request = cqstream::ConnectRequest {
        dest: "http://x.trycloudflare.com/".into(),
        conn_type: ConnectionType::Http,
        metadata: vec![(HTTP_METHOD_KEY.into(), "GET".into()), (HTTP_HOST_KEY.into(), "x.trycloudflare.com".into())],
    };
    cqstream::write_connect_request(&mut writer, &request).await.unwrap();
    futures::io::AsyncWriteExt::close(&mut writer).await.unwrap();
    let (error, meta) =
        tokio::time::timeout(std::time::Duration::from_secs(5), cqstream::read_connect_response(&mut reader))
            .await
            .expect("an answer to the data stream")
            .unwrap();
    assert_eq!(error, "");
    assert!(meta.iter().any(|(k, v)| k == cqstream::HTTP_STATUS_KEY && v == "200"), "{meta:?}");
    server.abort();
}

/// A writer that keeps the bytes until a flush, like a buffered edge
/// stream. `committed` holds what a flush passed on.
struct HoldUntilFlush {
    held: Vec<u8>,
    committed: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
}

impl futures::io::AsyncWrite for HoldUntilFlush {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.held.extend_from_slice(buf);
        std::task::Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let held = std::mem::take(&mut self.held);
        self.committed.lock().unwrap().extend_from_slice(&held);
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.poll_flush(cx)
    }
}

/// A small write of the origin reaches the edge before the origin
/// writes more (an SSE event), with a length and to the end of the
/// stream.
#[tokio::test]
async fn small_writes_go_out_at_once() {
    for with_length in [true, false] {
        let committed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut dst = HoldUntilFlush { held: Vec::new(), committed: committed.clone() };
        let (mut origin, mut src) = tokio::io::duplex(1 << 16);
        let copy = tokio::spawn(async move {
            if with_length {
                copy_tokio_to_futures_n(&mut src, &mut dst, 20, &Metrics::new(), &crate::capture::Tap::off())
                    .await
                    .map(|_| ())
            } else {
                copy_tokio_to_futures_eof(&mut src, &mut dst, &Metrics::new(), &crate::capture::Tap::off())
                    .await
                    .map(|_| ())
            }
        });
        tokio::io::AsyncWriteExt::write_all(&mut origin, b"data: event-1\n\n").await.unwrap();
        let start = std::time::Instant::now();
        while committed.lock().unwrap().len() < 15 {
            assert!(start.elapsed() < std::time::Duration::from_secs(2), "the event was held");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        tokio::io::AsyncWriteExt::write_all(&mut origin, b"12345").await.unwrap();
        drop(origin);
        copy.await.unwrap().unwrap();
        assert_eq!(committed.lock().unwrap().as_slice(), b"data: event-1\n\n12345");
    }
}

/// An answer with more than 64 headers passes.
#[tokio::test]
async fn an_origin_answer_with_many_headers_passes() {
    let mut raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n".to_vec();
    for i in 0..100 {
        raw.extend_from_slice(format!("Set-Cookie: c{i}=v\r\n").as_bytes());
    }
    raw.extend_from_slice(b"\r\nok");
    let (status, headers, leftover) = read_response_head(&mut &raw[..]).await.unwrap();
    assert_eq!((status, headers.len(), leftover.as_slice()), (200, 101, &b"ok"[..]));
}

/// Two different Content-Length values, or Content-Length
/// with a Transfer-Encoding that is not chunked, are an error.
#[test]
fn the_framing_of_an_origin_answer_is_checked() {
    let h = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
    let cl = "Content-Length";
    let te = "Transfer-Encoding";
    assert_eq!(response_length(&h(&[(cl, "5")])).unwrap(), Some(5));
    assert_eq!(response_length(&h(&[(cl, "5"), (cl, "5")])).unwrap(), Some(5));
    assert_eq!(response_length(&h(&[(cl, "5, 5")])).unwrap(), Some(5));
    assert_eq!(response_length(&h(&[])).unwrap(), None);
    assert_eq!(response_length(&h(&[(cl, "5"), (te, "chunked")])).unwrap(), None);
    for bad in [
        h(&[(cl, "5"), (cl, "6")]),
        h(&[(cl, "5, 6")]),
        h(&[(cl, "x")]),
        h(&[(cl, "+5")]),
        h(&[(cl, "5"), (te, "gzip")]),
    ] {
        assert!(response_length(&bad).is_err(), "{bad:?}");
    }
}

/// The trailer of a chunked answer has a cap, in all and
/// for each line.
#[tokio::test]
async fn the_trailer_of_the_origin_has_a_cap() {
    let mut many = b"1\r\na\r\n0\r\n".to_vec();
    for _ in 0..20_000 {
        many.extend_from_slice(b"X: y\r\n");
    }
    many.extend_from_slice(b"\r\n");
    let mut long = b"0\r\n".to_vec();
    long.extend(std::iter::repeat_n(b'a', 20_000));
    long.extend_from_slice(b"\r\n\r\n");
    for (raw, want) in [(&many, "larger than 64 KiB"), (&long, "longer than 8 KiB")] {
        let mut source = tokio::io::BufReader::new(&raw[..]);
        let mut out = futures::io::Cursor::new(Vec::new());
        let err = copy_chunked_to_futures(&mut source, &mut out, &Metrics::new(), &crate::capture::Tap::off())
            .await
            .unwrap_err();
        assert!(err.to_string().contains(want), "{err:#}");
    }
    // A small trailer is read and dropped.
    let raw = b"1\r\na\r\n0\r\nX-Sum: 1\r\n\r\n";
    let mut source = tokio::io::BufReader::new(&raw[..]);
    let mut out = futures::io::Cursor::new(Vec::new());
    copy_chunked_to_futures(&mut source, &mut out, &Metrics::new(), &crate::capture::Tap::off()).await.unwrap();
    assert_eq!(out.into_inner(), b"a");
}

#[tokio::test]
async fn a_chunked_body_is_decoded() {
    let (mut origin, podnet_side) = tokio::io::duplex(1 << 16);
    let mut wire = Vec::new();
    let mut want = Vec::new();
    for i in 0..100u32 {
        let chunk = format!("{}{i:04}", "c".repeat(1000));
        wire.extend_from_slice(format!("{:x};ext=1\r\n{chunk}\r\n", chunk.len()).as_bytes());
        want.extend_from_slice(chunk.as_bytes());
    }
    wire.extend_from_slice(b"0\r\nX-Trailer: 1\r\n\r\n");
    // The origin writes, and does not close: the decoder must end at
    // the last chunk.
    let writer = tokio::spawn(async move {
        tokio::io::AsyncWriteExt::write_all(&mut origin, &wire).await.unwrap();
        origin
    });
    let mut source = tokio::io::BufReader::new(podnet_side);
    let mut out = futures::io::Cursor::new(Vec::new());
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        copy_chunked_to_futures(&mut source, &mut out, &Metrics::new(), &crate::capture::Tap::off()),
    )
    .await
    .expect("the decoder ends at the last chunk")
    .unwrap();
    assert_eq!(out.into_inner(), want);
    drop(writer.await.unwrap());

    for bad in [&b"zz\r\nab\r\n0\r\n\r\n"[..], b"5\r\nab", b"2\r\nabXX0\r\n\r\n"] {
        let mut source = tokio::io::BufReader::new(bad);
        let mut out = futures::io::Cursor::new(Vec::new());
        assert!(
            copy_chunked_to_futures(&mut source, &mut out, &Metrics::new(), &crate::capture::Tap::off()).await.is_err(),
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
}
use crate::cloudflare::stream::ConnectionType;

fn req(dest: &str, metadata: Vec<(&str, &str)>) -> cqstream::ConnectRequest {
    cqstream::ConnectRequest {
        dest: dest.to_string(),
        conn_type: ConnectionType::Http,
        metadata: metadata.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    }
}

// With --auth-user, a request with no right credential gets
// 401 and reaches no origin; the right one passes, and the origin does
// not get the header.
#[tokio::test]
async fn the_credential_in_front_of_the_tunnel() {
    pod_testkit::need_loopback!();
    use tokio::io::AsyncReadExt as _;
    use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
    // An origin that answers with the request head it got.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let answer =
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{head}", head.len());
                let _ = tokio::io::AsyncWriteExt::write_all(&mut s, answer.as_bytes()).await;
            });
        }
    });
    let mut ingress = Ingress::new().catch_all(Service::Http(origin));
    let creds = pod_proxy::Credentials::new("u", "secret").unwrap();
    ingress.auth = Some(creds.clone());
    let runtime = Runtime::build(Arc::new(crate::testing::host()), &ingress, Metrics::new()).unwrap();
    let run = |auth: Option<String>| {
        let mut meta = vec![(HTTP_METHOD_KEY, "GET".to_string()), (HTTP_HOST_KEY, "x".to_string())];
        let header = format!("{HTTP_HEADER_KEY}:Authorization");
        let mut request = req("https://x/", vec![]);
        for (k, v) in meta.drain(..) {
            request.metadata.push((k.to_string(), v));
        }
        if let Some(a) = auth {
            request.metadata.push((header, a));
        }
        request
    };
    for (auth, want) in
        [(None, "401"), (Some("Basic d3Jvbmc6d3Jvbmc=".to_string()), "401"), (Some(creds.basic_header()), "200")]
    {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let (_input, input) = tokio::io::duplex(64);
        serve_request_inner(
            &runtime,
            &ingress,
            run(auth.clone()),
            Box::pin(input.compat()),
            Box::pin(ours.compat_write()),
        )
        .await
        .unwrap();
        let mut reader = theirs.compat();
        let (_, meta) = cqstream::read_connect_response(&mut reader).await.unwrap();
        let status = meta
            .iter()
            .find(|(k, _)| k == crate::cloudflare::stream::HTTP_STATUS_KEY)
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        assert_eq!(status, want, "{auth:?}");
        if want == "200" {
            let mut body = Vec::new();
            let _ = futures::io::AsyncReadExt::read_to_end(&mut reader, &mut body).await;
            let body = String::from_utf8_lossy(&body);
            assert!(!body.to_ascii_lowercase().contains("authorization"), "{body}");
        }
    }
}

// An HTTP request to a raw TCP origin gets 501, with no dial.
#[tokio::test]
async fn an_http_request_to_a_tcp_origin_gets_501() {
    pod_testkit::need_loopback!();
    use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service = Service::Tcp(listener.local_addr().unwrap().to_string());
    let request = req("https://x/", vec![(HTTP_HOST_KEY, "x")]);
    let (ours, theirs) = tokio::io::duplex(4096);
    let (_input, input) = tokio::io::duplex(64);
    forward_tcp(
        &crate::origin::Origins::new(crate::testing::tls()),
        &service,
        &OriginOptions::default(),
        &request,
        Box::pin(input.compat()),
        Box::pin(ours.compat_write()),
        &Metrics::new(),
    )
    .await
    .unwrap();
    let mut reader = theirs.compat();
    let (error, meta) = cqstream::read_connect_response(&mut reader).await.unwrap();
    assert!(error.is_empty(), "{error}");
    assert!(meta.contains(&(crate::cloudflare::stream::HTTP_STATUS_KEY.to_string(), "501".to_string())), "{meta:?}");
    let accepted = tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept()).await;
    assert!(accepted.is_err(), "the origin was dialed");
}

#[test]
fn head_and_flags() {
    let request =
        req("https://x/a", vec![(HTTP_METHOD_KEY, "POST"), (HTTP_HOST_KEY, "x"), ("HttpHeader:Content-Length", "5")]);
    assert_eq!(content_length(&request), Some(5));
    let head = build_request_head(&request, &OriginOptions::default(), false).unwrap();
    assert!(head.starts_with("POST /a HTTP/1.1\r\n"));
    assert!(head.contains("Connection: close\r\n"));
}

/// A WebSocket stream of the QUIC leg (connection type
/// Websocket) with no Upgrade header still upgrades the origin.
#[test]
fn a_websocket_stream_with_no_upgrade_header_upgrades() {
    let mut ws = req(
        "https://x/ws",
        vec![
            (HTTP_HOST_KEY, "x"),
            ("HttpHeader:Sec-Websocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ("HttpHeader:Connection", "keep-alive"),
        ],
    );
    ws.conn_type = cqstream::ConnectionType::Websocket;
    assert!(!is_upgrade(&ws) && is_websocket(&ws));
    let head = build_request_head(&ws, &OriginOptions::default(), is_websocket(&ws)).unwrap();
    assert!(head.contains("Upgrade: websocket\r\n"), "{head}");
    assert!(head.contains("Connection: Upgrade\r\n"), "{head}");
    assert!(!head.contains("keep-alive") && !head.contains("Connection: close"), "{head}");
    assert!(head.contains("Sec-Websocket-Key: dGhlIHNhbXBsZSBub25jZQ=="), "{head}");
    // With an Upgrade header, it is not doubled.
    let with = req("https://x/ws", vec![("HttpHeader:Upgrade", "websocket")]);
    let head = build_request_head(&with, &OriginOptions::default(), true).unwrap();
    assert_eq!(head.matches("pgrade: websocket").count(), 1, "{head}");
}

/// A line break in a field of the edge does not reach the
/// origin as a new header line.
#[test]
fn a_line_break_does_not_make_a_header_line() {
    let request = req(
        "https://x/a",
        vec![
            (HTTP_HOST_KEY, "x"),
            ("HttpHeader:X-A", "a\r\nInjected: 1"),
            ("HttpHeader:Bad Name", "b"),
            ("HttpHeader:X-Name\r\nInjected", "c"),
            ("HttpHeader:Cookie", "name=caf\u{e9}"),
        ],
    );
    let head = build_request_head(&request, &OriginOptions::default(), false).unwrap();
    assert!(!head.contains("Injected"), "{head}");
    assert!(!head.contains("Bad Name"), "{head}");
    assert!(head.contains("Cookie: name=caf\u{e9}\r\n"), "{head}");
    assert_eq!(head.matches("\r\n\r\n").count(), 1, "{head}");
    assert!(head.ends_with("\r\n\r\n"), "{head}");
    assert_eq!(head.matches('\n').count(), head.matches("\r\n").count());

    let bad = [
        req("https://x/a", vec![(HTTP_METHOD_KEY, "GET /x HTTP/1.1\r\n")]),
        req("https://x/a\r\nInjected: 1", vec![]),
        req("https://x/a b", vec![]),
        req("https://x/a", vec![(HTTP_HOST_KEY, "x\nInjected: 1")]),
    ];
    for request in &bad {
        assert!(build_request_head(request, &OriginOptions::default(), false).is_err(), "{request:?}");
    }
    let options = OriginOptions { http_host_header: Some("x\r\nInjected: 1".into()), ..OriginOptions::default() };
    let err = build_request_head(&req("https://x/", vec![]), &options, false).unwrap_err();
    assert!(err.to_string().contains("httpHostHeader"), "{err}");
}

/// The hop-by-hop headers and the headers that Connection
/// names do not go to the origin. Transfer-Encoding stays for a chunked
/// body.
#[test]
fn hop_by_hop_headers_stay_on_their_hop() {
    let request = req(
        "https://x/u",
        vec![
            (HTTP_METHOD_KEY, "POST"),
            ("HttpHeader:Keep-Alive", "timeout=5"),
            ("HttpHeader:Te", "trailers"),
            ("HttpHeader:Trailer", "X-Sum"),
            ("HttpHeader:Proxy-Authorization", "Basic YTpi"),
            ("HttpHeader:Connection", "close, X-Hop"),
            ("HttpHeader:X-Hop", "1"),
            ("HttpHeader:X-Keep", "2"),
            ("HttpHeader:Transfer-Encoding", "chunked"),
        ],
    );
    let head = build_request_head(&request, &OriginOptions::default(), false).unwrap();
    for gone in ["Keep-Alive", "Te:", "Trailer", "Proxy-Authorization", "X-Hop"] {
        assert!(!head.contains(gone), "{gone} in {head}");
    }
    assert!(head.contains("X-Keep: 2\r\n"), "{head}");
    assert!(head.contains("Transfer-Encoding: chunked\r\n"), "{head}");
    assert_eq!(head.matches("Connection:").count(), 1, "{head}");
    assert!(head.contains("Connection: close\r\n"), "{head}");
    let sized = req("https://x/u", vec![("HttpHeader:Content-Length", "5")]);
    let head = build_request_head(&sized, &OriginOptions::default(), false).unwrap();
    assert!(head.contains("Content-Length: 5\r\n"), "{head}");
}

#[test]
fn upgrade_and_chunked() {
    let ws = req("https://x/ws", vec![("HttpHeader:Upgrade", "websocket")]);
    assert!(is_upgrade(&ws));
    let chunked = req("https://x/u", vec![("HttpHeader:Transfer-Encoding", "chunked")]);
    assert!(is_chunked(&chunked));
}

#[test]
fn strips_accept_encoding_on_request() {
    let request = req("https://x/", vec![("HttpHeader:Accept-Encoding", "gzip, br")]);
    let head = build_request_head(&request, &OriginOptions::default(), false).unwrap();
    assert!(head.contains("Accept-Encoding: gzip, br"));

    let options = OriginOptions { strip_accept_encoding: true, ..OriginOptions::default() };
    let head = build_request_head(&request, &options, false).unwrap();
    assert!(!head.to_ascii_lowercase().contains("accept-encoding"));
}

#[test]
fn response_meta_prefixes_headers() {
    let headers = vec![
        ("Content-Type".to_string(), "text/plain".to_string()),
        ("Set-Cookie".to_string(), "a=b".to_string()),
        ("HttpHeader:Location".to_string(), "/next".to_string()),
    ];
    let meta = response_meta_pairs(303, &headers);
    assert_eq!(meta[0], ("HttpStatus".to_string(), "303".to_string()));
    assert!(meta.contains(&("HttpHeader:Content-Type".to_string(), "text/plain".to_string())));
    assert!(meta.contains(&("HttpHeader:Set-Cookie".to_string(), "a=b".to_string())));
    assert!(meta.contains(&("HttpHeader:Location".to_string(), "/next".to_string())));
    assert!(!meta.iter().any(|(k, _)| k == "Content-Type"));
}
