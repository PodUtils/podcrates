//! One session over an up carrier and a down carrier (T-500,
//! PodUtils/podnet#521, PodUtils/podnet@T-302): 1 MiB down a fast half and
//! 64 KiB up a half that loses datagrams (the ARQ over the `Lossy` shim)
//! cross whole in a link; a half that goes silent or fails ends the session
//! on each side with the loss of that half named; the listener joins the
//! halves of each session alone, refuses halves of two keys, and gives the
//! end of the bytes of each side; a link over each end names the lost half
//! in each of many rounds under load; the address and the facts.

mod common;

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use common::{limit, pattern};
use pod_identity::{Identity, PublicKey};
use pod_link::arq::{ArqSettings, ArqStream};
use pod_link::datagram::lossy::{Faults, Lossy};
use pod_link::datagram::memory;
use pod_link::split::{self, Half, Split};
use pod_link::{
    Accepted, BoxIo, Carrier, CarrierError, CarrierErrorKind, Cost, Facts, Link, Listener, Pending, Role, Settings,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};
use tokio::sync::mpsc;
use tokio::time::timeout;

fn key(n: u8) -> PublicKey {
    Identity::from_seed(&[n; 32]).public()
}

/// The state of every path of a carrier of the test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Open,
    /// Reads wait for ever and writes go nowhere: a cut network.
    Silent,
    /// Each read and write fails.
    Closed,
}

#[derive(Default)]
struct Switch {
    mode: Mutex<Option<Mode>>,
    wakers: Mutex<Vec<Waker>>,
}

impl Switch {
    fn mode(&self) -> Mode {
        self.mode.lock().unwrap().unwrap_or(Mode::Open)
    }

    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = Some(mode);
        self.wakers.lock().unwrap().drain(..).for_each(Waker::wake);
    }

    fn wait(&self, cx: &Context<'_>) {
        self.wakers.lock().unwrap().push(cx.waker().clone());
    }
}

/// One end of a path of the test, under its switch.
struct Cut {
    inner: DuplexStream,
    switch: Arc<Switch>,
}

impl AsyncRead for Cut {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.switch.mode() {
            Mode::Open => {
                let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
                if polled.is_pending() {
                    self.switch.wait(cx);
                }
                polled
            }
            Mode::Silent => {
                self.switch.wait(cx);
                Poll::Pending
            }
            Mode::Closed => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
        }
    }
}

impl AsyncWrite for Cut {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        match self.switch.mode() {
            Mode::Open => Pin::new(&mut self.inner).poll_write(cx, data),
            Mode::Silent => Poll::Ready(Ok(data.len())),
            Mode::Closed => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

type Doors = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Accepted>>>>;

/// A carrier of the test: `NAME@N` dials the door `NAME`, and its accept
/// gives the key of the identity N. `lossy` puts the ARQ over datagrams
/// that lose 3 % under the path; the dialed end of each path is under
/// `switch`, the accepted end under `far`.
#[derive(Default)]
struct Paths {
    doors: Doors,
    switch: Arc<Switch>,
    far: Arc<Switch>,
    lossy: bool,
    seed: AtomicU64,
}

struct Door(mpsc::UnboundedReceiver<Accepted>, String);

impl Listener for Door {
    fn address(&self) -> String {
        self.1.clone()
    }

    fn accept(&mut self) -> Pending<'_, BoxIo> {
        Box::pin(async move { Ok(Listener::accept_peer(self).await?.io) })
    }

    fn accept_peer(&mut self) -> Pending<'_, Accepted> {
        Box::pin(async move {
            self.0.recv().await.ok_or_else(|| CarrierError::new("paths", CarrierErrorKind::NotFound, "no more"))
        })
    }
}

