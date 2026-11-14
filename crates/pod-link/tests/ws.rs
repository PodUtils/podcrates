//! The ws carrier (T-420, PodUtils/podnet#515): a link over a WebSocket
//! with TLS carries streams; the dial goes through the way out of the
//! caller; the accept gives the key of the client; another key is refused
//! on each side and named; a silent peer holds no other one and is cut; a
//! silent listener ends the dial at its time limit; an address out of its
//! form is a usage error. The rules of podnet's carrier (T-506,
//! PodUtils/podnet@T-269): the subprotocol of both ends; a text message ends
//! the link; a peer that answers no Ping ends the session.

#![cfg(feature = "ws")]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::tcp::{Direct, Fails, id, ring};
use common::ws::{at, bound, carrier, pumped};
use common::{echo, limit, pattern};
use pod_core::{Class, Target};
use pod_identity::KeyName;
use pod_link::tcp::{Refusal, refusal};
use pod_link::ws::{self, Beat, Ws};
use pod_link::{Carrier, CarrierErrorKind, Facts, Link, Listener, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_a_websocket_carries_streams() {
    pod_testkit::need_loopback!();
    let way = Arc::new(Direct::default());
    let (server, client) = (id(1), id(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let dialing = carrier(2, way.clone(), &[]);
    let mut door = bound(&listening).await;
    assert_eq!(door.address(), door.local_addr().to_string());
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_peer()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    let (near, far) = (near.expect("the dial"), far.expect("the accept"));
    assert_eq!(far.peer, Some(client.public()), "the accept gives the key of the client");
    assert_eq!(way.dials.load(Ordering::SeqCst), 1, "the dial goes through the way out of the caller");
    assert_eq!(*way.last.lock().unwrap(), Some(Target::Addr(door.local_addr())));

    let dialer = Link::new(near, Role::Dialer, Settings::default()).unwrap();
    let _echo = echo(Link::new(far.io, Role::Listener, Settings::default()).unwrap());
    let mut moves = Vec::new();
    for seed in 0..3u32 {
        let mut stream = dialer.open().unwrap();
        moves.push(tokio::spawn(async move {
            let data = pattern(seed, 256 << 10);
            let (mut reader, mut writer) = tokio::io::split(&mut stream);
            let send = async {
                writer.write_all(&data).await.unwrap();
                writer.shutdown().await.unwrap();
            };
            let mut back = Vec::new();
            let (_, read) = tokio::join!(send, reader.read_to_end(&mut back));
            read.unwrap();
            assert!(back == data, "stream {seed}: the bytes came back whole");
        }));
    }
    for each in moves {
        timeout(limit(), each).await.expect("the echo in time").unwrap();
    }
    dialer.close().await;
}

#[tokio::test]
async fn the_connect_gives_the_key_of_the_listener_and_the_accept_the_key_of_the_client() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let dialing = carrier(2, Arc::new(Direct::default()), &[]);
    let mut door = bound(&listening).await;
    let (named, url) = Ws::address(&at(&server.public(), &door)).unwrap();
    let both = async { tokio::join!(dialing.connect(&url, &named), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    let ((mut near, seen), (mut far, key)) = (near.unwrap(), far.unwrap());
    assert_eq!((seen, key), (server.public(), client.public()));
    near.write_all(b"over a websocket").await.unwrap();
    near.shutdown().await.unwrap();
    let mut got = Vec::new();
    timeout(limit(), far.read_to_end(&mut got)).await.unwrap().unwrap();
    assert_eq!(got, b"over a websocket");
}

#[tokio::test]
async fn the_dialer_refuses_a_listener_of_another_key_and_names_it() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(3));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let dialing = carrier(2, Arc::new(Direct::default()), &[]);
    let mut door = bound(&listening).await;
    let address = at(&stranger.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (e, far) = timeout(limit(), both).await.expect("the refusal in time");
    let e = e.err().expect("a dial took a listener of another key");
    assert_eq!(e.kind, CarrierErrorKind::Refused, "{e}");
    assert_eq!(e.class().exit_code(), 77);
    assert_eq!(refusal(&e), Some(Refusal::Unpinned(server.public())), "{e}");
    assert!(e.to_string().contains(&server.public().fingerprint()), "the error names the key seen: {e}");
    let far = far.err().expect("the listener took a dialer that refused its key");
    assert_eq!(far.kind, CarrierErrorKind::KeyRefused, "the listener learns that its key was refused: {far}");
    assert_eq!(far.class().exit_code(), 77);
}

#[tokio::test]
async fn the_listener_refuses_a_client_of_another_key_and_stays_open() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(4));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);
    let other = carrier(4, Arc::new(Direct::default()), &[]);
    let near = other.dial(&address).await.err().expect("a client of another key dialed");
    assert_eq!(near.kind, CarrierErrorKind::KeyRefused, "the dialer learns of the refusal at the dial: {near}");
    assert_eq!(near.class().exit_code(), 77);
    assert!(near.to_string().contains(&stranger.public().fingerprint()), "{near}");

    let good = carrier(2, Arc::new(Direct::default()), &[]);
    let both = async { tokio::join!(good.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    near.expect("the pinned client after a refusal");
    assert_eq!(far.expect("the listener stays open").1, client.public(), "the refused client reached no accept");
}

#[tokio::test]
async fn a_peer_that_sends_nothing_holds_no_other_and_is_cut_at_its_deadline() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let mut listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    listening.serve.deadlines.tls = Duration::from_millis(300);
    let dialing = carrier(2, Arc::new(Direct::default()), &[]);
    let mut door = bound(&listening).await;
    let mut silent = tokio::net::TcpStream::connect(door.local_addr()).await.unwrap();
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(Duration::from_secs(20), both).await.expect("the good client is not held");
    near.unwrap();
    assert_eq!(far.unwrap().1, client.public());
    let mut byte = [0u8; 1];
    let end = timeout(limit(), silent.read(&mut byte)).await.expect("the deadline of TLS cuts a silent peer");
    assert!(matches!(end, Ok(0) | Err(_)), "{end:?}");
}

#[tokio::test]
async fn a_silent_listener_ends_the_dial_at_its_time_limit() {
    pod_testkit::need_loopback!();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = listener.local_addr().unwrap();
    let _hold = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((tcp, _)) = listener.accept().await {
            held.push(tcp);
        }
    });
    let mut dialing = carrier(2, Arc::new(Direct::default()), &[]);
    dialing.handshake_timeout = Duration::from_millis(300);
    let address = format!("{}@wss://{local}/link", id(1).public().hex());
    let e = timeout(limit(), dialing.dial(&address)).await.expect("the time limit of the dial").err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{e}");
    assert_eq!(e.class(), Class::Timeout);
}

