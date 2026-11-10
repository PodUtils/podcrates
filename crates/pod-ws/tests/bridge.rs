//! The bridge of a session and a raw stream, and the client WebSocket that a
//! bridge dials (podnet's `ws.rs`): bytes go both ways, a Close of the far
//! end ends the bridge, the counter counts each byte, and the credential of
//! a tunnel goes in `Authorization`, with a 401 as a refusal. Each server is
//! a stand-in on the loopback with this crate's server side.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pod_core::BoxedIo;
use pod_ws::frame::{self, Frame, Role};
use pod_ws::{ConnectError, Tally, Trust, bridge, server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

mod common;

const LIMIT: Duration = Duration::from_secs(20);

/// The counter of the tests.
#[derive(Default)]
struct Count {
    inward: AtomicU64,
    outward: AtomicU64,
}

impl Tally for Count {
    fn add_in(&self, bytes: u64) {
        self.inward.fetch_add(bytes, Ordering::SeqCst);
    }
    fn add_out(&self, bytes: u64) {
        self.outward.fetch_add(bytes, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn bridge_moves_bytes_both_ways() {
    let (edge_server, edge_client) = tokio::io::duplex(1 << 16);
    let (target_a, mut target_b) = tokio::io::duplex(1 << 16);

    let server = tokio::spawn(async move {
        let session = server::session(edge_server, Vec::new(), None);
        bridge::bridge(session, Box::new(target_a), &pod_ws::NoTally).await
    });

    // The handshake is covered by `accept_key_matches_rfc_example`; here the
    // framing is driven directly on both ends.
    let ws = pod_ws::RelaySession::new(edge_client, Vec::new(), Some(LIMIT), LIMIT);

    // client -> target
    ws.send_binary(b"hello").await.unwrap();
    let mut buf = [0u8; 5];
    target_b.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hello");

    // target -> client
    target_b.write_all(b"world").await.unwrap();
    let message = ws.read_frame().await.unwrap();
    assert_eq!((message.opcode, message.payload.as_slice()), (frame::OPCODE_BINARY, &b"world"[..]));

    drop(ws);
    let _ = server.await.unwrap();
}

/// The far end's Close (a relay at its byte cap: 1009) ends the way to the
/// raw stream, whose write side shuts; the end of the raw stream sends a
/// Close 1000; the counter has each byte of each way.
#[tokio::test]
async fn a_close_of_the_far_end_ends_the_bridge_and_each_byte_is_counted() {
    let (near, mut far) = tokio::io::duplex(1 << 16);
    let (raw, mut target) = tokio::io::duplex(1 << 16);
    let count = Arc::new(Count::default());
    let counted = count.clone();
    let task = tokio::spawn(async move {
        let session = pod_ws::RelaySession::new(near, Vec::new(), Some(LIMIT), LIMIT);
        bridge::bridge(session, Box::new(raw), &*counted).await
    });
    let from_far = |opcode: u8, payload: &[u8]| {
        frame::encode(&Frame { fin: true, opcode, payload: payload.to_vec() }, Role::Server, [0; 4])
    };
    far.write_all(&from_far(frame::OPCODE_BINARY, b"0123456789")).await.unwrap();
    let mut got = [0u8; 10];
    target.read_exact(&mut got).await.unwrap();
    target.write_all(b"abc").await.unwrap();
    // The 3 bytes as one binary message, before the far end closes.
    let mut wire = vec![0u8; 64];
    let n = tokio::time::timeout(LIMIT, far.read(&mut wire)).await.expect("a message").unwrap();
    let (first, used) = frame::decode(&wire[..n], Role::Client).unwrap().unwrap();
    assert_eq!((first.opcode, first.payload.as_slice(), used), (frame::OPCODE_BINARY, &b"abc"[..], n));
    let mut close = 1009u16.to_be_bytes().to_vec();
    close.extend_from_slice(b"session byte cap");
    far.write_all(&from_far(frame::OPCODE_CLOSE, &close)).await.unwrap();
    // The write side of the raw stream shuts: its reader sees the end.
    let mut rest = Vec::new();
    tokio::time::timeout(LIMIT, target.read_to_end(&mut rest)).await.expect("the end of the raw stream").unwrap();
    assert!(rest.is_empty(), "{rest:?}");
    drop(target);
    tokio::time::timeout(LIMIT, task).await.expect("the bridge ends").unwrap().expect("a clean end");
    assert_eq!(count.inward.load(Ordering::SeqCst), 10);
    assert_eq!(count.outward.load(Ordering::SeqCst), 3);
    // The answer to the Close, with its code, and nothing after it.
    let mut wire = Vec::new();
    far.read_to_end(&mut wire).await.unwrap();
    let (answer, used) = frame::decode(&wire, Role::Client).unwrap().unwrap();
    assert_eq!((answer.opcode, &answer.payload[..2]), (frame::OPCODE_CLOSE, &1009u16.to_be_bytes()[..]));
    assert_eq!(used, wire.len(), "one Close alone");
}

/// A stand-in server on the loopback: each upgrade that carries `wanted` in
/// `Authorization` (or any, when `wanted` is none) is echoed; another gets
/// 401. It counts nothing.
async fn stand_in(wanted: Option<&'static str>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            tokio::spawn(echo(tcp, wanted));
        }
    });
    port
}

async fn echo(mut tcp: TcpStream, wanted: Option<&'static str>) {
    let Ok((head, pending)) = pod_proxy::http::read_request(&mut tcp, Vec::new()).await else { return };
    let Ok(key) = server::check_upgrade(&head) else { return };
    if wanted.is_some_and(|w| head.header("authorization") != Some(w)) {
        let _ = tcp.write_all(&pod_proxy::http::request::refusal(401, "a login", &[])).await;
        return;
    }
    if tcp.write_all(&server::switching(&key)).await.is_err() {
        return;
    }
    let session = server::session(tcp, pending, Some(LIMIT));
    while let Ok(f) = session.read_frame().await {
        if f.opcode == frame::OPCODE_CLOSE || session.send_binary(&f.payload).await.is_err() {
            break;
        }
    }
}

/// podnet: the credential of a tunnel with a login goes in
/// `Authorization`; a dial without it gets the server's 401 as a refusal.
#[tokio::test]
async fn the_dial_sends_the_authorization_and_a_401_is_a_refusal() {
    pod_testkit::need_loopback!();
    let port = stand_in(Some("Basic dXNlcjpwdw==")).await;
    let url = format!("ws://127.0.0.1:{port}/__podnet/ws");
    let way = common::way();
    let session =
        bridge::dial(&url, &way, &Trust::Default, LIMIT, Some("Basic dXNlcjpwdw==")).await.expect("a session");
    session.send_binary(b"ping").await.unwrap();
    assert_eq!(session.read_frame().await.unwrap().payload, b"ping");
    let Err(refused) = bridge::dial(&url, &way, &Trust::Default, LIMIT, None).await else { panic!("a refusal") };
    assert!(matches!(refused, ConnectError::Refused { status: 401, .. }), "{refused:?}");
    let failure = pod_core::Failure::from(refused);
    assert_eq!((failure.stage, failure.class), (pod_core::Stage::WebSocket, pod_core::Class::Refused));
    let Err(bad) = bridge::dial("http://127.0.0.1/", &way, &Trust::Default, LIMIT, None).await else {
        panic!("not a WebSocket's URL")
    };
    assert!(matches!(bad, ConnectError::Config(_)), "{bad:?}");
}

/// podnet's `connect_bridge`: a raw stream to a remote WebSocket, both
/// ways, until the raw stream ends.
#[tokio::test]
async fn connect_bridge_carries_a_raw_stream_to_the_websocket() {
    pod_testkit::need_loopback!();
    let port = stand_in(None).await;
    let url = format!("ws://127.0.0.1:{port}/");
    let (raw, mut local) = tokio::io::duplex(1 << 16);
    let task = tokio::spawn(async move {
        let way = common::way();
        let io: BoxedIo = Box::new(raw);
        bridge::connect_bridge(&url, &way, &Trust::Default, io, None, &pod_ws::NoTally).await
    });
    local.write_all(b"through the echo").await.unwrap();
    let mut got = [0u8; 16];
    tokio::time::timeout(LIMIT, local.read_exact(&mut got)).await.expect("the echo").unwrap();
    assert_eq!(&got, b"through the echo");
    local.shutdown().await.unwrap();
    drop(local);
    tokio::time::timeout(LIMIT, task).await.expect("the bridge ends").unwrap().expect("a clean end");
}

mod bench {
    use std::time::{Duration, Instant};

    use pod_ws::{RelaySession, bridge, server};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    /// One direction of a delay line: each chunk goes out `delay` after it
    /// came in.
    async fn delay_pipe(
        mut r: tokio::net::tcp::OwnedReadHalf,
        mut w: tokio::net::tcp::OwnedWriteHalf,
        delay: Duration,
    ) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(Instant, Vec<u8>)>();
        let reader = async move {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = match r.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let _ = tx.send((Instant::now() + delay, buf[..n].to_vec()));
            }
        };
        let writer = async move {
            while let Some((at, chunk)) = rx.recv().await {
                tokio::time::sleep_until(at.into()).await;
                if w.write_all(&chunk).await.is_err() {
                    break;
                }
            }
            let _ = w.shutdown().await;
        };
        tokio::join!(reader, writer);
    }

    /// A TCP listener that forwards to `to` through a delay line.
    async fn delay_line(to: std::net::SocketAddr, delay: Duration) -> std::net::SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((a, _)) = l.accept().await {
                let b = TcpStream::connect(to).await.unwrap();
                let (ar, aw) = a.into_split();
                let (br, bw) = b.into_split();
                tokio::spawn(delay_pipe(ar, bw, delay));
                tokio::spawn(delay_pipe(br, aw, delay));
            }
        });
        addr
    }

    const SIZE: usize = 16 << 20;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "a measure: it prints the throughput through a delay line and asserts nothing (cargo test -- --ignored throughput)"]
    async fn throughput() {
        let delay = Duration::from_millis(25);
        // Raw TCP through the delay line.
        let sink = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let sink_addr = sink.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = sink.accept().await.unwrap();
            s.write_all(&vec![7u8; SIZE]).await.unwrap();
        });
        let via = delay_line(sink_addr, delay).await;
        let t = Instant::now();
        let mut c = TcpStream::connect(via).await.unwrap();
        let mut got = Vec::new();
        c.read_to_end(&mut got).await.unwrap();
        eprintln!("raw tcp: {} bytes in {:?}", got.len(), t.elapsed());

        // The bridge: origin -> server bridge -> delay line -> client bridge -> local.
        let ws_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_addr = ws_l.local_addr().unwrap();
        tokio::spawn(async move {
            let (s, _) = ws_l.accept().await.unwrap();
            let session = server::session(s, Vec::new(), None).with_limits(pod_ws::Limits::TUNGSTENITE);
            let (origin_a, mut origin_b) = tokio::io::duplex(1 << 16);
            tokio::spawn(async move {
                origin_b.write_all(&vec![7u8; SIZE]).await.unwrap();
                origin_b.shutdown().await.unwrap();
                let mut sink = Vec::new();
                let _ = origin_b.read_to_end(&mut sink).await;
            });
            let _ = bridge::bridge(session, Box::new(origin_a), &pod_ws::NoTally).await;
        });
        let via = delay_line(ws_addr, delay).await;
        let (local_a, mut local_b) = tokio::io::duplex(1 << 16);
        let t = Instant::now();
        tokio::spawn(async move {
            let s = TcpStream::connect(via).await.unwrap();
            let session = RelaySession::new(s, Vec::new(), None, Duration::from_secs(60));
            let _ =
                bridge::bridge(session.with_limits(pod_ws::Limits::TUNGSTENITE), Box::new(local_a), &pod_ws::NoTally)
                    .await;
        });
        let mut got = vec![0u8; SIZE];
        local_b.read_exact(&mut got).await.unwrap();
        eprintln!("ws bridge: {} bytes in {:?}", got.len(), t.elapsed());
    }
}
