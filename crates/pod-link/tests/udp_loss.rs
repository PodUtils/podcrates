//! The udp carrier over datagrams that are lost, reordered and duplicated
//! (T-424; PodUtils/podnet#468, row E2-1): the handshake of the carrier
//! over the `Lossy` shim of a memory pair, with KCP and TLS, carries the
//! bytes whole and in order; a peer that goes silent ends the stream at
//! the idle limit of KCP.
//!
//! The 8 MiB of podnet's gate run with `nodelay`; the default (the
//! congestion window of KCP) moves 1 MiB at 3 % loss.

#![cfg(feature = "udp")]

mod common;

use std::time::{Duration, Instant};

use common::tcp::id;
use common::udp::carrier;
use common::{LIMIT, pattern};
use pod_core::Target;
use pod_identity::KeyName;
use pod_link::BoxIo;
use pod_link::datagram::Datagrams;
use pod_link::datagram::lossy::{Faults, Lossy};
use pod_link::datagram::memory::{DEFAULT_DEPTH, DEFAULT_MTU, MemoryDatagrams, pair};
use pod_link::udp::{KcpSettings, KcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

type Shim = std::sync::Arc<Lossy<MemoryDatagrams>>;

/// Two ends of the udp carrier over a memory pair with `faults` in each
/// direction, and the shims.
async fn ends(faults: Faults, kcp: KcpSettings) -> (BoxIo, BoxIo, Shim, Shim) {
    let (a, b) = pair(DEFAULT_MTU, DEFAULT_DEPTH);
    let (a, b) = (std::sync::Arc::new(Lossy::new(a, faults, 1)), std::sync::Arc::new(Lossy::new(b, faults, 2)));
    let (server, client) = (id(1), id(2));
    let (mut listening, mut dialing) = (carrier(1, &[client.public()]), carrier(2, &[]));
    (listening.kcp, dialing.kcp) = (kcp, kcp);
    let target = Target::Addr("192.0.2.1:7000".parse().unwrap());
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
async fn cross(near: BoxIo, far: BoxIo, size: usize) -> Duration {
    let began = Instant::now();
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
    let (_, _, got_up, got_down) = timeout(Duration::from_secs(120), all).await.expect("the transfer in time");
    assert!(got_up == up, "up: whole and in order, {} of {}", got_up.len(), up.len());
    assert!(got_down == down, "down: whole and in order, {} of {}", got_down.len(), down.len());
    began.elapsed()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_mib_crosses_each_way_with_no_loss() {
    let (near, far, a, b) = ends(Faults::NONE, KcpSettings::default()).await;
    cross(near, far, 1 << 20).await;
    assert_eq!(a.stats().lost + b.stats().lost, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_mib_crosses_each_way_at_three_percent_loss_with_reorder_and_duplicates() {
    let faults = Faults { loss: 0.03, reorder: 0.01, duplicate: 0.01 };
    let (near, far, a, b) = ends(faults, KcpSettings::default()).await;
    cross(near, far, 1 << 20).await;
    let (sa, sb) = (a.stats(), b.stats());
    assert!(sa.lost > 0 && sb.lost > 0, "the shim lost datagrams each way: {sa:?} {sb:?}");
    assert!(sa.reordered + sb.reordered > 0 && sa.duplicated + sb.duplicated > 0, "{sa:?} {sb:?}");
}

/// podnet's round trip of 8 MiB at 5 % loss and 1 % reorder, with KCP's
/// "nodelay": the congestion window of KCP (the default) falls to one
/// segment at each loss and takes about 12 s for 2 MiB at 5 % loss here
/// (call C272).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eight_mib_cross_at_five_percent_loss_and_one_percent_reorder_with_nodelay() {
    let faults = Faults { loss: 0.05, reorder: 0.01, duplicate: 0.0 };
    let kcp = KcpSettings { nodelay: true, ..KcpSettings::default() };
    let (near, far, a, b) = ends(faults, kcp).await;
    cross(near, far, 8 << 20).await;
    assert!(a.stats().lost > 0 && b.stats().lost > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peer_that_goes_silent_ends_the_stream_at_the_idle_limit() {
    let kcp = KcpSettings { idle: Duration::from_secs(1), ..KcpSettings::default() };
    let (mut near, _far, _a, b) = ends(Faults::NONE, kcp).await;
    b.set_faults(Faults::SILENT);
    let began = Instant::now();
    let mut buf = [0u8; 16];
    let e = timeout(LIMIT, near.read(&mut buf)).await.expect("the end in time").expect_err("a silent peer");
    assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
    assert!(began.elapsed() >= Duration::from_millis(900), "not before the idle limit: {:?}", began.elapsed());
}

/// An `ACK` whose time stamp is 2^31 ms old overflows the arithmetic of
/// the crate `kcp`, which panics in a build with overflow checks: the
/// stream ends with an error and does not wait for ever (T-521).
#[cfg(debug_assertions)]
#[tokio::test]
async fn a_datagram_that_breaks_kcp_ends_the_stream_with_an_error() {
    let (a, b) = pair(DEFAULT_MTU, DEFAULT_DEPTH);
    let mut stream = KcpStream::new(a, Some(7), KcpSettings::default()).unwrap();
    let mut ack = [0u8; 24];
    ack[..4].copy_from_slice(&7u32.to_le_bytes());
    ack[4] = 82;
    ack[8..12].copy_from_slice(&0x8000_0000u32.to_le_bytes());
    b.send(&ack).await.unwrap();
    let mut buf = [0u8; 16];
    let e = timeout(LIMIT, stream.read(&mut buf)).await.expect("the end in time").expect_err("a broken KCP");
    assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "{e}");
}