#[tokio::test]
async fn an_address_out_of_its_form_is_a_usage_error() {
    let ws = carrier(1, Arc::new(Direct::default()), &[]);
    let key = id(2).public().hex();
    for bad in [
        "wss://127.0.0.1:7000/link".to_string(),
        "nokey@wss://127.0.0.1:7000/".into(),
        format!("{key}@ws://127.0.0.1:7000/link"),
        format!("{key}@127.0.0.1:7000"),
        format!("{key}@wss://127.0.0.1:0/"),
    ] {
        let e = ws.dial(&bad).await.err().unwrap_or_else(|| panic!("{bad} dialed"));
        assert_eq!(e.kind, CarrierErrorKind::Usage, "{bad}: {e}");
        assert_eq!(e.class().exit_code(), 64, "{bad}");
    }
    for bad in ["nowhere", "localhost:7000", "127.0.0.1"] {
        assert_eq!(ws.listen(bad).await.err().unwrap().kind, CarrierErrorKind::Usage, "{bad}");
    }
    let (named, url) = Ws::address(&format!("{}@wss://relay.example/l?x=1", id(2).public().fingerprint())).unwrap();
    assert!(matches!(named, KeyName::Fingerprint(_)) && named.matches(&id(2).public()));
    assert_eq!((url.host.as_str(), url.port, url.target.as_str()), ("relay.example", 443, "/l?x=1"));
    let mut none = ws.clone();
    none.serve.caps.max_unauth = 0;
    assert_eq!(none.listen("127.0.0.1:0").await.err().unwrap().kind, CarrierErrorKind::Usage);
}

