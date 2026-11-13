//! The udp carrier (T-424, PodUtils/podnet#515, row E2-1 of
//! PodUtils/podnet#468): a link over KCP with TLS carries streams; the
//! dial goes over the datagrams of the caller's way out; another key is
//! refused on each side and named; a peer that stops in its handshake
//! holds no other one; a silent listener ends the dial; datagrams of a
//! stranger change nothing; the bytes before a drop reach the peer.
//! `tests/udp_loss.rs` has the loss, the reorder and the duplicates.

#![cfg(feature = "udp")]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::dgram::UdpWay;
use common::tcp::{id, ring};
use common::udp::{at, bound, carrier};
use common::{LIMIT, echo, pattern};
use pod_core::{Class, Target};
use pod_identity::KeyName;
use pod_link::tcp::{Refusal, refusal};
use pod_link::udp::{self, KcpSettings, Udp};
use pod_link::{Carrier, CarrierErrorKind, Facts, Link, Listener, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_kcp_carries_streams() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let way = Arc::new(UdpWay::default());
    let listening = carrier(1, &[client.public()]);
    let dialing = carrier(2, &[]).with_way(way.clone());
    let mut door = bound(&listening).await;
    assert_eq!(door.address(), door.local_addr().to_string());
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_peer()) };
    let (near, far) = timeout(LIMIT, both).await.expect("the dial and the accept in time");
    let (near, far) = (near.expect("the dial"), far.expect("the accept"));
    assert_eq!(far.peer, Some(client.public()), "the accept gives the key of the client");
    assert_eq!(way.sockets.load(Ordering::SeqCst), 1, "the dial goes over the datagrams of the way out");

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
async fn the_dialer_refuses_a_listener_of_another_key_and_names_it() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(3));
    let door = bound(&carrier(1, &[client.public()])).await;
    let e = carrier(2, &[]).dial(&at(&stranger.public(), &door)).await.err().expect("a dial took another key");
    assert_eq!(e.kind, CarrierErrorKind::Refused, "{e}");
    assert_eq!(e.class().exit_code(), 77);
    assert_eq!(refusal(&e), Some(Refusal::Unpinned(server.public())), "{e}");
    assert!(e.to_string().contains(&server.public().fingerprint()), "the error names the key seen: {e}");
}

#[tokio::test]
async fn the_listener_refuses_a_client_of_another_key_and_stays_open() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(4));
    let mut door = bound(&carrier(1, &[client.public()])).await;
    let address = at(&server.public(), &door);
    let other = carrier(4, &[]);
    let both = async { tokio::join!(other.dial(&address), door.accept_key()) };
    let (near, far) = timeout(LIMIT, both).await.expect("the refusal in time");
    let near = near.err().expect("a client of another key dialed");
    assert_eq!(near.kind, CarrierErrorKind::Refused, "the dialer learns of the refusal at the dial: {near}");
    assert!(near.to_string().contains(&stranger.public().fingerprint()), "{near}");
    let far = far.err().expect("the listener took a client of another key");
    assert_eq!(far.kind, CarrierErrorKind::Refused, "{far}");
    assert_eq!(refusal(&far), Some(Refusal::Unpinned(stranger.public())), "{far}");

    let good = carrier(2, &[]);
    let both = async { tokio::join!(good.dial(&address), door.accept_key()) };
    let (near, far) = timeout(LIMIT, both).await.expect("the dial and the accept in time");
    near.expect("the pinned client after a refusal");
    assert_eq!(far.expect("the listener stays open").1, client.public());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peer_that_stops_in_its_handshake_holds_no_other_one() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let mut listening = carrier(1, &[client.public()]);
    listening.handshake_timeout = Duration::from_secs(300);
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);
    // The stopped client sends its first datagrams and nothing more.
    let mut stopped = carrier(2, &[]).with_way(Arc::new(UdpWay { passes: Some(1), ..UdpWay::default() }));
    stopped.handshake_timeout = Duration::from_secs(300);
    let held = tokio::spawn(async move { stopped.dial(&address).await.map(drop) });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (good, address) = (carrier(2, &[]), at(&server.public(), &door));
    let both = async { tokio::join!(good.dial(&address), door.accept_key()) };
    let (near, far) = timeout(Duration::from_secs(20), both).await.expect("the good client is not held");
    near.unwrap();
    assert_eq!(far.unwrap().1, client.public());
    held.abort();

    let mut quick = carrier(1, &[client.public()]);
    quick.handshake_timeout = Duration::from_millis(500);
    let mut door = bound(&quick).await;
    let address = at(&server.public(), &door);
    let mute = carrier(2, &[]).with_way(Arc::new(UdpWay { passes: Some(1), ..UdpWay::default() }));
    let _held = tokio::spawn(async move { mute.dial(&address).await.map(drop) });
    let e = timeout(LIMIT, door.accept_key()).await.expect("the limit of the handshake").err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{e}");
    assert_eq!(e.class(), Class::Timeout);
}

