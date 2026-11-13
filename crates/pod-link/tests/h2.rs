//! The h2 carrier (T-421, PodUtils/podnet#515): a link over one POST of
//! HTTP/2 with TLS carries streams; the dial goes through the way out of
//! the caller, also through a proxy that tunnels it, while the request body
//! stays open; the accept gives the key of the client; another key is
//! refused on each side and named; a request that is not a POST is
//! refused; a silent peer holds no other one; a silent listener ends the
//! dial at its time limit; an address out of its form is a usage error.

#![cfg(feature = "h2")]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::h2::{Tunnel, at, bound, carrier};
use common::tcp::{Direct, Fails, id, ring};
use common::{echo, limit, pattern};
use pod_core::{Class, Target};
use pod_identity::KeyName;
use pod_link::h2::{self, H2};
use pod_link::tcp::{Refusal, refusal};
use pod_link::{Carrier, CarrierErrorKind, Facts, Link, Listener, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_one_post_of_http2_carries_streams() {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dial_through_a_proxy_moves_bytes_both_ways_while_the_post_is_open() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let mut door = bound(&listening).await;
    let proxy = Tunnel::start(door.local_addr()).await;
    let dialing = carrier(2, proxy.way(), &[]);
    // A name, so that the way out sends it to the proxy and resolves nothing.
    let address = format!("{}@https://link.example/link", server.public().hex());
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    let (mut near, (mut far, key)) = (near.expect("the dial through the proxy"), far.expect("the accept"));
    assert_eq!(key, client.public());
    assert_eq!(proxy.tunnels.load(Ordering::SeqCst), 1, "the dial went through the proxy");
    // Each line goes and its answer comes back before the next line: a
    // body that waited for its end would hold the first answer.
    for n in 0..50u32 {
        let line = format!("line {n}\n");
        near.write_all(line.as_bytes()).await.unwrap();
        let mut got = vec![0u8; line.len()];
        timeout(limit(), far.read_exact(&mut got)).await.expect("the request body moves before its end").unwrap();
        far.write_all(&got).await.unwrap();
        let mut back = vec![0u8; line.len()];
        timeout(limit(), near.read_exact(&mut back)).await.expect("the response body moves before its end").unwrap();
        assert_eq!(back, line.as_bytes());
    }
    near.shutdown().await.unwrap();
    let mut rest = Vec::new();
    timeout(limit(), far.read_to_end(&mut rest)).await.expect("the end of the request body").unwrap();
    assert!(rest.is_empty(), "the shutdown of the dialer is the end of the request body");
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
    let both = async { tokio::join!(other.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the refusal in time");
    let near = near.err().expect("a client of another key dialed");
    assert_eq!(near.kind, CarrierErrorKind::Refused, "the dialer learns of the refusal at the dial: {near}");
    assert!(near.to_string().contains(&stranger.public().fingerprint()), "{near}");
    let far = far.err().expect("the listener took a client of another key");
    assert_eq!(far.kind, CarrierErrorKind::Refused, "{far}");
    assert_eq!(refusal(&far), Some(Refusal::Unpinned(stranger.public())), "{far}");

    let good = carrier(2, Arc::new(Direct::default()), &[]);
    let both = async { tokio::join!(good.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    near.expect("the pinned client after a refusal");
    assert_eq!(far.expect("the listener stays open").1, client.public());
}

#[tokio::test]
async fn a_request_that_is_not_a_post_is_refused() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let mut door = bound(&listening).await;
    let local = door.local_addr();
    let get = async {
        let config =
            pod_tls::peer::client_config(ring(), client.clone(), KeyName::Key(server.public()), &[h2::ALPN]).unwrap();
        let tcp = tokio::net::TcpStream::connect(local).await.unwrap();
        let name = rustls::pki_types::ServerName::IpAddress(local.ip().into());
        let tls = tokio_rustls::TlsConnector::from(config).connect(name, tcp).await.unwrap();
        let (mut ask, connection) = ::h2::client::handshake(tls).await.unwrap();
        tokio::spawn(connection);
        let request = http::Request::get(format!("https://{local}/link")).body(()).unwrap();
        let (answer, _) = ask.send_request(request, true).unwrap();
        answer.await.unwrap().status()
    };
    let (status, far) = timeout(limit(), async { tokio::join!(get, door.accept_key()) }).await.expect("in time");
    assert_eq!(status, http::StatusCode::METHOD_NOT_ALLOWED);
    let e = far.err().expect("the listener took a GET");
    assert!(e.to_string().contains("not POST"), "{e}");
}

#[tokio::test]
async fn a_peer_that_sends_nothing_holds_no_other_handshake() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let mut listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    listening.handshake_timeout = Duration::from_secs(300);
    let dialing = carrier(2, Arc::new(Direct::default()), &[]);
    let mut door = bound(&listening).await;
    let _silent = tokio::net::TcpStream::connect(door.local_addr()).await.unwrap();
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(Duration::from_secs(20), both).await.expect("the good client is not held");
    near.unwrap();
    assert_eq!(far.unwrap().1, client.public());

    let mut quick = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    quick.handshake_timeout = Duration::from_millis(300);
    let mut door = bound(&quick).await;
    let _silent = tokio::net::TcpStream::connect(door.local_addr()).await.unwrap();
    let e = timeout(limit(), door.accept_key()).await.expect("the limit of the handshake").err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{e}");
    assert_eq!(e.class(), Class::Timeout);
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
    let address = format!("{}@https://{local}/link", id(1).public().hex());
    let e = timeout(limit(), dialing.dial(&address)).await.expect("the time limit of the dial").err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{e}");
    assert_eq!(e.class(), Class::Timeout);
}

#[tokio::test]
async fn an_address_out_of_its_form_is_a_usage_error() {
    let carrier_ = carrier(1, Arc::new(Direct::default()), &[]);
    let key = id(2).public().hex();
    for bad in [
        "https://127.0.0.1:7000/link".to_string(),
        "nokey@https://127.0.0.1:7000/".into(),
        format!("{key}@http://127.0.0.1:7000/link"),
        format!("{key}@wss://127.0.0.1:7000/link"),
        format!("{key}@127.0.0.1:7000"),
        format!("{key}@https://127.0.0.1:0/"),
    ] {
        let e = carrier_.dial(&bad).await.err().unwrap_or_else(|| panic!("{bad} dialed"));
        assert_eq!(e.kind, CarrierErrorKind::Usage, "{bad}: {e}");
        assert_eq!(e.class().exit_code(), 64, "{bad}");
    }
    for bad in ["nowhere", "localhost:7000", "127.0.0.1"] {
        assert_eq!(carrier_.listen(bad).await.err().unwrap().kind, CarrierErrorKind::Usage, "{bad}");
    }
    let (named, url) = H2::address(&format!("{}@HTTPS://[::1]:8443/l?x=1", id(2).public().fingerprint())).unwrap();
    assert!(matches!(named, KeyName::Fingerprint(_)) && named.matches(&id(2).public()));
    assert_eq!((url.host.as_str(), url.port, url.path.as_str()), ("::1", 8443, "/l?x=1"));
    assert_eq!(url.authority(), "[::1]:8443");
    assert_eq!(url.target(), Target::Addr("[::1]:8443".parse().unwrap()));
    let mut none = carrier_.clone();
    none.max_handshakes = 0;
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
        let h2 = carrier(2, Arc::new(Fails(class)), &[]);
        let e = h2.dial(&format!("{key}@https://192.0.2.1:7000/")).await.err().unwrap();
        assert_eq!(e.kind, kind, "{class:?}: {e}");
        assert!(e.to_string().contains("test-fails"), "the error names the way out: {e}");
    }
    let h2 = H2::new(ring(), id(1), Arc::new(Direct::default()));
    assert_eq!((h2.name(), h2.facts()), ("h2", Facts::STREAM));
    if pod_testkit::host::loopback().is_ok() {
        assert!(h2.available().is_ok(), "{:?}", h2.available());
    }
    assert_eq!((h2::NAME, h2::ALPN, h2::MAX_HANDSHAKES), ("h2", &b"h2"[..], 64));
    assert_eq!((h2.connect_timeout, h2.handshake_timeout), (h2::CONNECT_TIMEOUT, h2::HANDSHAKE_TIMEOUT));
    let shown = format!("{h2:?}");
    assert!(shown.contains(&id(1).public().fingerprint()) && shown.contains("test-direct"), "{shown}");
}