#[tokio::test]
async fn a_failure_of_the_way_out_keeps_its_class_and_the_carrier_says_why_it_cannot_run() {
    let key = id(1).public().hex();
    for (class, kind) in [
        (Class::Refused, CarrierErrorKind::Refused),
        (Class::NotFound, CarrierErrorKind::NotFound),
        (Class::Unavailable, CarrierErrorKind::Unavailable),
    ] {
        let ws = carrier(2, Arc::new(Fails(class)), &[]);
        let e = ws.dial(&format!("{key}@wss://192.0.2.1:7000/")).await.err().unwrap();
        assert_eq!(e.kind, kind, "{class:?}: {e}");
        assert!(e.to_string().contains("test-fails"), "the error names the way out: {e}");
    }
    let ws = Ws::new(ring(), id(1), Arc::new(Direct::default()));
    assert_eq!((ws.name(), ws.facts()), ("ws", Facts::STREAM));
    if pod_testkit::host::loopback().is_ok() {
        assert!(ws.available().is_ok(), "{:?}", ws.available());
    }
    assert_eq!((ws::NAME, ws::ALPN), ("ws", &b"http/1.1"[..]));
    assert_eq!(ws.connect_timeout, ws::CONNECT_TIMEOUT);
    assert_eq!(ws.handshake_timeout, ws::HANDSHAKE_TIMEOUT);
    let shown = format!("{ws:?}");
    assert!(shown.contains(&id(1).public().fingerprint()) && shown.contains("test-direct"), "{shown}");
}

#[tokio::test]
async fn the_subprotocol_of_both_ends_is_offered_and_named() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]).with_protocol("podnet/1");
    let dialing = carrier(2, Arc::new(Direct::default()), &[]).with_protocol("podnet/1");
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    let (mut near, (mut far, _)) = (near.expect("the dial that offers the subprotocol"), far.unwrap());
    near.write_all(b"podnet/1").await.unwrap();
    let mut got = [0u8; 8];
    timeout(limit(), far.read_exact(&mut got)).await.unwrap().unwrap();
    assert_eq!(&got, b"podnet/1");
}

#[tokio::test]
async fn a_dialer_with_no_subprotocol_or_another_gets_426() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]).with_protocol("podnet/1");
    let door = bound(&listening).await;
    let address = at(&server.public(), &door);
    for dialing in [
        carrier(2, Arc::new(Direct::default()), &[]),
        carrier(2, Arc::new(Direct::default()), &[]).with_protocol("podnet/2"),
    ] {
        let e = timeout(limit(), dialing.dial(&address)).await.expect("the refusal in time").err().expect("a dial");
        assert_eq!(e.kind, CarrierErrorKind::Io(std::io::ErrorKind::InvalidData), "{e}");
        assert!(e.to_string().contains("426"), "{:?}: {e}", dialing.protocol);
    }

    // A listener with no subprotocol answers with none: a dialer that
    // offered one refuses the 101.
    let plain = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let door = bound(&plain).await;
    let dialing = carrier(2, Arc::new(Direct::default()), &[]).with_protocol("podnet/1");
    let e = timeout(limit(), dialing.dial(&at(&server.public(), &door))).await.unwrap().err().expect("a 101 with none");
    assert!(e.to_string().contains("named no subprotocol"), "{e}");

    let bad = carrier(2, Arc::new(Direct::default()), &[]).with_protocol("two words");
    assert_eq!(bad.check().unwrap_err().kind, CarrierErrorKind::Usage);
    assert_eq!(bad.listen("127.0.0.1:0").await.err().unwrap().kind, CarrierErrorKind::Usage);
    let mut zero = carrier(2, Arc::new(Direct::default()), &[]);
    zero.beat.pong_within = Duration::ZERO;
    assert_eq!(zero.dial(&address).await.err().unwrap().kind, CarrierErrorKind::Usage);
    assert_eq!(Beat::default(), Beat { ping_every: Some(ws::PING_EVERY), pong_within: ws::PONG_WITHIN });
    assert_eq!((ws::PING_EVERY, ws::PONG_WITHIN), (Duration::from_secs(20), Duration::from_secs(10)));
}