#[tokio::test]
async fn a_silent_listener_ends_the_dial_at_its_time_limit() {
    pod_testkit::need_loopback!();
    let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut dialing = carrier(2, &[]);
    dialing.handshake_timeout = Duration::from_millis(300);
    let address = format!("{}@{}", id(1).public().hex(), silent.local_addr().unwrap());
    let e = timeout(LIMIT, dialing.dial(&address)).await.expect("the time limit of the dial").err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{e}");
    assert_eq!(e.class(), Class::Timeout);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn datagrams_of_a_stranger_change_nothing() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    // A short limit of the handshake: a session that a stranger started
    // would end in an error of an accept within the wait below.
    let mut listening = carrier(1, &[client.public()]);
    listening.handshake_timeout = Duration::from_millis(300);
    let mut door = bound(&listening).await;
    let local = door.local_addr();
    let (dialing, address) = (carrier(2, &[]), at(&server.public(), &door));
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(LIMIT, both).await.expect("in time");
    let (mut near, (mut far, _)) = (near.unwrap(), far.unwrap());
    // Short datagrams, a segment of no session that is not a first one,
    // and bytes of no form, from another socket.
    let stranger = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut late = [0u8; 40];
    late[4] = 81;
    late[12] = 9;
    for junk in [&b"x"[..], &late[..], &[0xffu8; 300][..]] {
        stranger.send_to(junk, local).await.unwrap();
    }
    let data = pattern(3, 1 << 20);
    let sent = data.clone();
    let writer = tokio::spawn(async move {
        near.write_all(&sent).await.unwrap();
        near.shutdown().await.unwrap();
        near
    });
    let mut got = Vec::new();
    timeout(LIMIT, far.read_to_end(&mut got)).await.expect("the bytes in time").unwrap();
    assert!(got == data, "the bytes came whole: {} of {}", got.len(), data.len());
    drop(timeout(LIMIT, writer).await.unwrap().unwrap());
    let e = timeout(Duration::from_secs(1), door.accept_key()).await;
    assert!(e.is_err(), "the stranger started no session");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_bytes_before_a_drop_reach_the_peer() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let mut door = bound(&carrier(1, &[client.public()])).await;
    let (dialing, address) = (carrier(2, &[]), at(&server.public(), &door));
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(LIMIT, both).await.expect("in time");
    let (mut near, (mut far, _)) = (near.unwrap(), far.unwrap());
    let data = pattern(7, 2 << 20);
    let sent = data.clone();
    // The dialer writes, flushes the TLS into KCP and drops its stream
    // with the last segments not acknowledged; the listener reads on.
    let writer = tokio::spawn(async move {
        near.write_all(&sent).await.unwrap();
        near.flush().await.unwrap();
    });
    let mut got = vec![0u8; data.len()];
    timeout(LIMIT, far.read_exact(&mut got)).await.expect("the bytes in time").unwrap();
    timeout(LIMIT, writer).await.unwrap().unwrap();
    assert!(got == data, "each byte before the drop came whole");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listener_runs_over_the_datagrams_of_the_caller() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let socket = pod_core::SystemDatagrams::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let mut door = carrier(1, &[client.public()]).bind_over(Arc::new(socket)).await.unwrap();
    let (dialing, address) = (carrier(2, &[]), at(&server.public(), &door));
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(LIMIT, both).await.expect("in time");
    let (mut near, (mut far, key)) = (near.unwrap(), far.unwrap());
    assert_eq!(key, client.public());
    near.write_all(b"over the caller's datagrams").await.unwrap();
    let mut got = [0u8; 27];
    timeout(LIMIT, far.read_exact(&mut got)).await.unwrap().unwrap();
    assert_eq!(&got, b"over the caller's datagrams");
}

