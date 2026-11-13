//! The ARQ over a datagram pair in memory with a seeded shim, on a paused
//! clock: bytes whole and in order at 3 % loss with reorder and duplicates,
//! a link of streams over it, a peer that goes silent, the rekey before the
//! wrap, the end of the bytes, and an end that answers after its drop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use pod_link::arq::wire::Datagram;
use pod_link::arq::{ArqError, ArqHandle, ArqSettings, ArqStream};
use pod_link::datagram::lossy::{Faults, Lossy};
use pod_link::datagram::memory::{self, MemoryDatagrams};
use pod_link::datagram::{DatagramFuture, Datagrams};
use pod_link::{Link, Role, Settings};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::{Instant, timeout};

/// The bound of a transfer on the paused clock: a transfer that stops
/// fails the test, and does not hang it.
const LIMIT: Duration = Duration::from_secs(600);

/// The faults of the gate: 3 % of the datagrams lost, 1 % reordered, 1 %
/// sent two times.
const GATE: Faults = Faults { loss: 0.03, reorder: 0.01, duplicate: 0.01 };

type Shim = Arc<Lossy<MemoryDatagrams>>;

/// Two shims over a memory pair, each with `faults` from its own seed.
fn shims(faults: Faults, seed: u64) -> (Shim, Shim) {
    let (a, b) = memory::pair(memory::DEFAULT_MTU, memory::DEFAULT_DEPTH);
    (Arc::new(Lossy::new(a, faults, seed)), Arc::new(Lossy::new(b, faults, seed ^ 0x5eed)))
}

fn ends(a: &Shim, b: &Shim, settings: ArqSettings) -> (ArqStream, ArqStream) {
    (ArqStream::new(a.clone(), settings).unwrap(), ArqStream::new(b.clone(), settings).unwrap())
}

/// Bytes from a seed, so that a lost, doubled or moved fragment shows.
fn bytes(n: usize, seed: u64) -> Vec<u8> {
    let mut rng = pod_link::datagram::lossy::SplitMix64(seed);
    (0..n / 8).flat_map(|_| rng.next_u64().to_le_bytes()).collect()
}

/// Write `data` in writes of odd sizes, then shut down.
async fn send_all(mut to: impl AsyncWrite + Unpin, data: Vec<u8>) {
    for chunk in data.chunks(40_000 + 7) {
        to.write_all(chunk).await.unwrap();
    }
    to.shutdown().await.unwrap();
}

/// The SHA-256 and the count of each byte to the end.
async fn digest(mut from: impl AsyncRead + Unpin) -> ([u8; 32], usize) {
    let mut hash = Sha256::new();
    let mut buf = vec![0; 50_000];
    let mut n = 0;
    loop {
        let got = from.read(&mut buf).await.unwrap();
        if got == 0 {
            return (hash.finalize().into(), n);
        }
        hash.update(&buf[..got]);
        n += got;
    }
}