impl Carrier for Paths {
    fn name(&self) -> &'static str {
        if self.lossy { "lossy" } else { "paths" }
    }

    fn facts(&self) -> Facts {
        Facts::with_cost(if self.lossy { Cost::High } else { Cost::Low })
    }

    fn available(&self) -> Result<(), CarrierError> {
        Ok(())
    }

    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (name, n) = address.split_once('@').unwrap();
            let door = self.doors.lock().unwrap().get(name).cloned();
            let door = door.ok_or_else(|| CarrierError::new("paths", CarrierErrorKind::NotFound, name.to_string()))?;
            let (near, far): (BoxIo, BoxIo) = if self.lossy {
                let seed = self.seed.fetch_add(1, Ordering::SeqCst);
                let faults = Faults { loss: 0.03, reorder: 0.01, duplicate: 0.01 };
                let (a, b) = memory::pair(memory::DEFAULT_MTU, memory::DEFAULT_DEPTH);
                let near = ArqStream::new(Lossy::new(a, faults, seed), ArqSettings::default()).unwrap();
                let far = ArqStream::new(Lossy::new(b, faults, seed ^ 0x5eed), ArqSettings::default()).unwrap();
                (Box::new(near), Box::new(far))
            } else {
                let (a, b) = tokio::io::duplex(256 << 10);
                (
                    Box::new(Cut { inner: a, switch: self.switch.clone() }),
                    Box::new(Cut { inner: b, switch: self.far.clone() }),
                )
            };
            let peer = Some(key(n.parse().unwrap()));
            door.send(Accepted { io: far, peer })
                .map_err(|_| CarrierError::new("paths", CarrierErrorKind::NotFound, "gone"))?;
            Ok(near)
        })
    }

    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            let (tx, rx) = mpsc::unbounded_channel();
            self.doors.lock().unwrap().insert(address.to_string(), tx);
            Ok(Box::new(Door(rx, address.to_string())) as Box<dyn Listener>)
        })
    }
}

impl Paths {
    /// Set the dialed ends to `near` and the accepted ends to `far`.
    fn set(&self, near: Mode, far: Mode) {
        self.switch.set(near);
        self.far.set(far);
    }
}

/// A split of `up` and `down` with short beats.
fn split(up: Arc<Paths>, down: Arc<Paths>) -> Split {
    let mut s = Split::new(up, down);
    (s.beat, s.lost_after, s.join_timeout) =
        (Duration::from_millis(50), Duration::from_millis(400), Duration::from_secs(2));
    s
}