#[tokio::test]
async fn an_address_or_a_setting_out_of_its_form_is_a_usage_error() {
    let udp = carrier(1, &[]);
    let key = id(2).public().hex();
    for bad in ["127.0.0.1:7000".to_string(), "nokey@127.0.0.1:7000".into(), format!("{key}@127.0.0.1")] {
        let e = udp.dial(&bad).await.err().unwrap_or_else(|| panic!("{bad} dialed"));
        assert_eq!(e.kind, CarrierErrorKind::Usage, "{bad}: {e}");
        assert_eq!(e.class().exit_code(), 64, "{bad}");
    }
    for bad in ["nowhere", "localhost:7000", "127.0.0.1"] {
        assert_eq!(udp.listen(bad).await.err().unwrap().kind, CarrierErrorKind::Usage, "{bad}");
    }
    let (named, target) = Udp::address(&format!("{}@[::1]:8443", id(2).public().fingerprint())).unwrap();
    assert!(matches!(named, KeyName::Fingerprint(_)) && named.matches(&id(2).public()));
    assert_eq!(target, Target::Addr("[::1]:8443".parse().unwrap()));
    let mut none = udp.clone();
    none.max_handshakes = 0;
    assert_eq!(none.listen("127.0.0.1:0").await.err().unwrap().kind, CarrierErrorKind::Usage);
    for kcp in [
        KcpSettings { window: 31, ..KcpSettings::default() },
        KcpSettings { window: 1025, ..KcpSettings::default() },
        KcpSettings { segment: 1200, ..KcpSettings::default() },
        KcpSettings { interval: Duration::from_millis(5), ..KcpSettings::default() },
    ] {
        assert!(kcp.check().is_err(), "{kcp:?}");
        let mut bad = udp.clone();
        bad.kcp = kcp;
        let e = bad.dial(&format!("{key}@127.0.0.1:7000")).await.err().expect("a dial with a bad setting");
        assert_eq!(e.kind, CarrierErrorKind::Usage, "{kcp:?}: {e}");
        assert_eq!(bad.available().err().map(|e| e.kind), Some(CarrierErrorKind::Unavailable), "{kcp:?}");
    }
}

#[tokio::test]
async fn the_carrier_says_what_it_gives_and_why_it_cannot_run() {
    let udp = Udp::new(ring(), id(1));
    assert_eq!((udp.name(), udp.facts()), ("udp", Facts::STREAM));
    if pod_testkit::host::loopback().is_ok() {
        assert!(udp.available().is_ok(), "{:?}", udp.available());
    }
    let mut old = rustls::crypto::ring::default_provider();
    old.cipher_suites.retain(|s| s.version() != &rustls::version::TLS13);
    let e = Udp::new(Arc::new(old), id(1)).available().expect_err("a provider with no TLS 1.3");
    assert_eq!(e.kind, CarrierErrorKind::Unavailable, "{e}");
    let k = KcpSettings::default();
    assert_eq!((k.mtu, k.segment, k.window, k.nodelay, k.idle), (1200, 1024, 256, false, Duration::from_secs(20)));
    assert_eq!((udp::NAME, udp::ALPN, udp::MAX_HANDSHAKES), ("udp", &b"podlink/1"[..], 64));
    let shown = format!("{:?}", udp.clone().with_way(Arc::new(UdpWay::default())));
    assert!(shown.contains(&id(1).public().fingerprint()) && shown.contains("test-udp"), "{shown}");
}
