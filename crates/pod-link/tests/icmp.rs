//! The ICMP echo carrier (T-497, PodUtils/podnet#469 row E3-2). The framing
//! of the echo messages runs over a memory pair (the `Lossy` shim), with the
//! ARQ and TLS, with no privilege: a link carries streams; the bytes cross
//! whole at loss, reorder and duplicates, also across the wrap of the
//! sequence of the ARQ (T-541); a silent peer ends the link; the
//! keys are pinned on each side and a refused key is named; an echo of the
//! wrong session or the wrong kind is dropped. The real raw socket runs the
//! same framing; its test skips with a reason where the host has no
//! `CAP_NET_RAW` (call C276).

#![cfg(feature = "icmp")]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::icmp::{carrier, fast_arq};
use common::tcp::{id, ring};
use common::{limit, pattern};
use pod_core::Target;
use pod_identity::KeyName;
use pod_link::BoxIo;
use pod_link::arq::ArqSettings;
use pod_link::datagram::Datagrams;
use pod_link::datagram::lossy::{Faults, Lossy};
use pod_link::datagram::memory::{DEFAULT_DEPTH, DEFAULT_MTU, MemoryDatagrams, pair};
use pod_link::icmp::{EchoFrames, Icmp};
use pod_link::tcp::{Refusal, refusal};
use pod_link::{Carrier, CarrierErrorKind, Cost, Facts};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

type Shim = Arc<Lossy<MemoryDatagrams>>;

const SESSION: u32 = 0x1234_5678;

/// Two ends of the ICMP carrier over a memory pair with `faults` each way:
/// the dialer frames requests, the listener replies.
async fn ends(
    faults: Faults,
    arq: ArqSettings,
) -> (
    Result<(BoxIo, pod_identity::PublicKey), pod_link::CarrierError>,
    Result<(BoxIo, pod_identity::PublicKey), pod_link::CarrierError>,
    Shim,
    Shim,
) {
    let (a, b) = pair(DEFAULT_MTU, DEFAULT_DEPTH);
    let (a, b) = (Arc::new(Lossy::new(a, faults, 1)), Arc::new(Lossy::new(b, faults, 2)));
    let (server, client) = (id(1), id(2));
    let (mut listening, mut dialing) = (carrier(1, &[client.public()]), carrier(2, &[]));
    (listening.arq, dialing.arq) = (arq, arq);
    let up = EchoFrames::client(a.clone(), SESSION, SESSION as u16);
    let down = EchoFrames::server(b.clone(), SESSION, SESSION as u16);
    let target = Target::Addr("127.0.0.1:0".parse().unwrap());
    let key = KeyName::Key(server.public());
    let both =
        async { tokio::join!(dialing.connect_over(up, &target, &key), listening.accept_over(down, &"the peer")) };
    let (near, far) = timeout(limit(), both).await.expect("the handshake in time");
    (near, far, a, b)
}