#[tokio::test]
async fn a_text_message_ends_the_link_with_an_error_of_protocol() {
    let (mut io, peer) = pumped(Beat::default());
    peer.send_binary(b"binary").await.unwrap();
    let mut got = [0u8; 6];
    timeout(limit(), io.read_exact(&mut got)).await.unwrap().unwrap();
    assert_eq!(&got, b"binary", "a binary message goes to the link");
    peer.send_text("text").await.unwrap();
    let e =
        timeout(limit(), io.read(&mut got)).await.expect("the end in time").expect_err("a text message is an error");
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidData, "{e}");
    assert!(e.to_string().contains("text message"), "{e}");
    let close = timeout(limit(), peer.read_frame()).await.unwrap().unwrap();
    assert_eq!(close.opcode, pod_ws::frame::OPCODE_CLOSE);
    assert_eq!(pod_ws::close::close_code_and_reason(&close.payload).0, Some(pod_ws::close::code::UNSUPPORTED));

    let (io, peer) = pumped(Beat::default());
    let link = Link::new(io, Role::Dialer, Settings::default()).unwrap();
    peer.send_text("{}").await.unwrap();
    match timeout(limit(), link.closed()).await.expect("the end of the link in time") {
        pod_link::Error::Io { kind, text } => {
            assert_eq!(kind, std::io::ErrorKind::InvalidData, "{text}");
            assert!(text.contains("text message"), "{text}");
        }
        other => panic!("the link ended with {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn a_peer_that_answers_no_ping_ends_the_session() {
    let (mut io, peer) = pumped(Beat::default());
    let start = tokio::time::Instant::now();
    let mut byte = [0u8; 1];
    let e = timeout(Duration::from_secs(300), io.read(&mut byte)).await.expect("the end in time").unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
    assert_eq!(start.elapsed(), Duration::from_secs(30), "the Ping at 20 s, its wait of 10 s");
    drop(peer);
}

/// The reads of the peer, which answer its Pings: the data messages.
fn answering(
    peer: Arc<pod_ws::RelaySession<tokio::io::DuplexStream>>,
) -> tokio::sync::mpsc::UnboundedReceiver<Vec<u8>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok(message) = peer.read_frame().await {
            let _ = tx.send(message.payload);
        }
    });
    rx
}

#[tokio::test(start_paused = true)]
async fn a_peer_that_answers_each_ping_keeps_the_session() {
    let (mut io, peer) = pumped(Beat::default());
    let mut data = answering(peer.clone());
    tokio::time::sleep(Duration::from_secs(95)).await;
    assert_eq!(peer.frames_received(), 4, "a Ping at 20, 40, 60 and 80 s");
    io.write_all(b"up").await.unwrap();
    assert_eq!(timeout(limit(), data.recv()).await.unwrap().unwrap(), b"up");
    peer.send_binary(b"down").await.unwrap();
    let mut got = [0u8; 4];
    timeout(limit(), io.read_exact(&mut got)).await.unwrap().unwrap();
    assert_eq!(&got, b"down");

    let (mut io, peer) = pumped(Beat { ping_every: None, ..Beat::default() });
    tokio::time::sleep(Duration::from_secs(95)).await;
    assert_eq!(peer.frames_received(), 0, "no Ping");
    peer.send_binary(b"down").await.unwrap();
    timeout(limit(), io.read_exact(&mut got)).await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_message_that_waits_for_the_link_keeps_the_session() {
    let (mut io, peer) = pumped(Beat::default());
    let _data = answering(peer.clone());
    let sent = pattern(7, 512 << 10);
    for part in sent.chunks(16 << 10) {
        peer.send_binary(part).await.unwrap();
    }
    // The link reads nothing for 45 s: the pump waits on it, not on the peer.
    tokio::time::sleep(Duration::from_secs(45)).await;
    let mut got = vec![0u8; sent.len()];
    timeout(limit(), io.read_exact(&mut got)).await.expect("the bytes in time").expect("no error");
    assert!(got == sent, "the bytes came whole");
    peer.send_binary(b"after").await.unwrap();
    let mut after = [0u8; 5];
    timeout(limit(), io.read_exact(&mut after)).await.unwrap().expect("the session lives");
    assert_eq!(&after, b"after");
}
