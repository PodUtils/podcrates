//! The DNS simplex carrier (T-493, PodUtils/podnet#468 row E2-3): a link
//! over a real authoritative server on the loopback carries streams; the
//! frames go up in the labels of query names and come down in TXT records;
//! another key is refused on each side and named; a silent server ends the
//! dial; a query of a stranger changes nothing; an address or a setting out
//! of its form is a usage error. `tests/dns_loss.rs` has the loss, the
//! reorder and the duplicates over the `Lossy` shim.

#![cfg(feature = "dns")]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::dns::{ZONE, at, bound, carrier};
use common::tcp::{id, ring};
use common::{echo, limit, pattern};
use pod_core::Class;
use pod_link::dns::{Dns, DnsSettings, EDNS_SIZE};
use pod_link::tcp::{Refusal, refusal};
use pod_link::{Carrier, CarrierErrorKind, Cost, Facts, Link, Listener, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_dns_carries_streams() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let mut door = bound(&carrier(1, &[client.public()])).await;
    let dialing = carrier(2, &[]).to(door.local_addr());
    let address = at(&server.public());
    let both = async { tokio::join!(dialing.dial(&address), door.accept_peer()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    let (near, far) = (near.expect("the dial"), far.expect("the accept"));
    assert_eq!(far.peer, Some(client.public()), "the accept gives the key of the client");

    let dialer = Link::new(near, Role::Dialer, Settings::default()).unwrap();
    let _echo = echo(Link::new(far.io, Role::Listener, Settings::default()).unwrap());
    let mut moves = Vec::new();
    for seed in 0..2u32 {
        let mut stream = dialer.open().unwrap();
        moves.push(tokio::spawn(async move {
            let data = pattern(seed, 48 << 10);
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
async fn the_dialer_refuses_a_listener_of_another_key_and_names_it() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(3));
    let door = bound(&carrier(1, &[client.public()])).await;
    let dialing = carrier(2, &[]).to(door.local_addr());
    let e = dialing.dial(&at(&stranger.public())).await.err().expect("a dial took another key");
    assert_eq!(e.kind, CarrierErrorKind::Refused, "{e}");
    assert_eq!(e.class().exit_code(), 77);
    assert_eq!(refusal(&e), Some(Refusal::Unpinned(server.public())), "{e}");
    assert!(e.to_string().contains(&server.public().fingerprint()), "the error names the key seen: {e}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_listener_refuses_a_client_of_another_key_and_stays_open() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(4));
    let mut door = bound(&carrier(1, &[client.public()])).await;
    let address = at(&server.public());
    let other = carrier(4, &[]).to(door.local_addr());
    let both = async { tokio::join!(other.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the refusal in time");
    let near = near.err().expect("a client of another key dialed");
    assert_eq!(near.kind, CarrierErrorKind::KeyRefused, "the dialer learns of the refusal at the dial: {near}");
    assert_eq!(near.class().exit_code(), 77);
    assert!(near.to_string().contains(&stranger.public().fingerprint()), "{near}");
    let far = far.err().expect("the listener took a client of another key");
    assert_eq!(far.kind, CarrierErrorKind::Refused, "{far}");
    assert_eq!(refusal(&far), Some(Refusal::Unpinned(stranger.public())), "{far}");

    let good = carrier(2, &[]).to(door.local_addr());
    let both = async { tokio::join!(good.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    near.expect("the pinned client after a refusal");
    assert_eq!(far.expect("the listener stays open").1, client.public());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_silent_server_ends_the_dial_at_its_time_limit() {
    pod_testkit::need_loopback!();
    // A UDP socket that answers no query.
    let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut dialing = carrier(2, &[]).to(silent.local_addr().unwrap());
    dialing.handshake_timeout = Duration::from_millis(500);
    let e = timeout(limit(), dialing.dial(&at(&id(1).public()))).await.expect("the time limit").err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{e}");
    assert_eq!(e.class(), Class::Timeout);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_query_of_a_stranger_changes_nothing() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let mut listening = carrier(1, &[client.public()]);
    // The stranger's session ends at this bound; the key's own handshake
    // must fit in it, also under an emulator.
    listening.handshake_timeout = pod_testkit::host::scaled(Duration::from_millis(500));
    let mut door = bound(&listening).await;
    let local = door.local_addr();
    let dialing = carrier(2, &[]).to(local);
    let address = at(&server.public());
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("in time");
    let (mut near, (mut far, _)) = (near.unwrap(), far.unwrap());
    // Junk and a query of a session that no key opened, from another socket.
    let stranger = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let junk = pod_dns::txt::query(7, &format!("aaaa.{ZONE}"), pod_dns::txt::TXT, Some(EDNS_SIZE)).unwrap();
    for bytes in [&b"x"[..], &junk[..], &[0xffu8; 40][..]] {
        stranger.send_to(bytes, local).await.unwrap();
    }
    let data = pattern(3, 32 << 10);
    let sent = data.clone();
    let writer = tokio::spawn(async move {
        near.write_all(&sent).await.unwrap();
        near.shutdown().await.unwrap();
    });
    let mut got = Vec::new();
    timeout(limit(), far.read_to_end(&mut got)).await.expect("the bytes in time").unwrap();
    assert!(got == data, "the bytes came whole: {} of {}", got.len(), data.len());
    timeout(limit(), writer).await.unwrap().unwrap();
    let e = timeout(Duration::from_secs(1), door.accept_key()).await;
    assert!(e.is_err(), "the stranger started no session");
}

#[tokio::test]
async fn an_address_or_a_setting_out_of_its_form_is_a_usage_error() {
    let dns = carrier(1, &[]).to("127.0.0.1:53".parse().unwrap());
    let key = id(2).public().hex();
    for bad in ["l.test".to_string(), "nokey@l.test".into()] {
        let e = dns.dial(&bad).await.err().unwrap_or_else(|| panic!("{bad} dialed"));
        assert_eq!(e.kind, CarrierErrorKind::Usage, "{bad}: {e}");
        assert_eq!(e.class().exit_code(), 64, "{bad}");
    }
    // A dial with no server set.
    let no_server = carrier(1, &[]);
    assert_eq!(no_server.dial(&format!("{key}@l.test")).await.err().unwrap().kind, CarrierErrorKind::Usage);
    // A listener with no zone.
    let mut no_zone = carrier(1, &[]);
    no_zone.zone = String::new();
    assert_eq!(no_zone.listen("127.0.0.1:0").await.err().unwrap().kind, CarrierErrorKind::Usage);
    for bad in ["nowhere", "l.test"] {
        assert_eq!(dns.listen(bad).await.err().unwrap().kind, CarrierErrorKind::Usage, "{bad}");
    }
    let (named, zone) = Dns::address(&format!("{}@zone.example", id(2).public().fingerprint())).unwrap();
    assert!(named.matches(&id(2).public()) && zone == "zone.example");
    let mut none = dns.clone();
    none.max_handshakes = 0;
    assert_eq!(none.listen("127.0.0.1:0").await.err().unwrap().kind, CarrierErrorKind::Usage);
    for dsettings in [
        DnsSettings { queries_per_second: 0.0, ..DnsSettings::default() },
        DnsSettings { in_flight: 0, ..DnsSettings::default() },
        DnsSettings { answer: 100, ..DnsSettings::default() },
    ] {
        assert!(dsettings.check().is_err(), "{dsettings:?}");
        let mut bad = dns.clone();
        bad.dns = dsettings;
        assert_eq!(bad.dial(&format!("{key}@l.test")).await.err().unwrap().kind, CarrierErrorKind::Usage);
        assert_eq!(bad.available().err().map(|e| e.kind), Some(CarrierErrorKind::Unavailable));
    }
}

#[tokio::test]
async fn the_carrier_says_what_it_gives_and_why_it_cannot_run() {
    let dns = Dns::new(ring(), id(1)).zone("z.test");
    assert_eq!(dns.name(), "dns");
    assert_eq!(dns.facts(), Facts { cost: Cost::Highest, ..Facts::STREAM });
    if pod_testkit::host::loopback().is_ok() {
        assert!(dns.available().is_ok(), "{:?}", dns.available());
    }
    let mut old = rustls::crypto::ring::default_provider();
    old.cipher_suites.retain(|s| s.version() != &rustls::version::TLS13);
    let e = Dns::new(Arc::new(old), id(1)).zone("z.test").available().expect_err("a provider with no TLS 1.3");
    assert_eq!(e.kind, CarrierErrorKind::Unavailable, "{e}");
    assert_eq!(
        (pod_link::dns::NAME, pod_link::dns::ALPN, pod_link::dns::MAX_HANDSHAKES),
        ("dns", &b"podlink/1"[..], 64)
    );
    let shown = format!("{dns:?}");
    assert!(shown.contains(&id(1).public().fingerprint()) && shown.contains("z.test"), "{shown}");
}
