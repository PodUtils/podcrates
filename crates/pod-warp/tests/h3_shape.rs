//! The conformance of the HTTP/3 server shape (PodUtils/podnet#486,
//! QUIC-6): a client of this workspace sends a GET and a POST to a listener
//! of the common shape, and the log of the listener shows the `h3` ALPN,
//! the control stream and its SETTINGS, and the status of each answer.
//!
//! A stock HTTP/3 client is not in the workspace or in its cache, so the
//! client here is the workspace's own `pod-quic` stack with a request of
//! the static QPACK table. What this test proves: the listener negotiates
//! `h3`, sends a control stream whose first frame is SETTINGS with the
//! pairs of the shape before any answer, answers a GET with 200 and a body
//! and a POST with 405, and records each of those events in its log. What
//! it does not prove: interoperation with the QUIC or QPACK implementation
//! of another project, a request that uses the QPACK dynamic table or a
//! Huffman coded literal (the server refuses the first and skips the
//! second), and the behaviour of the shape under loss.

mod common;

use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Datagrams};
use pod_quic::noq;
use pod_warp::Identity;
use pod_warp::h3::shape::{Profile, Shape, ShapeReport};
use pod_warp::h3::wire::{self, FRAME_HEADERS};
use pod_warp::h3::{self, Frames, Listener, ShapeEvent};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;

/// The certificate of the listener, made for each handshake.
#[derive(Debug)]
struct Shown(Identity);

impl ResolvesServerCert for Shown {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        Some(Arc::new(self.0.certified(now)))
    }
}

/// What the client saw, in order.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    ControlStream(u64),
    ServerSettings(Vec<(u64, u64)>),
    Status(u16),
    Body(usize),
}

/// The server crypto of the listener, and the key that the client pins.
fn crypto() -> (Arc<dyn noq::crypto::ServerConfig>, Vec<u8>) {
    let id = Identity::generate().unwrap();
    let pin = id.spki_der().to_vec();
    let mut tls = rustls::ServerConfig::builder_with_provider(common::provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Shown(id)));
    tls.alpn_protocols = vec![h3::ALPN.to_vec()];
    (Arc::new(noq::crypto::rustls::QuicServerConfig::try_from(Arc::new(tls)).unwrap()), pin)
}

/// The field section of a GET or a POST of `path` to `authority`.
fn headers(method: u8, path: &str, authority: &str) -> Vec<u8> {
    let mut out = vec![0x00, 0x00, 0xc0 | method, 0xc0 | 23];
    if path == "/" {
        out.push(0xc0 | 1);
    } else {
        out.push(0x50 | 1);
        out.push(u8::try_from(path.len()).unwrap());
        out.extend_from_slice(path.as_bytes());
    }
    out.push(0x50);
    out.push(u8::try_from(authority.len()).unwrap());
    out.extend_from_slice(authority.as_bytes());
    out
}

/// The handshake and the client's control stream, then the server's
/// control stream read up to its SETTINGS. The endpoint and the receive
/// side are returned still alive, so that a caller can see whether the
/// server closes the control stream after SETTINGS.
async fn open_client(
    door: std::net::SocketAddr,
    pin: &[u8],
) -> (noq::Endpoint, noq::Connection, noq::RecvStream, Vec<Seen>) {
    let settings = pod_quic::Settings { handshake_wait: Duration::from_secs(10), ..pod_quic::Settings::default() };
    let client = Identity::generate().unwrap();
    let tls = pod_warp::tls::client_config(common::provider(), &client, pin.to_vec(), h3::ALPN).unwrap();
    let datagrams: Arc<dyn Datagrams> =
        Arc::new(pod_core::SystemDatagrams::bind("0.0.0.0:0".parse().unwrap()).await.unwrap());
    let endpoint = pod_quic::endpoint::over(datagrams, None).unwrap();
    let config = pod_quic::connect::client_config(tls, &settings, false).unwrap();
    let conn =
        tokio::time::timeout(Duration::from_secs(10), endpoint.connect_with(config, door, "shape.test").unwrap())
            .await
            .expect("the handshake")
            .unwrap();
    // The client's own control stream.
    let mut control = conn.open_uni().await.unwrap();
    let mut first = vec![0x00];
    let mut pairs = Vec::new();
    for (id, value) in h3::SETTINGS {
        wire::put_varint(&mut pairs, *id);
        wire::put_varint(&mut pairs, *value);
    }
    first.extend_from_slice(&h3::frame(h3::FRAME_SETTINGS, &pairs));
    control.write_all(&first).await.unwrap();
    // The server's control stream and SETTINGS.
    let mut events = Vec::new();
    let mut uni = tokio::time::timeout(Duration::from_secs(10), conn.accept_uni()).await.expect("a stream").unwrap();
    let mut buf = Vec::new();
    let mut frames = Frames::default();
    loop {
        if let Some((kind, payload)) = frames.next_frame().unwrap() {
            if kind == h3::FRAME_SETTINGS {
                events.push(Seen::ServerSettings(wire::settings_pairs(&payload).unwrap()));
                break;
            }
            continue;
        }
        match uni.read_chunk(65536).await {
            Ok(Some(chunk)) => {
                if buf.is_empty() {
                    let (kind, used) = wire::get_varint(&chunk).expect("the stream type");
                    events.push(Seen::ControlStream(kind));
                    frames.push(&chunk[used..]);
                    buf.push(0);
                } else {
                    frames.push(&chunk);
                }
            }
            _ => break,
        }
    }
    (endpoint, conn, uni, events)
}