/// `size` bytes each way on a handshake that worked.
async fn cross(near: BoxIo, far: BoxIo, size: usize) {
    let (mut near_read, mut near_write) = tokio::io::split(near);
    let (mut far_read, mut far_write) = tokio::io::split(far);
    let (up, down) = (pattern(1, size), pattern(2, size));
    let (up2, down2) = (up.clone(), down.clone());
    let send_up = async move {
        near_write.write_all(&up2).await.unwrap();
        near_write.shutdown().await.unwrap();
    };
    let send_down = async move {
        far_write.write_all(&down2).await.unwrap();
        far_write.shutdown().await.unwrap();
    };
    let read_up = async move {
        let mut got = Vec::new();
        far_read.read_to_end(&mut got).await.unwrap();
        got
    };
    let read_down = async move {
        let mut got = Vec::new();
        near_read.read_to_end(&mut got).await.unwrap();
        got
    };
    let all = async { tokio::join!(send_up, send_down, read_up, read_down) };
    let (_, _, got_up, got_down) = timeout(Duration::from_secs(60), all).await.expect("the transfer in time");
    assert!(got_up == up, "up: whole and in order, {} of {}", got_up.len(), up.len());
    assert!(got_down == down, "down: whole and in order, {} of {}", got_down.len(), down.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bytes_cross_each_way_with_no_loss() {
    let (near, far, a, b) = ends(Faults::NONE, ArqSettings::default()).await;
    cross(near.unwrap().0, far.unwrap().0, 128 << 10).await;
    assert_eq!(a.stats().lost + b.stats().lost, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bytes_cross_each_way_at_three_percent_loss_with_reorder_and_duplicates() {
    let faults = Faults { loss: 0.03, reorder: 0.01, duplicate: 0.01 };
    let (near, far, a, b) = ends(faults, ArqSettings::default()).await;
    cross(near.unwrap().0, far.unwrap().0, 128 << 10).await;
    let (sa, sb) = (a.stats(), b.stats());
    assert!(sa.lost + sb.lost > 0, "{sa:?} {sb:?}");
    assert!(sa.reordered + sb.reordered > 0 && sa.duplicated + sb.duplicated > 0, "{sa:?} {sb:?}");
}

/// The first sequence is 64 frames before the wrap, so each direction
/// wraps in the transfer: the carrier answers the rekey of the ARQ, and
/// no direction stops at the wrap.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bytes_cross_the_wrap_of_the_sequence_at_loss() {
    let faults = Faults { loss: 0.03, reorder: 0.01, duplicate: 0.01 };
    let arq = ArqSettings { start: u32::MAX - 63, rekey_ahead: 256, ..ArqSettings::default() };
    let (near, far, _a, _b) = ends(faults, arq).await;
    cross(near.unwrap().0, far.unwrap().0, 128 << 10).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_keys_are_proven_on_each_side() {
    let (near, far, _a, _b) = ends(Faults::NONE, ArqSettings::default()).await;
    assert_eq!(near.unwrap().1, id(1).public(), "the dialer proves the key of the listener");
    assert_eq!(far.unwrap().1, id(2).public(), "the listener proves the key of the client");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_key_is_named_on_each_side() {
    let (a, b) = pair(DEFAULT_MTU, DEFAULT_DEPTH);
    let (server, client, stranger) = (id(1), id(2), id(3));
    // The listener pins another key than the client's.
    let listening = carrier(1, &[stranger.public()]);
    let dialing = carrier(2, &[]);
    let up = EchoFrames::client(a, SESSION, SESSION as u16);
    let down = EchoFrames::server(b, SESSION, SESSION as u16);
    let target = Target::Addr("127.0.0.1:0".parse().unwrap());
    let key = KeyName::Key(server.public());
    let both =
        async { tokio::join!(dialing.connect_over(up, &target, &key), listening.accept_over(down, &"the peer")) };
    let (near, far) = timeout(limit(), both).await.expect("the refusal in time");
    let near = near.err().expect("the dialer learns of the refusal");
    assert_eq!(near.kind, CarrierErrorKind::Refused, "{near}");
    assert!(near.to_string().contains(&client.public().fingerprint()), "names this key: {near}");
    let far = far.err().expect("the listener refuses the client");
    assert_eq!(refusal(&far), Some(Refusal::Unpinned(client.public())), "{far}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peer_that_goes_silent_ends_the_link() {
    let (near, _far, _a, b) = ends(Faults::NONE, fast_arq()).await;
    let mut near = near.unwrap().0;
    b.set_faults(Faults::SILENT);
    let began = Instant::now();
    // Frames that the silent peer never acknowledges, so the ARQ ends.
    near.write_all(&vec![7u8; 64 << 10]).await.ok();
    let mut buf = [0u8; 16];
    let e = timeout(limit(), near.read(&mut buf)).await.expect("the end in time").expect_err("a silent peer");
    assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
    assert!(began.elapsed() >= Duration::from_millis(400), "not before the retries: {:?}", began.elapsed());
}

#[tokio::test]
async fn an_echo_of_the_wrong_session_or_mark_is_dropped() {
    // A client's echo of the same session reaches the server.
    let (a, b) = pair(DEFAULT_MTU, DEFAULT_DEPTH);
    let client = EchoFrames::client(a, 1, 1);
    let server = EchoFrames::server(b, 1, 1);
    client.send(b"hello").await.unwrap();
    assert_eq!(timeout(limit(), server.recv()).await.unwrap().unwrap(), b"hello");
    // Another session is dropped.
    let (c, d) = pair(DEFAULT_MTU, DEFAULT_DEPTH);
    let c1 = EchoFrames::client(c, 1, 1);
    let wrong = EchoFrames::server(d, 2, 2);
    c1.send(b"x").await.unwrap();
    assert!(timeout(Duration::from_millis(200), wrong.recv()).await.is_err(), "the wrong session is dropped");
    // A client's own mark (a self echo or the kernel's reply to a request on
    // a shared socket) is dropped by another client.
    let (e, f) = pair(DEFAULT_MTU, DEFAULT_DEPTH);
    let c2 = EchoFrames::client(e, 1, 1);
    let also_client = EchoFrames::client(f, 1, 1);
    c2.send(b"y").await.unwrap();
    assert!(timeout(Duration::from_millis(200), also_client.recv()).await.is_err(), "the wrong mark is dropped");
}

#[tokio::test]
async fn an_address_or_a_setting_out_of_its_form_is_a_usage_error() {
    let icmp = carrier(1, &[]);
    for bad in ["127.0.0.1", "nokey@127.0.0.1"] {
        assert_eq!(icmp.dial(bad).await.err().unwrap().kind, CarrierErrorKind::Usage, "{bad}");
    }
    let (named, target) = Icmp::address(&format!("{}@10.0.0.1", id(2).public().fingerprint())).unwrap();
    assert!(named.matches(&id(2).public()));
    assert_eq!(target, Target::Addr("10.0.0.1:0".parse().unwrap()));
    for bad in ["nowhere", "not an ip", "[::1]"] {
        assert_eq!(icmp.listen(bad).await.err().unwrap().kind, CarrierErrorKind::Usage, "{bad}");
    }
    let mut bad = icmp.clone();
    bad.icmp.payload = 10;
    assert!(bad.icmp.check().is_err());
    assert_eq!(
        bad.connect_over(
            pair(DEFAULT_MTU, DEFAULT_DEPTH).0,
            &Target::Addr("127.0.0.1:0".parse().unwrap()),
            &KeyName::Key(id(1).public())
        )
        .await
        .err()
        .unwrap()
        .kind,
        CarrierErrorKind::Usage
    );
}

#[tokio::test]
async fn the_carrier_says_what_it_gives() {
    let icmp = Icmp::new(ring(), id(1));
    assert_eq!(icmp.name(), "icmp");
    assert_eq!(icmp.facts(), Facts { cost: Cost::High, ..Facts::STREAM });
    assert_eq!(
        (pod_link::icmp::NAME, pod_link::icmp::ALPN, pod_link::icmp::PAYLOAD),
        ("icmp", &b"podlink/1"[..], 1400)
    );
    let mut old = rustls::crypto::ring::default_provider();
    old.cipher_suites.retain(|s| s.version() != &rustls::version::TLS13);
    let e = Icmp::new(Arc::new(old), id(1)).available().expect_err("a provider with no TLS 1.3");
    assert_eq!(e.kind, CarrierErrorKind::Unavailable, "{e}");
}

/// `available` runs with no tokio runtime, as a probe of a host does: the
/// test of the raw socket does not register it with tokio.
#[test]
fn available_needs_no_runtime() {
    let result = Icmp::new(ring(), id(1)).available();
    if let Err(e) = result {
        assert_eq!(e.kind, CarrierErrorKind::Unavailable, "{e}");
    }
}