fn sha(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// 8 MiB one way and 1 MiB the other way at the same time, at 3 % loss,
/// 1 % reorder and 1 % duplicates: each byte comes once and in order.
#[tokio::test(start_paused = true)]
async fn eight_mib_cross_three_percent_loss_whole_and_in_order() {
    let (sa, sb) = shims(GATE, 3);
    let (a, b) = ends(&sa, &sb, ArqSettings::default());
    let (ha, hb) = (a.handle(), b.handle());
    let (big, small) = (bytes(8 << 20, 1), bytes(1 << 20, 2));
    let (a_read, a_write) = tokio::io::split(a);
    let (b_read, b_write) = tokio::io::split(b);
    let moves = async {
        tokio::join!(send_all(a_write, big.clone()), send_all(b_write, small.clone()), digest(b_read), digest(a_read))
    };
    let ((), (), (at_b, nb), (at_a, na)) = timeout(LIMIT, moves).await.expect("the transfer in time");
    assert_eq!((nb, at_b), (big.len(), sha(&big)));
    assert_eq!((na, at_a), (small.len(), sha(&small)));
    let lost = sa.stats().lost + sb.stats().lost;
    assert!(lost > 200, "{lost} datagrams lost");
    assert!(sa.stats().reordered > 0 && sa.stats().duplicated > 0, "{:?}", sa.stats());
    let (a_stats, b_stats) = (ha.stats(), hb.stats());
    assert!(a_stats.retransmitted > 0 && b_stats.duplicates > 0, "{a_stats:?} {b_stats:?}");
    assert!(ha.error().is_none() && hb.error().is_none());
}

/// A link with two streams of 4 MiB runs over the ARQ at the faults of the
/// gate; each stream comes whole.
#[tokio::test(start_paused = true)]
async fn a_link_runs_over_the_arq_at_three_percent_loss() {
    let (sa, sb) = shims(GATE, 11);
    let (a, b) = ends(&sa, &sb, ArqSettings::default());
    let dialer = Link::new(a, Role::Dialer, Settings::default()).unwrap();
    let listener = Link::new(b, Role::Listener, Settings::default()).unwrap();
    let data = [bytes(4 << 20, 21), bytes(4 << 20, 22)];
    let streams = [dialer.open().unwrap(), dialer.open().unwrap()];
    let mut senders = Vec::new();
    for (s, d) in streams.into_iter().zip(data.clone()) {
        senders.push(tokio::spawn(send_all(s, d)));
    }
    let mut got = Vec::new();
    for _ in 0..2 {
        let s = listener.accept().await.unwrap();
        got.push((s.id(), tokio::spawn(digest(s))));
    }
    for (id, task) in got {
        let (hash, n) = timeout(LIMIT, task).await.expect("the transfer in time").unwrap();
        let want = &data[(id / 2) as usize];
        assert_eq!((n, hash), (want.len(), sha(want)), "the stream {id}");
    }
    for s in senders {
        s.await.unwrap();
    }
    assert!(dialer.ended().is_none() && listener.ended().is_none());
    assert!(sa.stats().lost + sb.stats().lost > 100);
}

/// The peer goes silent while bytes move: the ARQ ends with
/// `arq-timeout` after the retries of its oldest frame, and the read and
/// the write fail with `TimedOut` and the typed error inside.
#[tokio::test(start_paused = true)]
async fn a_silent_peer_ends_with_arq_timeout() {
    let (sa, sb) = shims(Faults::NONE, 5);
    let (mut a, mut b) = ends(&sa, &sb, ArqSettings::default());
    let handle: ArqHandle = a.handle();
    a.write_all(b"before").await.unwrap();
    let mut buf = [0; 6];
    b.read_exact(&mut buf).await.unwrap();
    assert!(handle.srtt().is_some());
    sa.set_faults(Faults::SILENT);
    sb.set_faults(Faults::SILENT);
    let start = Instant::now();
    a.write_all(b"after").await.unwrap();
    let e = a.read(&mut buf).await.unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
    let inner = e.get_ref().and_then(|e| e.downcast_ref::<ArqError>()).cloned().unwrap();
    let Some(ArqError::Timeout { retries: 8, waited, .. }) = handle.error() else { panic!("{:?}", handle.error()) };
    assert_eq!(handle.error(), Some(inner));
    // The RTO floor of 200 ms, doubled up to 8 s: 36.6 s in all.
    assert_eq!(waited, Duration::from_millis(36_600));
    assert_eq!(start.elapsed(), waited);
    assert_eq!(a.write(b"x").await.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    let failure = pod_core::Failure::from(handle.error().unwrap());
    assert_eq!(failure.class, pod_core::Class::Timeout);
}

/// A link over the ARQ whose peer goes silent ends: by its own keepalive
/// (30 s) before the ARQ's timeout, and its streams fail.
#[tokio::test(start_paused = true)]
async fn a_link_over_a_silent_arq_ends() {
    let (sa, sb) = shims(Faults::NONE, 6);
    let (a, b) = ends(&sa, &sb, ArqSettings::default());
    let dialer = Link::new(a, Role::Dialer, Settings::default()).unwrap();
    let _listener = Link::new(b, Role::Listener, Settings::default()).unwrap();
    let mut s = dialer.open().unwrap();
    s.write_all(b"hello").await.unwrap();
    sa.set_faults(Faults::SILENT);
    sb.set_faults(Faults::SILENT);
    let e = dialer.closed().await;
    assert_eq!(e.class(), pod_core::Class::Timeout, "{e}");
    assert!(s.read(&mut [0; 1]).await.is_err());
}

/// The first sequence is 100 frames before the wrap, and the rekey is due
/// 256 frames ahead: the ARQ asks at once. The 100 frames hold two whole
/// records of 40007 bytes (40 fragments each) and a part of the third;
/// then the bytes wait at the wrap. After `rekeyed` the rest comes in
/// order.
#[tokio::test(start_paused = true)]
async fn the_bytes_wait_at_the_wrap_until_the_rekey() {
    let (sa, sb) = shims(Faults::NONE, 7);
    let settings = ArqSettings { start: u32::MAX - 99, rekey_ahead: 256, ..ArqSettings::default() };
    let (a, mut b) = ends(&sa, &sb, settings);
    let handle = a.handle();
    timeout(Duration::from_secs(1), handle.rekey_wanted()).await.unwrap();
    let data = bytes(300 << 10, 8);
    let writer = tokio::spawn(send_all(a, data.clone()));
    let before = 2 * 40_007;
    let mut got = vec![0; before];
    timeout(LIMIT, b.read_exact(&mut got)).await.expect("the bytes before the wrap in time").unwrap();
    assert!(timeout(Duration::from_secs(60), b.read(&mut [0; 1])).await.is_err());
    assert_eq!(handle.stats().rekeys, 0);
    assert_eq!(handle.stats().sent, 100);
    handle.rekeyed();
    let (hash, n) = timeout(LIMIT, digest(&mut b)).await.expect("the bytes after the wrap in time");
    assert_eq!((before + n, hash), (data.len(), sha(&data[before..])));
    assert_eq!(got, data[..before]);
    timeout(LIMIT, writer).await.unwrap().unwrap();
    assert_eq!(handle.stats().rekeys, 1);
    assert!(!handle.rekey_due());
}

/// A shutdown returns when the peer acknowledged the `FIN`; the peer reads
/// to the end. A drop after it lets the task of the stream end.
#[tokio::test(start_paused = true)]
async fn a_shutdown_waits_for_the_acknowledgement_of_the_fin() {
    let (sa, sb) = shims(GATE, 9);
    let (mut a, mut b) = ends(&sa, &sb, ArqSettings::default());
    a.write_all(b"the last bytes").await.unwrap();
    a.shutdown().await.unwrap();
    let mut got = Vec::new();
    b.read_to_end(&mut got).await.unwrap();
    assert_eq!(got, b"the last bytes");
    b.shutdown().await.unwrap();
    assert!(a.read(&mut [0; 4]).await.unwrap() == 0);
    assert_eq!(a.write(b"x").await.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
    assert_eq!(sa.mtu(), memory::DEFAULT_MTU);
}

/// A carrier that loses the first `ACK` whose next frame is `next`.
struct LoseAck {
    inner: MemoryDatagrams,
    next: u32,
    lost: AtomicBool,
}

impl Datagrams for LoseAck {
    fn mtu(&self) -> usize {
        self.inner.mtu()
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        let ack = matches!(Datagram::decode(datagram), Ok(Datagram::Ack { next, .. }) if next == self.next);
        if ack && !self.lost.swap(true, Ordering::SeqCst) {
            return Box::pin(async { Ok(()) });
        }
        self.inner.send(datagram)
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        self.inner.recv()
    }
}

/// The `ACK` of the peer's `FIN` is lost, and this end then drops its
/// stream: its task answers the `FIN` that comes again, so the shutdown of
/// the peer ends well.
#[tokio::test(start_paused = true)]
async fn a_dropped_end_answers_the_peer_until_it_is_quiet() {
    let (near, far) = memory::pair(memory::DEFAULT_MTU, memory::DEFAULT_DEPTH);
    // The frame 0 is the byte, the frame 1 the `FIN`.
    let far = LoseAck { inner: far, next: 2, lost: AtomicBool::new(false) };
    let mut a = ArqStream::new(near, ArqSettings::default()).unwrap();
    let mut b = ArqStream::new(far, ArqSettings::default()).unwrap();
    let peer = async move {
        let mut got = Vec::new();
        b.read_to_end(&mut got).await.unwrap();
        b.shutdown().await.unwrap();
        drop(b);
        got
    };
    let near = async {
        a.write_all(b"x").await.unwrap();
        a.shutdown().await
    };
    let (got, shut) = timeout(LIMIT, async { tokio::join!(peer, near) }).await.expect("the ends in time");
    assert_eq!(got, b"x");
    shut.expect("the peer answers the `FIN` after its drop");
    // The lost `ACK` covered the byte and the `FIN`: each went again.
    assert_eq!(a.handle().stats().retransmitted, 2);
}
