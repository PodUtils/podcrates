//! The tcp carrier (T-396): a link over TCP with TLS 1.3 carries streams;
//! the dial goes through the way out of the caller; the accept gives the
//! key of the client; a key is named in each form of `KeyName`; the
//! carrier says why a host or a provider cannot use it; an address out of
//! its form is a usage error; a peer that sends nothing holds no other
//! handshake; both ends take the ALPN of the carrier. The refusals are in
//! `tests/tcp_refused.rs`.

#![cfg(feature = "tcp")]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::tcp::{Direct, Fails, at, bound, carrier, id, ring};
use common::{echo, limit, pattern};
use pod_core::{Class, Target};
use pod_identity::KeyName;
use pod_link::tcp::{self, Tcp};
use pod_link::{Carrier, CarrierErrorKind, Facts, Link, Listener, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_tcp_with_tls_carries_streams() {
    pod_testkit::need_loopback!();
    let way = Arc::new(Direct::default());
    let (server, client) = (id(1), id(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    let dialing = carrier(2, way.clone(), &[]);
    let mut door = bound(&listening).await;
    assert_eq!(door.address(), door.local_addr().to_string());
    let address = at(&server.public(), &door);
    let (near, far) = tokio::join!(dialing.dial(&address), door.accept_peer());
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
    let target = Target::Addr(door.local_addr());
    let named = KeyName::Key(server.public());
    let (near, far) = tokio::join!(dialing.connect(&target, &named), door.accept_key());
    let ((mut near, seen), (mut far, key)) = (near.unwrap(), far.unwrap());
    assert_eq!((seen, key), (server.public(), client.public()));
    near.write_all(b"over tls").await.unwrap();
    near.shutdown().await.unwrap();
    let mut got = Vec::new();
    timeout(limit(), far.read_to_end(&mut got)).await.unwrap().unwrap();
    assert_eq!(got, b"over tls");
}

#[tokio::test]
async fn a_key_named_by_its_fingerprint_or_its_display_form_dials() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[]);
    // The listener names its client by the display form.
    listening.clients().set([KeyName::parse(&client.public().display_fingerprint()).unwrap()]);
    let dialing = carrier(2, Arc::new(Direct::default()), &[]);
    let mut door = bound(&listening).await;
    let local = door.local_addr();
    for name in [server.public().fingerprint(), server.public().display_fingerprint()] {
        let address = format!("{name}@{local}");
        let (near, far) = tokio::join!(dialing.dial(&address), door.accept_peer());
        near.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(far.unwrap().peer, Some(client.public()), "{name}");
    }
}

#[tokio::test]
async fn the_carrier_says_why_a_host_or_a_provider_cannot_use_it() {
    let tcp = carrier(1, Arc::new(Direct::default()), &[]);
    assert_eq!(tcp.name(), "tcp");
    assert_eq!(tcp.facts(), Facts::STREAM);
    if pod_testkit::host::loopback().is_ok() {
        assert!(tcp.available().is_ok(), "{:?}", tcp.available());
    }
    let mut provider = rustls::crypto::ring::default_provider();
    let all = provider.signature_verification_algorithms;
    let mapping: Vec<_> = all.mapping.iter().filter(|(s, _)| *s != rustls::SignatureScheme::ED25519).copied().collect();
    provider.signature_verification_algorithms =
        rustls::crypto::WebPkiSupportedAlgorithms { all: all.all, mapping: Box::leak(mapping.into_boxed_slice()) };
    let lame = Tcp::new(Arc::new(provider), id(1), Arc::new(Direct::default()));
    let e = lame.available().unwrap_err();
    assert_eq!(e.kind, CarrierErrorKind::Unavailable);
    assert_eq!(e.class().exit_code(), 69);
    assert!(e.to_string().contains("Ed25519"), "{e}");
    let e = lame.listen("127.0.0.1:0").await.err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Unavailable, "{e}");
}