/// One request on `conn`: the status and the body.
async fn request_on(conn: &noq::Connection, method: u8, path: &str) -> (u16, Vec<u8>) {
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    send.write_all(&h3::frame(FRAME_HEADERS, &headers(method, path, "shape.test"))).await.unwrap();
    let mut frames = Frames::default();
    let status = loop {
        if let Some((kind, payload)) = frames.next_frame().unwrap() {
            if kind == FRAME_HEADERS {
                break wire::response_status(&payload).unwrap();
            }
            continue;
        }
        match recv.read_chunk(65536).await {
            Ok(Some(chunk)) => frames.push(&chunk),
            _ => panic!("the answer of the shape"),
        }
    };
    let mut body = Vec::new();
    loop {
        if let Some((kind, payload)) = frames.next_frame().unwrap() {
            if kind == h3::FRAME_DATA {
                body.extend_from_slice(&payload);
            }
            continue;
        }
        match recv.read_chunk(65536).await {
            Ok(Some(chunk)) => frames.push(&chunk),
            _ => break,
        }
    }
    (status, body)
}

/// A request of the workspace's own HTTP/3 client: the status, the body,
/// and its events.
async fn request(door: std::net::SocketAddr, pin: &[u8], method: u8, path: &str) -> (u16, Vec<u8>, Vec<Seen>) {
    let (_endpoint, conn, _control, mut events) = open_client(door, pin).await;
    let (status, body) = request_on(&conn, method, path).await;
    events.push(Seen::Status(status));
    if !body.is_empty() {
        events.push(Seen::Body(body.len()));
    }
    conn.close(0u32.into(), b"");
    (status, body, events)
}

