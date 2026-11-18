use super::*;

fn parts(headers: &[(&str, &str)]) -> http::request::Parts {
    let mut builder = http::Request::builder().method("POST").uri("https://app.example.com/api/x?y=1");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(()).unwrap().into_parts().0
}

#[test]
fn classifies_streams_in_cloudflared_order() {
    let p = parts(&[(CONTROL_STREAM_HEADER, CONTROL_STREAM_VALUE)]);
    assert_eq!(stream_type(&p.headers), StreamType::Control);
    let p = parts(&[(CONTROL_STREAM_HEADER, "update-configuration")]);
    assert_eq!(stream_type(&p.headers), StreamType::Configuration);
    let p = parts(&[(CONTROL_STREAM_HEADER, "websocket")]);
    assert_eq!(stream_type(&p.headers), StreamType::Websocket);
    let p = parts(&[(PROXY_SRC_HEADER, "10.0.0.1:22")]);
    assert_eq!(stream_type(&p.headers), StreamType::Tcp);
    let p = parts(&[("accept", "*/*")]);
    assert_eq!(stream_type(&p.headers), StreamType::Http);
}

#[test]
fn builds_a_connect_request_like_the_quic_transport() {
    let p = parts(&[
        ("content-type", "application/json"),
        ("cf-ray", "abc"),
        ("cf-cloudflared-proxy-connection-upgrade", ""),
    ]);
    let request = connect_request(&p, StreamType::Http);
    assert_eq!(request.dest, "https://app.example.com/api/x?y=1");
    assert_eq!(request.conn_type, ConnectionType::Http);
    assert_eq!(request.meta(HTTP_METHOD_KEY), Some("POST"));
    assert_eq!(request.meta(HTTP_HOST_KEY), Some("app.example.com"));
    assert_eq!(request.meta("HttpHeader:Content-Type"), Some("application/json"));
    // cf-ray is a visitor header that the origin sees in cloudflared too.
    assert_eq!(request.meta("HttpHeader:Cf-Ray"), Some("abc"));
    assert!(request.metadata.iter().all(|(k, _)| !k.to_ascii_lowercase().contains("cloudflared")));
}

#[test]
fn a_websocket_request_gets_the_upgrade_headers() {
    let p = parts(&[(CONTROL_STREAM_HEADER, "websocket"), ("sec-websocket-key", "k")]);
    let request = connect_request(&p, StreamType::Websocket);
    assert_eq!(request.conn_type, ConnectionType::Websocket);
    assert_eq!(request.meta("HttpHeader:Upgrade"), Some("websocket"));
    assert_eq!(request.meta("HttpHeader:Connection"), Some("Upgrade"));
    assert_eq!(request.meta("HttpHeader:Sec-Websocket-Key"), Some("k"));
}

#[test]
fn a_tcp_request_names_the_destination() {
    let p = parts(&[(PROXY_SRC_HEADER, "10.0.0.1:22")]);
    let request = connect_request(&p, StreamType::Tcp);
    assert_eq!(request.conn_type, ConnectionType::Tcp);
    assert_eq!(request.dest, "app.example.com");
}

#[test]
fn serializes_response_headers_for_the_edge() {
    let meta = vec![
        (HTTP_STATUS_KEY.to_string(), "201".to_string()),
        ("HttpHeader:Content-Length".to_string(), "5".to_string()),
        ("HttpHeader:X-Thing".to_string(), "v".to_string()),
    ];
    let (status, headers) = edge_response("", &meta);
    assert_eq!(status, 201);
    assert!(headers.iter().any(|(k, v)| k == "content-length" && v == "5"));
    let blob = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("cf-cloudflared-response-headers"))
        .map(|(_, v)| v.clone())
        .unwrap();
    let decoded = crate::cloudflare::http2::decode_headers(&blob);
    assert!(decoded.contains(&("X-Thing".to_string(), "v".to_string())));
    let (status, _) = edge_response("origin down", &[]);
    assert_eq!(status, 502);
}

