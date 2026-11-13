//! The DNS carrier over datagrams that are lost, reordered and duplicated
//! (T-493; PodUtils/podnet#468, row E2-3): the handshake of the carrier over
//! the `Lossy` shim of a memory pair, with the ARQ and TLS, carries the
//! bytes whole and in order; a peer that goes silent ends the link at the
//! ARQ timeout. The DNS wire of queries and TXT answers runs in
//! `tests/dns.rs` against a real authoritative server on the loopback.

#![cfg(feature = "dns")]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::dns::{carrier, fast_arq};
use common::tcp::id;
use common::{LIMIT, pattern};
use pod_core::Target;
use pod_identity::KeyName;
use pod_link::BoxIo;
use pod_link::arq::ArqSettings;
use pod_link::datagram::lossy::{Faults, Lossy};
use pod_link::datagram::memory::{DEFAULT_DEPTH, DEFAULT_MTU, MemoryDatagrams, pair};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

type Shim = Arc<Lossy<MemoryDatagrams>>;

/// Two ends of the DNS carrier over a memory pair with `faults` each way.
async fn ends(faults: Faults, arq: ArqSettings) -> (BoxIo, BoxIo, Shim, Shim) {
    let (a, b) = pair(DEFAULT_MTU, DEFAULT_DEPTH);
    let (a, b) = (Arc::new(Lossy::new(a, faults, 1)), Arc::new(Lossy::new(b, faults, 2)));
    let (server, client) = (id(1), id(2));
    let (mut listening, mut dialing) = (carrier(1, &[client.public()]), carrier(2, &[]));
    (listening.arq, dialing.arq) = (arq, arq);
    let target = Target::Name { host: "l.test".into(), port: 53 };
    let key = KeyName::Key(server.public());
    let both = async {
        tokio::join!(dialing.connect_over(a.clone(), &target, &key), listening.accept_over(b.clone(), &"the peer"))
    };
    let (near, far) = timeout(LIMIT, both).await.expect("the handshake in time");
    let (near, far) = (near.expect("the dial"), far.expect("the accept"));
    assert_eq!(near.1, server.public());
    assert_eq!(far.1, client.public());
    (near.0, far.0, a, b)
}

/// `size` bytes each way at once; the time that it took.
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
    cross(near, far, 128 << 10).await;
    assert_eq!(a.stats().lost + b.stats().lost, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bytes_cross_each_way_at_three_percent_loss_with_reorder_and_duplicates() {
    let faults = Faults { loss: 0.03, reorder: 0.01, duplicate: 0.01 };
    let (near, far, a, b) = ends(faults, ArqSettings::default()).await;
    cross(near, far, 128 << 10).await;
    let (sa, sb) = (a.stats(), b.stats());
    assert!(sa.lost + sb.lost > 0, "the shim lost datagrams: {sa:?} {sb:?}");
    assert!(sa.reordered + sb.reordered > 0 && sa.duplicated + sb.duplicated > 0, "{sa:?} {sb:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peer_that_goes_silent_ends_the_link_at_the_arq_timeout() {
    // Keep the far end alive: a silent peer, not a closed one.
    let (mut near, _far, _a, b) = ends(Faults::NONE, fast_arq()).await;
    b.set_faults(Faults::SILENT);
    let began = Instant::now();
    // Frames that the silent peer never acknowledges, so the ARQ ends after
    // its retries (the ARQ has no idle timer on a pure receive).
    near.write_all(&vec![7u8; 64 << 10]).await.ok();
    let mut buf = [0u8; 16];
    let e = timeout(LIMIT, near.read(&mut buf)).await.expect("the end in time").expect_err("a silent peer");
    assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
    assert!(began.elapsed() >= Duration::from_millis(400), "not before the retries: {:?}", began.elapsed());
}