/// Wait until the log holds `event`, within a limit.
async fn wait_for(log: &pod_warp::h3::ShapeLog, event: ShapeEvent) {
    for _ in 0..500 {
        if log.events().contains(&event) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{event:?} is not in {:?}", log.events());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_common_shape_serves_a_get_with_a_control_stream_and_settings() {
    pod_testkit::need_loopback!();
    let shape = Shape::parse("common").unwrap();
    assert_eq!(shape.profile, Profile::Cubic);
    assert_eq!(shape.name(), "common");
    let (crypto, pin) = crypto();
    let listener = Arc::new(
        Listener::bind("127.0.0.1:0".parse().unwrap(), crypto, shape.clone(), &pod_quic::Settings::default()).unwrap(),
    );
    let door = listener.local_addr().unwrap();
    let serving = tokio::spawn(listener.clone().serve_forever());
    let (status, body, events) = request(door, &pin, 17, "/").await;
    assert_eq!(status, 200);
    assert_eq!(body, pod_warp::h3::server::BODY);
    assert_eq!(
        events,
        vec![
            Seen::ControlStream(0),
            Seen::ServerSettings(shape.settings.pairs()),
            Seen::Status(200),
            Seen::Body(pod_warp::h3::server::BODY.len()),
        ]
    );
    // A method that the shape does not serve is 405.
    let (status, body, events) = request(door, &pin, 20, "/upload").await;
    assert_eq!((status, body.len()), (405, 0));
    assert!(events.contains(&Seen::Status(405)));
    // The log of the listener has the ALPN, the control stream, the SETTINGS
    // and each request, in order.
    wait_for(&listener.log(), ShapeEvent::Request { method: "POST".into(), path: "/upload".into(), status: 405 }).await;
    let log = listener.log().events();
    assert!(log.iter().any(|e| *e == ShapeEvent::Alpn { protocol: "h3".into() }), "{log:?}");
    assert!(log.contains(&ShapeEvent::ControlStream { kind: 0 }), "{log:?}");
    assert!(log.contains(&ShapeEvent::SettingsSent(shape.settings.pairs())), "{log:?}");
    assert!(log.contains(&ShapeEvent::Request { method: "GET".into(), path: "/".into(), status: 200 }), "{log:?}");
    assert!(
        log.contains(&ShapeEvent::Request { method: "POST".into(), path: "/upload".into(), status: 405 }),
        "{log:?}"
    );
    // The data of `doctor`: the congestion profile and the two observations.
    let report = ShapeReport::of(&shape, &listener.log());
    assert_eq!(report.congestion, "cubic");
    assert!(report.alpn_h3 && report.control_stream && report.settings, "{report:?}");
    assert_eq!(report.setting_ids, vec![0x01, 0x06, 0x08, 0x33]);
    assert_eq!(report.line(), "h3-shape=common congestion=cubic control-stream=yes settings=yes");
    serving.abort();
}

/// RFC 9114, section 6.2.1: the server does not close its control stream.
/// The client reads the SETTINGS, waits for a FIN and sees none, and the
/// connection still answers a request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_server_keeps_its_control_stream_open_after_settings() {
    pod_testkit::need_loopback!();
    let shape = Shape::parse("common").unwrap();
    let (crypto, pin) = crypto();
    let listener = Arc::new(
        Listener::bind("127.0.0.1:0".parse().unwrap(), crypto, shape.clone(), &pod_quic::Settings::default()).unwrap(),
    );
    let door = listener.local_addr().unwrap();
    let serving = tokio::spawn(listener.clone().serve_forever());
    let (_endpoint, conn, mut control, events) = open_client(door, &pin).await;
    assert_eq!(events, vec![Seen::ControlStream(0), Seen::ServerSettings(shape.settings.pairs())]);
    // A FIN on the control stream is an immediate `Ok(None)`; the server
    // must not send one, so this read stays pending until the timeout.
    let after = tokio::time::timeout(Duration::from_millis(300), control.read_chunk(65536)).await;
    assert!(after.is_err(), "the server closed its control stream: {after:?}");
    // The connection still serves a request.
    let (status, body) = request_on(&conn, 17, "/").await;
    assert_eq!(status, 200);
    assert_eq!(body, pod_warp::h3::server::BODY);
    conn.close(0u32.into(), b"");
    serving.abort();
}

/// A profile of the set is selected by name, and the report names it: the
/// data of `doctor` is the profile, not a guess.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_selected_profile_is_reported() {
    pod_testkit::need_loopback!();
    for (text, name) in [("cubic", "cubic"), ("newreno", "newreno"), ("bbr", "bbr")] {
        let shape = Shape::parse(text).unwrap();
        assert_eq!(shape.profile.name(), name);
        let (crypto, pin) = crypto();
        let listener = Arc::new(
            Listener::bind("127.0.0.1:0".parse().unwrap(), crypto, shape.clone(), &pod_quic::Settings::default())
                .unwrap(),
        );
        let door = listener.local_addr().unwrap();
        let serving = tokio::spawn(listener.clone().serve_forever());
        let (status, _, _) = request(door, &pin, 17, "/").await;
        assert_eq!(status, 200, "{name}");
        let report = ShapeReport::of(&shape, &listener.log());
        assert_eq!(report.congestion, name);
        assert_eq!(report.shape, shape.name());
        serving.abort();
    }
    // A server with no control stream is a negative shape: the report says
    // that the control stream and the SETTINGS were not observed.
    let shape = Shape::parse("cubic-no-control").unwrap();
    let report = ShapeReport::of(&shape, &pod_warp::h3::ShapeLog::default());
    assert!(!report.control_stream && !report.settings);
    assert_eq!(report.congestion, "cubic");
    assert_eq!(Class::Config, Shape::parse("fast").unwrap_err().class);
}