#[tokio::test]
async fn an_address_out_of_its_form_is_a_usage_error() {
    let tcp = carrier(1, Arc::new(Direct::default()), &[]);
    let key = id(2).public().hex();
    for bad in
        ["127.0.0.1:7000".to_string(), "nokey@127.0.0.1:7000".into(), format!("{key}@noport"), format!("{key}@:1")]
    {
        let e = tcp.dial(&bad).await.err().unwrap_or_else(|| panic!("{bad} dialed"));
        assert_eq!(e.kind, CarrierErrorKind::Usage, "{bad}: {e}");
        assert_eq!(e.class().exit_code(), 64, "{bad}");
    }
    for bad in ["nowhere", "localhost:7000", "127.0.0.1"] {
        assert_eq!(tcp.listen(bad).await.err().unwrap().kind, CarrierErrorKind::Usage, "{bad}");
    }
    let (named, target) = Tcp::address(&format!("{}@relay.example:443", id(2).public().fingerprint())).unwrap();
    assert!(matches!(named, KeyName::Fingerprint(_)) && named.matches(&id(2).public()));
    assert_eq!(target, Target::Name { host: "relay.example".into(), port: 443 });
    let mut none = tcp.clone();
    none.max_handshakes = 0;
    assert_eq!(none.listen("127.0.0.1:0").await.err().unwrap().kind, CarrierErrorKind::Usage);
}

#[tokio::test]
async fn a_failure_of_the_way_out_keeps_its_class() {
    let key = id(1).public().hex();
    for (class, kind) in [
        (Class::Refused, CarrierErrorKind::Refused),
        (Class::NotFound, CarrierErrorKind::NotFound),
        (Class::Policy, CarrierErrorKind::Refused),
        (Class::Unavailable, CarrierErrorKind::Unavailable),
    ] {
        let tcp = carrier(2, Arc::new(Fails(class)), &[]);
        let e = tcp.dial(&format!("{key}@192.0.2.1:7000")).await.err().unwrap();
        assert_eq!(e.kind, kind, "{class:?}: {e}");
        assert!(e.to_string().contains("test-fails"), "the error names the way out: {e}");
    }
}

#[tokio::test]
async fn a_peer_that_sends_nothing_holds_no_other_handshake() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let mut listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    // A limit far over the wait of the test: a listener that shook hands
    // one at a time would hold the good client behind the silent one.
    listening.handshake_timeout = Duration::from_secs(300);
    let dialing = carrier(2, Arc::new(Direct::default()), &[]);
    let mut door = bound(&listening).await;
    let _silent = tokio::net::TcpStream::connect(door.local_addr()).await.unwrap();
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(Duration::from_secs(20), both).await.expect("the good client is not held");
    near.unwrap();
    assert_eq!(far.unwrap().1, client.public());

    // The handshake of a silent peer ends at its time limit, as an error of
    // one accept.
    let mut quick = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    quick.handshake_timeout = Duration::from_millis(300);
    let mut door = bound(&quick).await;
    let _silent = tokio::net::TcpStream::connect(door.local_addr()).await.unwrap();
    let e = timeout(limit(), door.accept_key()).await.expect("the limit of the handshake").err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{e}");
    assert_eq!(e.class(), Class::Timeout);
}

#[test]
fn the_constants_of_the_carrier() {
    assert_eq!(tcp::NAME, "tcp");
    assert_eq!(tcp::ALPN, b"podlink/1");
    assert_eq!(tcp::HANDSHAKE_TIMEOUT, Duration::from_secs(10));
    assert_eq!(tcp::MAX_HANDSHAKES, 64);
    let tcp = Tcp::new(ring(), id(1), Arc::new(Direct::default()));
    assert_eq!(tcp.connect_timeout, tcp::CONNECT_TIMEOUT);
    assert!(tcp.clients().list().is_empty(), "a new carrier takes no client");
    let shown = format!("{tcp:?}");
    assert!(shown.contains(&id(1).public().fingerprint()) && shown.contains("test-direct"), "{shown}");
}

#[tokio::test]
async fn both_ends_take_the_alpn_of_the_carrier() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let mut listening = carrier(1, Arc::new(Direct::default()), &[client.public()]);
    listening.alpn = b"podnet/1";
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);
    let e = carrier(2, Arc::new(Direct::default()), &[]).dial(&address).await.err().expect("a dial of another ALPN");
    assert_ne!(e.kind, CarrierErrorKind::Refused, "{e}");
    let far = timeout(limit(), door.accept_key()).await.expect("in time").err().expect("an accept of another ALPN");
    assert!(far.to_string().contains("TLS handshake"), "{far}");
    let mut dialing = carrier(2, Arc::new(Direct::default()), &[]);
    dialing.alpn = b"podnet/1";
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("in time");
    near.expect("the dial of the same ALPN");
    assert_eq!(far.expect("the accept").1, client.public());
    assert_eq!(Tcp::new(ring(), id(1), Arc::new(Direct::default())).alpn, tcp::ALPN);
}