/// A dial of `s` with the identity `n` on each half, and its accept.
async fn pair(s: &Split, door: &mut split::SplitListener, n: u8) -> (BoxIo, Accepted) {
    let (up, down) = (format!("u@{n}"), format!("d@{n}"));
    let both = async { tokio::join!(s.connect(&up, &down), door.accept_peer()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the join in time");
    (near.expect("the dial"), far.expect("the join"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_mib_down_a_fast_half_and_64_kib_up_a_lossy_half_cross_whole() {
    let up = Arc::new(Paths { lossy: true, ..Paths::default() });
    let s = Split::new(up, Arc::new(Paths::default()));
    let mut door = s.bind("u", "d").await.unwrap();
    assert_eq!(door.address(), "u|d");
    let (near, far) = pair(&s, &mut door, 2).await;
    assert_eq!(far.peer, Some(key(2)), "the accept gives the key of the halves");
    let dialer = Link::new(near, Role::Dialer, Settings::default()).unwrap();
    let listener = Link::new(far.io, Role::Listener, Settings::default()).unwrap();
    let (up_data, down_data) = (pattern(1, 64 << 10), pattern(2, 1 << 20));
    let serve = {
        let down_data = down_data.clone();
        async move {
            let mut stream = listener.accept().await.unwrap();
            let mut got = vec![0u8; 64 << 10];
            stream.read_exact(&mut got).await.unwrap();
            stream.write_all(&down_data).await.unwrap();
            stream.shutdown().await.unwrap();
            (got, listener)
        }
    };
    let ask = async {
        let mut stream = dialer.open().unwrap();
        stream.write_all(&up_data).await.unwrap();
        let mut back = Vec::new();
        stream.read_to_end(&mut back).await.unwrap();
        back
    };
    let ((got, _listener), back) =
        timeout(limit(), async { tokio::join!(serve, ask) }).await.expect("the bytes in time");
    assert!(got == up_data, "64 KiB up came whole");
    assert!(back == down_data, "1 MiB down came whole");
    dialer.close().await;
}

/// Kill the `half` of a live session with `mode`: each side ends with the
/// loss of that half.
async fn kill(half: Half, mode: Mode) {
    let (up, down) = (Arc::new(Paths::default()), Arc::new(Paths::default()));
    let s = split(up.clone(), down.clone());
    let mut door = s.bind("u", "d").await.unwrap();
    let (mut near, far) = pair(&s, &mut door, 2).await;
    let mut far = far.io;
    near.write_all(b"ping").await.unwrap();
    let mut got = [0u8; 4];
    timeout(limit(), far.read_exact(&mut got)).await.expect("the first bytes in time").unwrap();
    // A silent session lives on its beats.
    tokio::time::sleep(s.lost_after * 2).await;
    far.write_all(b"pong").await.unwrap();
    timeout(limit(), near.read_exact(&mut got)).await.expect("the bytes after the silence in time").unwrap();
    let started = Instant::now();
    match half {
        Half::Up => up.set(mode, mode),
        Half::Down => down.set(mode, mode),
    }
    for (side, io) in [("the dialer", &mut near), ("the listener", &mut far)] {
        let e = timeout(limit(), io.read(&mut got)).await.expect("the end in time").unwrap_err();
        let lost = split::lost(&e).unwrap_or_else(|| panic!("{side}: no loss in {e}"));
        assert_eq!(lost.half, half, "{side}: {e}");
        assert_eq!(e.kind(), io::ErrorKind::ConnectionAborted);
        assert_eq!(split::lost_in(&e.to_string()), Some(half), "{e}");
    }
    if mode == Mode::Silent {
        // The silence started at the last frame, one sleep of a beat before
        // the cut or less. The timer of Windows ticks each 15.625 ms, so a
        // sleep of 50 ms there takes 62.5 ms: the bound allows two beats.
        let waited = started.elapsed();
        assert!(waited >= s.lost_after - 2 * s.beat, "a silence is a loss after lost_after alone: {waited:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silent_up_half_ends_the_session_with_up_carrier_lost_on_each_side() {
    kill(Half::Up, Mode::Silent).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_down_half_ends_the_session_with_down_carrier_lost_on_each_side() {
    kill(Half::Down, Mode::Closed).await;
}

/// The rounds of the race of a loss and the end of the bytes.
const ROUNDS: usize = 240;

/// Close `half` of a session of two links after 3 bytes: at each end, or
/// at the end that sends on the other half while the far end sees
/// silence. The texts of the links that do not name the loss of `half`.
async fn close_under_links(half: Half, at_each_end: bool) -> Vec<String> {
    let (up, down) = (Arc::new(Paths::default()), Arc::new(Paths::default()));
    let s = split(up.clone(), down.clone());
    let mut door = s.bind("u", "d").await.unwrap();
    let (near, far) = pair(&s, &mut door, 2).await;
    let dialer = Link::new(near, Role::Dialer, Settings::default()).unwrap();
    let listener = Link::new(far.io, Role::Listener, Settings::default()).unwrap();
    let mut stream = dialer.open().unwrap();
    stream.write_all(b"one").await.unwrap();
    let mut accepted = timeout(limit(), listener.accept()).await.expect("the stream in time").unwrap();
    let mut three = [0u8; 3];
    timeout(limit(), accepted.read_exact(&mut three)).await.expect("the bytes in time").unwrap();
    assert_eq!(&three, b"one");
    match (half, at_each_end) {
        (Half::Up, true) => up.set(Mode::Closed, Mode::Closed),
        (Half::Down, true) => down.set(Mode::Closed, Mode::Closed),
        // The listener sends down and the dialer sends up.
        (Half::Up, false) => up.set(Mode::Silent, Mode::Closed),
        (Half::Down, false) => down.set(Mode::Closed, Mode::Silent),
    }
    let mut wrong = Vec::new();
    for (side, link) in [("the dialer", &dialer), ("the listener", &listener)] {
        let why = timeout(limit(), link.closed()).await.expect("the end in time").to_string();
        if split::lost_in(&why) != Some(half) {
            wrong.push(format!("{side}: {why}"));
        }
    }
    wrong
}

/// A link closes its byte stream when the loss of a half ends it; the
/// split must not send that close as the end of its bytes (T-639,
/// PodUtils/podnet#572). Tasks that spin load the runtime so that the
/// two ends see the loss at different times.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_closed_half_ends_each_link_with_the_loss_of_that_half_in_each_round_under_load() {
    let busy = Arc::new(AtomicBool::new(true));
    let load: Vec<_> = (0..8)
        .map(|_| {
            let busy = busy.clone();
            tokio::spawn(async move {
                while busy.load(Ordering::Relaxed) {
                    let at = Instant::now();
                    while at.elapsed() < Duration::from_micros(200) {
                        std::hint::spin_loop();
                    }
                    tokio::task::yield_now().await;
                }
            })
        })
        .collect();
    let mut failed = Vec::new();
    for round in 0..ROUNDS {
        let half = if round % 2 == 0 { Half::Up } else { Half::Down };
        let at_each_end = round % 4 < 2;
        let form = if at_each_end { "at each end" } else { "at one end" };
        for text in close_under_links(half, at_each_end).await {
            failed.push(format!("round {round}, the {} half closed {form}, {text}", half.name()));
        }
    }
    busy.store(false, Ordering::Relaxed);
    for task in load {
        task.await.unwrap();
    }
    assert!(failed.is_empty(), "{} ends of {ROUNDS} rounds named no lost half: {:#?}", failed.len(), failed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_listener_joins_the_halves_of_each_session_and_refuses_halves_of_two_keys() {
    let s = split(Arc::new(Paths::default()), Arc::new(Paths::default()));
    let mut door = s.bind("u", "d").await.unwrap();
    // Two sessions at once: each joins its own halves.
    let dials = async { tokio::join!(s.connect("u@2", "d@2"), s.connect("u@3", "d@3")) };
    let accepts = async { (door.accept_peer().await.unwrap(), door.accept_peer().await.unwrap()) };
    let ((a, b), (x, y)) = timeout(limit(), async { tokio::join!(dials, accepts) }).await.expect("in time");
    let mut nears = HashMap::from([(key(2), a.unwrap()), (key(3), b.unwrap())]);
    for far in [x, y] {
        let (peer, mut io) = (far.peer.unwrap(), far.io);
        let near = nears.get_mut(&peer).unwrap();
        near.write_all(peer.hex().as_bytes()).await.unwrap();
        near.shutdown().await.unwrap();
        let mut got = Vec::new();
        let read = timeout(limit(), io.read_to_end(&mut got)).await.expect("the end of the bytes up in time");
        read.expect("the end of the bytes up, not a loss");
        assert_eq!(got, peer.hex().into_bytes(), "the up half of each session joined its own down half");
        io.write_all(b"back").await.unwrap();
        io.shutdown().await.unwrap();
        let mut back = Vec::new();
        timeout(limit(), near.read_to_end(&mut back)).await.expect("the end of the bytes down in time").unwrap();
        assert_eq!(back, b"back");
    }
    let (dialed, refused) =
        timeout(limit(), async { tokio::join!(s.connect("u@2", "d@3"), door.accept_peer()) }).await.expect("in time");
    let e = refused.expect_err("halves of two keys are refused");
    assert_eq!(e.kind, CarrierErrorKind::Refused, "{e}");
    assert!(e.to_string().contains("the up half proved"), "{e}");
    assert!(dialed.is_err(), "no join comes down");
    let (_near, _far) = pair(&s, &mut door, 4).await;
}

#[tokio::test]
async fn the_address_the_settings_and_the_facts() {
    let s = Split::new(Arc::new(Paths { lossy: true, ..Paths::default() }), Arc::new(Paths::default()));
    assert_eq!(Split::addresses("a@1|b@2").unwrap(), ("a@1", "b@2"));
    for bad in ["a", "|b", "a|"] {
        assert_eq!(Split::addresses(bad).err().unwrap().kind, CarrierErrorKind::Usage, "{bad}");
    }
    let facts = s.facts();
    assert_eq!((s.name(), facts.cost, facts.reliable, facts.duplex), ("split", Cost::High, true, true));
    s.available().unwrap();
    let mut wrong = Split::new(Arc::new(Paths::default()), Arc::new(Paths::default()));
    wrong.lost_after = wrong.beat;
    assert_eq!(wrong.check().err().unwrap().kind, CarrierErrorKind::Usage);
    let _door = s.bind("u", "d").await.unwrap();
    let e = s.connect("u@1", "nowhere@1").await.err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::NotFound, "{e}");
    assert!(e.to_string().contains("the down half"), "{e}");
}