/// Visitors whose origin reads nothing send small WebSocket frames until
/// the windows stop the edge. The connection must stay up and serve a
/// new visitor. With the default budget of h2, ENHANCE_YOUR_CALM ends it,
/// and each visitor with it.
#[tokio::test(flavor = "multi_thread")]
async fn small_frames_of_slow_visitors_do_not_end_the_connection() {
    use crate::ingress::{ForwardTarget, IngressRule, OriginOptions, Service};
    // A forward target that accepts and never reads: TCP on the loopback,
    // so that the test runs on each system.
    pod_testkit::need_loopback!();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        let mut kept = Vec::new();
        while let Ok((s, _)) = listener.accept().await {
            kept.push(s);
        }
    });
    let ingress = Arc::new(
        Ingress::new()
            .rule_with(IngressRule {
                hostname: None,
                path: Some(crate::ingress::PathPattern::prefix("/__podnet/ws")),
                service: Service::Forward(ForwardTarget::Tcp(target)),
                origin: OriginOptions::default(),
            })
            .catch_all(Service::HelloWorld),
    );
    let runtime =
        Arc::new(Runtime::build(Arc::new(crate::testing::host()), &ingress, crate::metrics::Metrics::new()).unwrap());
    let (edge_side, podnet_side) = tokio::io::duplex(1 << 20);
    let client = tokio::spawn(h2::client::Builder::new().max_concurrent_streams(1000).handshake::<_, Bytes>(edge_side));
    let _served = serve_connection(podnet_side, runtime, ingress, Keepalive::default(), None).await.unwrap();
    let (mut send, conn) = client.await.unwrap().unwrap();
    let conn = tokio::spawn(conn);
    // A masked WebSocket binary frame of 60 bytes.
    let mut frame = vec![0x82u8, 0x80 | 54, 1, 2, 3, 4];
    frame.extend(std::iter::repeat_n(0u8, 54));
    let frame = Bytes::from(frame);
    let mut streams = Vec::new();
    for _ in 0..16 {
        let request = http::Request::builder()
            .method("GET")
            .uri("https://x.example/__podnet/ws")
            .header(CONTROL_STREAM_HEADER, WEBSOCKET_VALUE)
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .body(())
            .unwrap();
        send = send.ready().await.unwrap();
        let (_response, mut body) = send.send_request(request, false).unwrap();
        let frame = frame.clone();
        streams.push(tokio::spawn(async move {
            // Send until the window stops the edge (no capacity for 2 s).
            loop {
                body.reserve_capacity(frame.len());
                match tokio::time::timeout(
                    Duration::from_secs(2),
                    futures::future::poll_fn(|cx| body.poll_capacity(cx)),
                )
                .await
                {
                    Ok(Some(Ok(n))) if n >= frame.len() => {
                        if body.send_data(frame.clone(), false).is_err() {
                            return;
                        }
                    }
                    Ok(Some(Ok(_))) => continue,
                    _ => return,
                }
            }
        }));
    }
    for s in streams {
        let _ = s.await;
    }
    // A new visitor still gets its answer on the same connection.
    let request = http::Request::builder().method("GET").uri("https://x.example/").body(()).unwrap();
    let mut send = tokio::time::timeout(Duration::from_secs(5), send.ready())
        .await
        .expect("the connection takes a new stream")
        .expect("the connection is up");
    let (response, _) = send.send_request(request, true).unwrap();
    let response =
        tokio::time::timeout(Duration::from_secs(5), response).await.expect("an answer").expect("no ENHANCE_YOUR_CALM");
    assert_eq!(response.status(), 200);
    assert!(!conn.is_finished(), "the connection ended");
}

