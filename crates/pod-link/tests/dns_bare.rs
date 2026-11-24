//! The bare form of the DNS carrier (T-1167, PodUtils/podssh#349): the ARQ
//! with no TLS, for a caller that seals its own stream. A bare link over a
//! real authoritative server on the loopback carries streams and gives no
//! key; a bare end and an end with TLS never pair, over the listener and
//! over the datagrams of one peer; a call of one form refuses a carrier of
//! the other; the bytes of each datagram with and without TLS.

#![cfg(feature = "dns")]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use common::dns::{ZONE, at, bound, carrier};
use common::tcp::id;
use common::{echo, limit, pattern};
use pod_core::Target;
use pod_identity::KeyName;
use pod_link::arq::wire::Datagram;
use pod_link::datagram::memory::{DEFAULT_DEPTH, MemoryDatagrams, pair};
use pod_link::datagram::{DatagramFuture, Datagrams};
use pod_link::dns::Dns;
use pod_link::{BoxIo, Carrier, CarrierErrorKind, Link, Listener, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

/// A bare carrier of the identity `n` under [`ZONE`].
fn bare(n: u8) -> Dns {
    carrier(n, &[]).without_tls()
}

/// The time limit of a handshake that must fail.
const SHORT: Duration = Duration::from_secs(1);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bare_link_over_dns_carries_streams_and_gives_no_key() {
    pod_testkit::need_loopback!();
    let mut door = bound(&bare(1)).await;
    let dialing = bare(2).to(door.local_addr());
    let both = async { tokio::join!(dialing.dial(ZONE), door.accept_peer()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    let (near, far) = (near.expect("the bare dial"), far.expect("the bare accept"));
    assert_eq!(far.peer, None, "a bare listener proves no key");

    let dialer = Link::new(near, Role::Dialer, Settings::default()).unwrap();
    let _echo = echo(Link::new(far.io, Role::Listener, Settings::default()).unwrap());
    let mut stream = dialer.open().unwrap();
    let data = pattern(7, 24 << 10);
    let (mut reader, mut writer) = tokio::io::split(&mut stream);
    let send = async {
        writer.write_all(&data).await.unwrap();
        writer.shutdown().await.unwrap();
    };
    let mut back = Vec::new();
    let (_, read) = timeout(limit(), async { tokio::join!(send, reader.read_to_end(&mut back)) }).await.unwrap();
    read.unwrap();
    assert!(back == data, "the bytes came back whole");
    dialer.close().await;

    let again = bare(3).to(door.local_addr());
    let both = async { tokio::join!(again.dial(ZONE), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the second dial in time");
    near.expect("a second bare dial");
    let e = far.err().expect("accept_key of a bare listener gave a key");
    assert_eq!(e.kind, CarrierErrorKind::Usage, "{e}");
}

/// The dial and the accept of two ends over a memory pair; the time limit
/// of each handshake is [`SHORT`].
async fn over_pair(dial_bare: bool, accept_bare: bool) -> (Result<BoxIo, String>, Result<BoxIo, String>) {
    let (a, b) = pair(1200, DEFAULT_DEPTH);
    let (server, client) = (id(1), id(2));
    let mut dialing = if dial_bare { bare(2) } else { carrier(2, &[]) };
    let mut listening = if accept_bare { bare(1) } else { carrier(1, &[client.public()]) };
    (dialing.handshake_timeout, listening.handshake_timeout) = (SHORT, SHORT);
    let target = Target::Name { host: ZONE.into(), port: 53 };
    let key = KeyName::Key(server.public());
    let near = async {
        match dial_bare {
            true => dialing.connect_bare_over(a, &target).await,
            false => dialing.connect_over(a, &target, &key).await.map(|(io, _)| io),
        }
    };
    let far = async {
        match accept_bare {
            true => listening.accept_bare_over(b, &"the peer").await,
            false => listening.accept_over(b, &"the peer").await.map(|(io, _)| io),
        }
    };
    let (near, far) = timeout(limit(), async { tokio::join!(near, far) }).await.expect("each handshake ends");
    (near.map_err(|e| e.to_string()), far.map_err(|e| e.to_string()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bare_end_and_an_end_with_tls_never_pair() {
    pod_testkit::need_loopback!();
    // Over the datagrams of one peer: each accept refuses the other form.
    let (near, far) = over_pair(false, true).await;
    let why = far.err().expect("the bare accept took a ClientHello");
    assert!(why.contains("no bare hello"), "{why}");
    near.err().expect("a dial with TLS paired with a bare accept");
    let (near, far) = over_pair(true, false).await;
    far.err().expect("the accept with TLS took a bare hello");
    near.err().expect("a bare dial paired with an accept with TLS");
    let (near, far) = over_pair(true, true).await;
    near.expect("a bare dial");
    far.expect("a bare accept");

    // Over the listeners: each dial of the other form ends at its time limit,
    // and no listener gives a client.
    let (server, client) = (id(1), id(2));
    let mut with_tls = bound(&carrier(1, &[client.public()])).await;
    let mut without = bound(&bare(1)).await;
    let mut bare_dial = bare(2).to(with_tls.local_addr());
    let mut tls_dial = carrier(2, &[]).to(without.local_addr());
    (bare_dial.handshake_timeout, tls_dial.handshake_timeout) = (SHORT, SHORT);
    let address = at(&server.public());
    let (a, b) = timeout(limit(), async { tokio::join!(bare_dial.dial(ZONE), tls_dial.dial(&address)) }).await.unwrap();
    let a = a.err().expect("a bare dial paired with a listener with TLS");
    assert_eq!(a.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{a}");
    let b = b.err().expect("a dial with TLS paired with a bare listener");
    assert_eq!(b.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{b}");
    assert!(
        timeout(Duration::from_millis(300), with_tls.accept_key()).await.is_err(),
        "the listener with TLS gave a client"
    );
    assert!(
        timeout(Duration::from_millis(300), without.accept_peer()).await.is_err(),
        "the bare listener gave a client"
    );

    // Each listener still takes an end of its own form.
    let good = carrier(2, &[]).to(with_tls.local_addr());
    let (near, far) =
        timeout(limit(), async { tokio::join!(good.dial(&address), with_tls.accept_key()) }).await.unwrap();
    near.expect("a dial with TLS");
    assert_eq!(far.expect("the listener with TLS").1, client.public());
    let good = bare(2).to(without.local_addr());
    let (near, far) = timeout(limit(), async { tokio::join!(good.dial(ZONE), without.accept_peer()) }).await.unwrap();
    near.expect("a bare dial");
    far.expect("the bare listener");
}

#[tokio::test]
async fn a_call_of_one_form_refuses_a_carrier_of_the_other() {
    let key = KeyName::Key(id(1).public());
    let (a, b) = pair(1200, DEFAULT_DEPTH);
    let target = Target::Name { host: ZONE.into(), port: 53 };
    let server = "127.0.0.1:53".parse().unwrap();
    let errors = [
        bare(2).to(server).connect(ZONE, &key).await.err().expect("connect of a bare carrier"),
        bare(2).connect_over(a, &target, &key).await.err().expect("connect_over of a bare carrier"),
        bare(2).accept_over(b, &"the peer").await.err().expect("accept_over of a bare carrier"),
        bare(2).to(server).dial(&at(&id(1).public())).await.err().expect("a bare dial of KEY@ZONE"),
        carrier(2, &[]).to(server).connect_bare(ZONE).await.err().expect("connect_bare with TLS"),
    ];
    for e in errors {
        assert_eq!(e.kind, CarrierErrorKind::Usage, "{e}");
    }
    let (a, b) = pair(1200, DEFAULT_DEPTH);
    let e = carrier(2, &[]).connect_bare_over(a, &"the peer").await.err().expect("connect_bare_over with TLS");
    assert_eq!(e.kind, CarrierErrorKind::Usage, "{e}");
    let e = carrier(2, &[]).accept_bare_over(b, &"the peer").await.err().expect("accept_bare_over with TLS");
    assert_eq!(e.kind, CarrierErrorKind::Usage, "{e}");
    assert!(format!("{:?}", bare(2)).contains("bare: true"));
}

/// The datagrams and the bytes that one end sent, and of them the `DATA`
/// frames of the ARQ.
#[derive(Default)]
struct Count {
    datagrams: AtomicU64,
    bytes: AtomicU64,
    data: AtomicU64,
    data_bytes: AtomicU64,
}

impl Count {
    fn now(&self) -> [u64; 4] {
        [&self.datagrams, &self.bytes, &self.data, &self.data_bytes].map(|c| c.load(Ordering::SeqCst))
    }
}

/// A memory end that counts its sends.
struct Counted {
    inner: MemoryDatagrams,
    count: Arc<Count>,
}

impl Datagrams for Counted {
    fn mtu(&self) -> usize {
        self.inner.mtu()
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        let n = datagram.len() as u64;
        self.count.datagrams.fetch_add(1, Ordering::SeqCst);
        self.count.bytes.fetch_add(n, Ordering::SeqCst);
        if matches!(Datagram::decode(datagram), Ok(Datagram::Data { .. })) {
            self.count.data.fetch_add(1, Ordering::SeqCst);
            self.count.data_bytes.fetch_add(n, Ordering::SeqCst);
        }
        self.inner.send(datagram)
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        self.inner.recv()
    }
}

/// The unit of the DNS carrier under [`ZONE`]: the base32 budget of a name
/// of 255 bytes under `l.test` (243 characters, 151 bytes) less the session
/// and the counter.
const MTU: usize = 143;

/// The counts of one form, each way: after the handshake, after 100 writes
/// of 32 bytes that the far end echoes, and after 32 KiB up.
async fn measure(bare_form: bool) -> [[[u64; 4]; 2]; 3] {
    let (a, b) = pair(MTU, DEFAULT_DEPTH);
    let (up, down) = (Arc::new(Count::default()), Arc::new(Count::default()));
    let (a, b) = (Counted { inner: a, count: up.clone() }, Counted { inner: b, count: down.clone() });
    let (server, client) = (id(1), id(2));
    let target = Target::Name { host: ZONE.into(), port: 53 };
    let (mut near, mut far) = if bare_form {
        let (dialing, listening) = (bare(2), bare(1));
        let both =
            async { tokio::join!(dialing.connect_bare_over(a, &target), listening.accept_bare_over(b, &"the peer")) };
        let (near, far) = timeout(limit(), both).await.unwrap();
        (near.unwrap(), far.unwrap())
    } else {
        let (dialing, listening) = (carrier(2, &[]), carrier(1, &[client.public()]));
        let key = KeyName::Key(server.public());
        let both =
            async { tokio::join!(dialing.connect_over(a, &target, &key), listening.accept_over(b, &"the peer")) };
        let (near, far) = timeout(limit(), both).await.unwrap();
        (near.unwrap().0, far.unwrap().0)
    };
    let mut out = [[[0; 4]; 2]; 3];
    out[0] = [up.now(), down.now()];
    let mut word = [0u8; 32];
    for i in 0..100u32 {
        near.write_all(&pattern(i, 32)).await.unwrap();
        near.flush().await.unwrap();
        far.read_exact(&mut word).await.unwrap();
        far.write_all(&word).await.unwrap();
        far.flush().await.unwrap();
        near.read_exact(&mut word).await.unwrap();
        assert_eq!(word.to_vec(), pattern(i, 32));
    }
    out[1] = [up.now(), down.now()];
    let bulk = pattern(9, 32 << 10);
    let send = async {
        near.write_all(&bulk).await.unwrap();
        near.flush().await.unwrap();
    };
    let mut got = vec![0u8; bulk.len()];
    let (_, read) = tokio::join!(send, far.read_exact(&mut got));
    read.unwrap();
    assert!(got == bulk);
    out[2] = [up.now(), down.now()];
    out
}

/// One line of the measure: the datagrams and the bytes between two counts,
/// and the mean bytes of a `DATA` frame.
fn line(what: &str, from: [u64; 4], to: [u64; 4]) -> (String, f64) {
    let [d, b, f, fb] = [0, 1, 2, 3].map(|i| to[i] - from[i]);
    let mean = fb as f64 / f.max(1) as f64;
    (format!("{what}: {d} datagrams, {b} bytes; {f} DATA frames, {fb} bytes, {mean:.1} bytes each"), mean)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_bytes_of_each_datagram_with_and_without_tls() {
    let (tls, bare_form) = (measure(false).await, measure(true).await);
    let mut small = [[0.0; 2]; 2];
    for (form, runs) in [(0, tls), (1, bare_form)] {
        for side in 0..2 {
            let what = format!("{}, {}", ["TLS", "bare"][form], ["up", "down"][side]);
            let lines = [
                line(&format!("{what}, the handshake"), [0; 4], runs[0][side]),
                line(&format!("{what}, 100 writes of 32 bytes"), runs[0][side], runs[1][side]),
                line(&format!("{what}, 32 KiB up"), runs[1][side], runs[2][side]),
            ];
            lines.iter().for_each(|(text, _)| eprintln!("{text}"));
            small[form][side] = lines[1].1;
        }
    }
    // The handshake of TLS has a certificate each way; a bare hello is one
    // DATA frame up and one byte down.
    for side in 0..2 {
        assert!(bare_form[0][side][1] * 4 < tls[0][side][1], "the bytes of the bare handshake, {side}");
    }
    // A write of 32 bytes is one record of TLS: 22 bytes more in its frame.
    for (with, without) in small[0].into_iter().zip(small[1]) {
        assert!(with - without >= 20.0, "a DATA frame of a small write: {with:.1} bytes with TLS, {without:.1} bare");
    }
}
