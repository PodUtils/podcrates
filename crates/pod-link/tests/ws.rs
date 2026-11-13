//! The ws carrier (T-420, PodUtils/podnet#515): a link over a WebSocket
//! with TLS carries streams; the dial goes through the way out of the
//! caller; the accept gives the key of the client; another key is refused
//! on each side and named; a silent peer holds no other one and is cut; a
//! silent listener ends the dial at its time limit; an address out of its
//! form is a usage error.

#![cfg(feature = "ws")]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::tcp::{Direct, Fails, id, ring};
use common::ws::{at, bound, carrier};
use common::{LIMIT, echo, pattern};
use pod_core::{Class, Target};
use pod_identity::KeyName;
use pod_link::tcp::{Refusal, refusal};
use pod_link::ws::{self, Ws};
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
    let (near, far) = timeout(LIMIT, both).await.expect("the dial and the accept in time");
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
        timeout(LIMIT, each).await.expect("the echo in time").unwrap();
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
    let (near, far) = timeout(LIMIT, both).await.expect("the dial and the accept in time");
    let ((mut near, seen), (mut far, key)) = (near.unwrap(), far.unwrap());
    assert_eq!((seen, key), (server.public(), client.public()));
    near.write_all(b"over a websocket").await.unwrap();
    near.shutdown().await.unwrap();
    let mut got = Vec::new();
    timeout(LIMIT, far.read_to_end(&mut got)).await.unwrap().unwrap();
    assert_eq!(got, b"over a websocket");
}

#[tokio::test]
async fn the_dialer_refuses_a_listener_of_another_key_and_names_it() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(3));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let dialing = carrier(2, Arc::new(Direct::default()), &[]);
    let door = bound(&listening).await;
    let e = dialing.dial(&at(&stranger.public(), &door)).await.err().expect("a dial took a listener of another key");
    assert_eq!(e.kind, CarrierErrorKind::Refused, "{e}");
    assert_eq!(e.class().exit_code(), 77);
    assert_eq!(refusal(&e), Some(Refusal::Unpinned(server.public())), "{e}");
    assert!(e.to_string().contains(&server.public().fingerprint()), "the error names the key seen: {e}");
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
    assert_eq!(near.kind, CarrierErrorKind::Refused, "the dialer learns of the refusal at the dial: {near}");
    assert!(near.to_string().contains(&stranger.public().fingerprint()), "{near}");

    let good = carrier(2, Arc::new(Direct::default()), &[]);
    let both = async { tokio::join!(good.dial(&address), door.accept_key()) };
    let (near, far) = timeout(LIMIT, both).await.expect("the dial and the accept in time");
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
    let end = timeout(LIMIT, silent.read(&mut byte)).await.expect("the deadline of TLS cuts a silent peer");
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
    let e = timeout(LIMIT, dialing.dial(&address)).await.expect("the time limit of the dial").err().unwrap();
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