fn service() -> (Arc<Runtime>, Arc<Ingress>) {
    let ingress = Arc::new(Ingress::new().catch_all(crate::ingress::Service::HelloWorld));
    let runtime =
        Arc::new(Runtime::build(Arc::new(crate::testing::host()), &ingress, crate::metrics::Metrics::new()).unwrap());
    (runtime, ingress)
}

/// An edge that is silent after TLS does not hold the
/// HTTP/2 handshake.
#[tokio::test(start_paused = true)]
async fn a_silent_edge_fails_the_handshake_in_the_dial_time() {
    let (runtime, ingress) = service();
    let (_edge_side, podnet_side) = tokio::io::duplex(1 << 16);
    let start = tokio::time::Instant::now();
    let served = serve_connection(podnet_side, runtime, ingress, Keepalive::default(), None);
    let err = match tokio::time::timeout(DIAL_TIMEOUT * 2, served).await {
        Ok(Err(err)) => err,
        Ok(Ok(_)) => panic!("the handshake passed"),
        Err(_) => panic!("the handshake has no time limit"),
    };
    assert!(err.to_string().contains("timed out"), "{err:#}");
    assert_eq!(start.elapsed(), DIAL_TIMEOUT);
}

/// A failed registration closes the connection: the task
/// that serves it does not go on.
#[tokio::test(start_paused = true)]
async fn a_failed_registration_closes_the_connection() {
    let (runtime, ingress) = service();
    let (edge_side, podnet_side) = tokio::io::duplex(1 << 16);
    let edge = tokio::spawn(h2::client::handshake(edge_side));
    let auth = rpc::TunnelAuth { account_tag: "test".into(), tunnel_secret: vec![0; 32] };
    let options = rpc::ConnectionOptions::default_for_quick_tunnel("test");
    let addr: SocketAddr = "127.0.0.1:7844".parse().unwrap();
    // The edge opens no control stream.
    let attempt = register_served(addr, podnet_side, &auth, Uuid::nil(), 0, &options, runtime, ingress, None);
    let (_send, connection) = edge.await.unwrap().unwrap();
    let driver = tokio::spawn(connection);
    let err = match attempt.await {
        Err(err) => err,
        Ok(_) => panic!("the registration passed"),
    };
    assert!(err.to_string().contains("control stream"), "{err:#}");
    // The connection ends soon, not at the next PING (30 s + 30 s).
    let closed = tokio::time::timeout(Duration::from_secs(1), driver).await;
    assert!(closed.is_ok(), "the connection is still served");
}

