//! The DNS simplex carrier (T-493, PodUtils/podnet#468 row E2-3): a link
//! over a real authoritative server on the loopback carries streams; the
//! frames go up in the labels of query names and come down in TXT records;
//! another key is refused on each side and named; a silent server ends the
//! dial; a query of a stranger changes nothing; each query of the zone that
//! the listener cannot take gets an answer (T-1164, PodUtils/podssh#349);
//! an answer keeps the bytes of the question and clears RA (T-1170); an
//! address or a setting out of its form is a usage error.
//! `tests/dns_loss.rs` has the loss, the reorder and the duplicates over the
//! `Lossy` shim.

#![cfg(feature = "dns")]

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use common::dns::{ZONE, at, bound, carrier};
use common::tcp::{id, ring};
use common::{echo, limit, pattern};
use pod_core::Class;
use pod_dns::txt::{self, TXT};
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
    let mut door = bound(&carrier(1, &[client.public()])).await;
    let dialing = carrier(2, &[]).to(door.local_addr());
    let address = at(&stranger.public());
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (e, far) = timeout(limit(), both).await.expect("the refusal in time");
    let e = e.err().expect("a dial took another key");
    assert_eq!(e.kind, CarrierErrorKind::Refused, "{e}");
    assert_eq!(e.class().exit_code(), 77);
    assert_eq!(refusal(&e), Some(Refusal::Unpinned(server.public())), "{e}");
    assert!(e.to_string().contains(&server.public().fingerprint()), "the error names the key seen: {e}");
    let far = far.err().expect("the listener took a dialer that refused its key");
    assert_eq!(far.kind, CarrierErrorKind::KeyRefused, "the listener learns that its key was refused: {far}");
    assert_eq!(far.class().exit_code(), 77);
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

/// The bytes of the answer of the listener at `server` to `query`; `None`
/// when no answer comes in time.
async fn ask_bytes(server: SocketAddr, query: &[u8]) -> Option<Vec<u8>> {
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    socket.send_to(query, server).await.unwrap();
    let mut buf = vec![0u8; 1 << 16];
    let wait = pod_testkit::host::scaled(Duration::from_secs(2));
    let (n, _) = timeout(wait, socket.recv_from(&mut buf)).await.ok()?.unwrap();
    buf.truncate(n);
    Some(buf)
}

/// The answer of the listener at `server` to the query of `kind` for `name`,
/// with an OPT record of `edns` bytes when given; `None` when no answer
/// comes in time. The listener does no recursion, so RA is clear.
async fn ask(server: SocketAddr, name: &str, kind: u16, edns: Option<u16>) -> Option<txt::Answer> {
    let bytes = ask_bytes(server, &txt::query(0x5a5a, name, kind, edns).unwrap()).await?;
    assert_eq!(bytes[3] & 0x80, 0, "RA is clear: {name}");
    Some(txt::read(&bytes, 0x5a5a, name, kind).expect("an answer to the question"))
}

/// NOERROR with no record and the AA bit, with the OPT record when the
/// query sent one.
fn assert_no_record(answer: Option<txt::Answer>, edns: bool, what: &str) {
    let answer = answer.unwrap_or_else(|| panic!("{what}: no answer"));
    assert_eq!((answer.rcode, answer.authoritative), (0, true), "{what}: {answer:?}");
    assert!(answer.records.is_empty(), "{what}: {answer:?}");
    assert_eq!(answer.edns.is_some(), edns, "{what}: {answer:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_query_of_the_zone_that_the_listener_cannot_take_gets_an_answer() {
    pod_testkit::need_loopback!();
    let door = bound(&carrier(1, &[id(2).public()])).await;
    let local = door.local_addr();
    // A resolver that asks for a part of the name first (RFC 9156) asks for
    // A records and for the zone itself.
    let partial = format!("abc.{ZONE}");
    assert_no_record(ask(local, &partial, pod_dns::wire::A, Some(EDNS_SIZE)).await, true, "an A query");
    assert_no_record(ask(local, &partial, TXT, Some(EDNS_SIZE)).await, true, "a TXT query of a part of a name");
    assert_no_record(ask(local, &partial, TXT, None).await, false, "a TXT query with no OPT record");
    assert_no_record(ask(local, &ZONE.to_uppercase(), TXT, None).await, false, "the zone in upper case");
    let out = ask(local, "abc.example.org", TXT, Some(EDNS_SIZE)).await.expect("an answer out of the zone");
    assert_eq!((out.rcode, out.authoritative), (u16::from(txt::RCODE_REFUSED), false), "{out:?}");
    assert!(out.records.is_empty(), "{out:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_listener_answers_a_new_session_with_no_record() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let mut door = bound(&carrier(1, &[client.public()])).await;
    let local = door.local_addr();
    let dialing = carrier(2, &[]).to(local);
    let address = at(&server.public());
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    // The open session keeps the task of the listener up after the drop.
    let (_near, _far) = (near.unwrap(), far.unwrap());
    drop(door);
    // The task reads the stop before the next query.
    tokio::time::sleep(Duration::from_millis(200)).await;
    // Thirteen base32 characters hold the eight bytes of a session and a
    // counter.
    let fresh = format!("abcdefghijklm.{ZONE}");
    assert_no_record(ask(local, &fresh, TXT, Some(EDNS_SIZE)).await, true, "a new session after the drop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_data_answer_keeps_the_bytes_of_the_question_and_clears_ra() {
    pod_testkit::need_loopback!();
    let door = bound(&carrier(1, &[id(2).public()])).await;
    // A resolver with 0x20 (Unbound `use-caps-for-id`) sends the name in
    // mixed case, and it drops an answer whose question has other bytes.
    let mixed = format!("aBcDeFgHiJkLm.{}", ZONE.to_uppercase());
    let query = txt::query(0x0a0b, &mixed, TXT, Some(EDNS_SIZE)).unwrap();
    let got = ask_bytes(door.local_addr(), &query).await.expect("an answer to a query of a session");
    let end = 12 + mixed.len() + 2 + 4;
    assert_eq!(&got[12..end], &query[12..end], "the question of the answer has the bytes of the query");
    assert_eq!((got[2] & 0x84, got[3] & 0x80), (0x84, 0), "QR and AA set, RA clear");
    let answer = txt::read(&got, 0x0a0b, &mixed, TXT).expect("an answer to the question");
    assert_eq!((answer.rcode, answer.authoritative), (0, true), "{answer:?}");
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
    assert_eq!(dns.facts(), Facts::with_cost(Cost::Highest));
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