/// The configuration push has a cap and a time limit.
#[tokio::test(start_paused = true)]
async fn a_configuration_push_has_a_cap_and_a_time_limit() {
    let ok = read_configuration(&br#"{"version":7}"#[..], REGISTER_TIMEOUT).await;
    assert_eq!(ok.unwrap(), br#"{"version":7}"#);
    // A push larger than the cap, from a source that never ends.
    let big = tokio::io::repeat(b' ');
    let err = read_configuration(big, REGISTER_TIMEOUT).await.unwrap_err();
    assert!(err.to_string().contains("larger than"), "{err:#}");
    // A push that never ends.
    let (_keep, silent) = tokio::io::duplex(64);
    let err = read_configuration(silent, REGISTER_TIMEOUT).await.unwrap_err();
    assert!(err.to_string().contains("did not end"), "{err:#}");
}

/// The edge can open as many streams as on QUIC, not
/// u32::MAX.
#[tokio::test]
async fn the_edge_gets_the_stream_limit_of_quic() {
    let (runtime, ingress) = service();
    let (edge_side, podnet_side) = tokio::io::duplex(1 << 16);
    let client = tokio::spawn(h2::client::handshake(edge_side));
    let _served = serve_connection(podnet_side, runtime, ingress, Keepalive::default(), None).await.unwrap();
    let (send, connection) = client.await.unwrap().unwrap();
    let driver = tokio::spawn(connection);
    // The SETTINGS of the client come with the first round trip.
    let send = send.ready().await.unwrap();
    let mut send = send;
    let request = http::Request::builder().method("GET").uri("https://x.trycloudflare.com/").body(()).unwrap();
    let (response, _) = send.send_request(request, true).unwrap();
    response.await.unwrap();
    assert_eq!(send.current_max_send_streams(), crate::cloudflare::MAX_EDGE_STREAMS as usize);
    drop(send);
    driver.abort();
}

/// A spent byte budget drains the connection, as a GOAWAY does: the
/// tunnel connects a replacement before the relay ends the session.
#[tokio::test]
async fn a_spent_budget_drains_the_connection() {
    let (runtime, ingress) = service();
    let (edge_side, podnet_side) = tokio::io::duplex(1 << 16);
    let spent = Arc::new(tokio::sync::Notify::new());
    let podnet_side = Metered::new(podnet_side, 4 << 10, spent.clone());
    let budget = Budget { spent, age: Duration::from_secs(3600) };
    let client = tokio::spawn(h2::client::handshake(edge_side));
    let served = serve_connection(podnet_side, runtime, ingress, Keepalive::default(), Some(budget)).await.unwrap();
    let (mut send, connection) = client.await.unwrap().unwrap();
    let _driver = tokio::spawn(connection);
    let mut draining = served.draining;
    assert!(draining.try_recv().is_err(), "no drain before the budget is spent");
    // The answers of more requests than the budget go through the meter.
    let mut reason = None;
    for _ in 0..200 {
        if let Ok(r) = draining.try_recv() {
            reason = Some(r);
            break;
        }
        let request = http::Request::builder().method("GET").uri("https://x.trycloudflare.com/").body(()).unwrap();
        let (response, _) = send.send_request(request, true).unwrap();
        let response = response.await.unwrap();
        let mut body = response.into_body();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.unwrap();
            let _ = body.flow_control().release_capacity(chunk.len());
        }
    }
    let reason = match reason {
        Some(reason) => reason,
        None => tokio::time::timeout(Duration::from_secs(5), draining)
            .await
            .expect("the spent budget drains the connection")
            .unwrap(),
    };
    assert!(reason.contains("byte budget"), "{reason}");
}

/// An old connection drains at the age of its budget.
#[tokio::test(start_paused = true)]
async fn an_old_connection_drains() {
    let (runtime, ingress) = service();
    let (edge_side, podnet_side) = tokio::io::duplex(1 << 16);
    let budget = Budget { spent: Arc::new(tokio::sync::Notify::new()), age: Duration::from_secs(600) };
    let keepalive = Keepalive { interval: Duration::from_secs(3600 * 24), timeout: Duration::from_secs(30) };
    let client = tokio::spawn(h2::client::handshake(edge_side));
    let served = serve_connection(podnet_side, runtime, ingress, keepalive, Some(budget)).await.unwrap();
    let (_send, connection) = client.await.unwrap().unwrap();
    let _driver = tokio::spawn(connection);
    let reason = tokio::time::timeout(Duration::from_secs(601), served.draining)
        .await
        .expect("the old connection drains")
        .unwrap();
    assert!(reason.contains("10 min old"), "{reason}");
}

/// An edge that stops answering (its HTTP/2 connection is not polled
/// any more) is found by the PING, not by the OS.
#[tokio::test]
async fn a_ping_with_no_answer_ends_the_connection() {
    let (runtime, ingress) = service();
    let (edge_side, podnet_side) = tokio::io::duplex(1 << 16);
    let keepalive = Keepalive { interval: Duration::from_millis(100), timeout: Duration::from_millis(200) };
    let client = tokio::spawn(h2::client::handshake(edge_side));
    let served = serve_connection(podnet_side, runtime, ingress, keepalive, None).await.unwrap();
    // The edge side keeps its connection but never polls it again.
    let _edge = client.await.unwrap().unwrap();
    let reason = tokio::time::timeout(Duration::from_secs(5), served.draining)
        .await
        .expect("the lost connection is found")
        .unwrap();
    assert!(reason.contains("did not answer a PING"), "{reason}");
}

/// When the edge ends the connection, the draining signal comes, and a
/// stream that is open goes on to its end.
#[tokio::test]
async fn goaway_drains_and_open_streams_finish() {
    let (runtime, ingress) = service();
    let (edge_side, podnet_side) = tokio::io::duplex(1 << 16);
    let client = tokio::spawn(h2::client::handshake(edge_side));
    let served = serve_connection(podnet_side, runtime, ingress, Keepalive::default(), None).await.unwrap();
    let (mut send, connection) = client.await.unwrap().unwrap();
    let driver = tokio::spawn(connection);
    let request = http::Request::builder().method("GET").uri("https://x.trycloudflare.com/").body(()).unwrap();
    let (response, _) = send.send_request(request, true).unwrap();
    // No more requests from this edge connection: it sends GOAWAY when
    // its streams end.
    drop(send);
    let response = response.await.unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body();
    let mut text = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.unwrap();
        let _ = body.flow_control().release_capacity(chunk.len());
        text.extend_from_slice(&chunk);
    }
    assert_eq!(text, b"Hello, world! (podnet)\n");
    drop(body);
    let reason =
        tokio::time::timeout(Duration::from_secs(5), served.draining).await.expect("the drain signal").unwrap();
    assert!(reason.contains("GOAWAY") || reason.contains("failed"), "{reason}");
    let _ = tokio::time::timeout(Duration::from_secs(5), served.serve).await;
    driver.abort();
}

/// An edge that sends GOAWAY while a stream is open (a long WebSocket):
/// the drain signal comes at once, not when the stream ends. The test
/// writes the HTTP/2 frames itself.
#[tokio::test]
async fn goaway_with_an_open_stream_drains_at_once() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
        let len = payload.len() as u32;
        let mut out = len.to_be_bytes()[1..].to_vec();
        out.extend_from_slice(&[kind, flags]);
        out.extend_from_slice(&stream.to_be_bytes());
        out.extend_from_slice(payload);
        out
    }
    let (runtime, ingress) = service();
    let (mut edge, podnet_side) = tokio::io::duplex(1 << 16);
    let mut start = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".to_vec();
    start.extend(frame(0x4, 0, 0, &[])); // SETTINGS
    // HEADERS on stream 1, END_HEADERS, no END_STREAM: GET https://x/.
    start.extend(frame(0x1, 0x4, 1, &[0x82, 0x87, 0x84, 0x41, 0x01, b'x']));
    edge.write_all(&start).await.unwrap();
    let served = serve_connection(podnet_side, runtime, ingress, Keepalive::default(), None).await.unwrap();
    let mut draining = served.draining;
    // The edge reads what the client sends, so the client is not blocked.
    let (mut edge_read, mut edge_write) = tokio::io::split(edge);
    let reader = tokio::spawn(async move {
        let mut buf = vec![0u8; 1 << 16];
        while edge_read.read(&mut buf).await.unwrap_or(0) > 0 {}
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(draining.try_recv().is_err(), "no drain before GOAWAY");
    edge_write
        .write_all(&frame(0x4, 0x1, 0, &[])) // SETTINGS ACK
        .await
        .unwrap();
    let mut goaway = 1u32.to_be_bytes().to_vec();
    goaway.extend_from_slice(&0u32.to_be_bytes());
    edge_write.write_all(&frame(0x7, 0, 0, &goaway)).await.unwrap();
    let reason = tokio::time::timeout(Duration::from_secs(2), draining)
        .await
        .expect("the drain signal while stream 1 is open")
        .unwrap();
    assert!(reason.contains("GOAWAY"), "{reason}");
    reader.abort();
}

/// The scan finds a GOAWAY header in any split of the bytes, and skips
/// the payloads (a payload with the byte 0x07 in it is not a header).
#[test]
fn the_frame_scan_finds_goaway_in_any_split() {
    let mut bytes = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".to_vec();
    bytes.extend_from_slice(&[0, 0, 6, 0x4, 0, 0, 0, 0, 0]); // SETTINGS
    bytes.extend_from_slice(&[0, 3, 0, 0, 0, 0x7]);
    bytes.extend_from_slice(&[0, 0, 4, 0x0, 0, 0, 0, 0, 1, 7, 7, 7, 7]); // DATA
    let before_goaway = bytes.len();
    bytes.extend_from_slice(&[0, 0, 8, 0x7, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    for split in 1..bytes.len() {
        let mut scan = FrameScan::new();
        let mut found_at = None;
        for (i, chunk) in bytes.chunks(split).enumerate() {
            if scan.feed(chunk) && found_at.is_none() {
                found_at = Some((i + 1) * split);
            }
        }
        let at = found_at.unwrap_or_else(|| panic!("split {split}: no GOAWAY"));
        assert!(at > before_goaway, "split {split}: GOAWAY found too early");
    }
    let mut scan = FrameScan::new();
    assert!(!scan.feed(&bytes[..before_goaway]));
}

/// Run a real HTTP/2 connection in memory: the test is the edge (the
/// HTTP/2 client), the code under test is the server. A visitor request
/// goes through the hello_world service and comes back with serialized
/// headers.
#[tokio::test]
async fn serves_a_visitor_request_over_http2() {
    let ingress = Arc::new(Ingress::new().catch_all(crate::ingress::Service::HelloWorld));
    let runtime =
        Arc::new(Runtime::build(Arc::new(crate::testing::host()), &ingress, crate::metrics::Metrics::new()).unwrap());
    let (edge_side, podnet_side) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn(async move {
        let mut conn = h2::server::handshake(podnet_side).await.unwrap();
        while let Some(Ok((request, respond))) = conn.accept().await {
            let kind = stream_type(request.headers());
            let runtime = runtime.clone();
            let ingress = ingress.clone();
            tokio::spawn(async move {
                serve_stream(kind, request, respond, runtime, ingress).await.unwrap();
            });
        }
    });
    let (mut client, connection) = h2::client::handshake(edge_side).await.unwrap();
    tokio::spawn(connection);
    let request = http::Request::builder().method("GET").uri("https://x.trycloudflare.com/").body(()).unwrap();
    let (response, _) = client.send_request(request, true).unwrap();
    let response = response.await.unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers().contains_key("cf-cloudflared-response-headers"));
    let mut body = response.into_body();
    let mut text = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.unwrap();
        let _ = body.flow_control().release_capacity(chunk.len());
        text.extend_from_slice(&chunk);
    }
    assert_eq!(text, b"Hello, world! (podnet)\n");
    drop(client);
    server.abort();
}

/// A leg with no relay dials the edge directly, also when the host has a
/// relay; the leg with the relay dials through it.
#[tokio::test]
async fn a_leg_with_no_relay_dials_the_edge_directly() {
    pod_testkit::need_loopback!();
    let edge = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = edge.local_addr().unwrap();
    let refusing = Arc::new(crate::testing::Refusing::default());
    let mut host = crate::testing::host();
    host.relay = Some(crate::testing::relay(refusing.clone()));
    let relay = host.relay.clone();
    let (index, _io) = dial(&host, &[addr], None).await.expect("a direct dial");
    assert_eq!(index, 0);
    assert_eq!(refusing.calls.load(std::sync::atomic::Ordering::SeqCst), 0, "the direct leg took the relay");
    let err = dial(&host, &[addr], relay.as_ref()).await.err().expect("the relay of the test refuses");
    assert_eq!(err.stage, pod_core::Stage::Relay);
    assert_eq!(refusing.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
