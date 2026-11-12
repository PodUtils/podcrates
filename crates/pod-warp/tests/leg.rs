//! The race of the direct path and the relay leg (PodUtils/podnet issues 76
//! and 126), against the stand-in API and the stand-in edge on the
//! loopback, with two dials of the test: the direct path wins when it is
//! fast; the relay wins when the direct path hangs, and its budget goes to
//! the network; the relay goes first when the leg says so; a failure of
//! both legs names both causes.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use common::edge::{self, Edge, Route};
use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Stage, Target};
use pod_testkit::stand;
use pod_warp::leg::{self, Leg, RELAY_DELAY, Relay};
use pod_warp::net::Renew;
use pod_warp::{Meter, Warp};
use tokio::time::Instant;

/// The name of the relay of the tests.
const RELAY: &str = "wss://relay.test/";
/// Less than the time limit of a connect of the tests (10 s): a relay that
/// opens in this time did not wait for the direct path to fail.
const QUICK: Duration = Duration::from_secs(5);

/// A dial that never connects. It counts each dial, and each dial that its
/// caller dropped.
#[derive(Default)]
struct Hang {
    dials: AtomicUsize,
    dropped: Arc<AtomicUsize>,
}

struct Dropped(Arc<AtomicUsize>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl Dial for Hang {
    fn label(&self) -> String {
        "the hung dial of the test".into()
    }

    fn dial<'a>(&'a self, _target: &'a Target, _deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        self.dials.fetch_add(1, Ordering::SeqCst);
        let guard = Dropped(self.dropped.clone());
        Box::pin(async move {
            let _guard = guard;
            std::future::pending().await
        })
    }
}

/// A relay that refuses each stream.
struct Refuse;

impl Dial for Refuse {
    fn label(&self) -> String {
        "the refusing relay of the test".into()
    }

    fn dial<'a>(&'a self, _target: &'a Target, _deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async {
            Err(Failure::new(Stage::Connect, Class::Refused, "the relay of the test refused the stream"))
        })
    }
}

/// The stand-in API whose records name the doors with the key of the edge,
/// and the edge with an echo route.
async fn world() -> (stand::warp::Api, stand::Stand, Edge) {
    let echo = edge::echo_server().await;
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    let edge = Edge::start(api.clone(), vec![Route { port: 80, outside: echo }]).await;
    api.serve_key(&edge.pem());
    let served = stand::http(api.handler()).unwrap();
    (api, served, edge)
}

/// A dial that carries both doors to `to`.
fn to(to: SocketAddr) -> Arc<common::Moved> {
    common::Moved::new(vec![(edge::door_v4(), to), (edge::door_v6(), to)])
}

/// A port of the loopback where nothing listens.
fn dead() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

/// A device registered at `served`, whose sessions take the direct dial
/// `direct` and the relay `relay`.
async fn device(served: &stand::Stand, direct: Arc<dyn Dial>, relay: Relay) -> (Warp, Arc<common::Kept>) {
    let (mut settings, kept) = common::settings(&format!("http://{}", served.addr()), direct);
    settings.relay = Some(relay);
    (Warp::register(settings).await.unwrap(), kept)
}

fn decided(kept: &common::Kept, outcome: &str) -> bool {
    kept.decisions.lock().unwrap().contains(&("warp.leg".into(), outcome.into()))
}

/// A direct path that opens before the delay of the relay wins; the relay
/// is not dialed, and the leg file does not change.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_direct_path_wins_when_it_is_fast() {
    pod_testkit::need_loopback!();
    let (_api, served, edge) = world().await;
    let (direct, through) = (to(edge.addr), to(edge.addr));
    let (warp, kept) = device(&served, direct.clone(), Relay::new(through.clone(), RELAY)).await;
    let started = Instant::now();
    warp.open_session().await.unwrap();
    assert!(started.elapsed() < RELAY_DELAY, "{:?}", started.elapsed());
    assert_eq!(direct.dials.load(Ordering::SeqCst), 1);
    assert_eq!(through.dials.load(Ordering::SeqCst), 0, "the relay waits for its delay");
    assert_eq!((warp.leg(), warp.relay_first(), warp.take_leg_change()), (Some(Leg::Direct), false, None));
    assert!(decided(&kept, "direct"), "{:?}", kept.decisions.lock().unwrap());
}

/// A direct path that hangs loses to the relay after its delay: the dial of
/// the direct path is dropped, the relay goes first next time, and the
/// budget of the meter of the relay goes to the network.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_relay_wins_when_the_direct_path_hangs() {
    pod_testkit::need_loopback!();
    let (_api, served, edge) = world().await;
    let (hang, through) = (Arc::new(Hang::default()), to(edge.addr));
    let (metered, spent) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
    let mut relay = Relay::new(through.clone(), RELAY);
    relay.delay = Duration::from_millis(300);
    let (count, flag) = (metered.clone(), spent.clone());
    let meter: Meter = Arc::new(move |io| {
        count.fetch_add(1, Ordering::SeqCst);
        let flag = flag.clone();
        let budget: Renew = Arc::new(move || flag.load(Ordering::SeqCst));
        (io, budget)
    });
    relay.meter = Some(meter);
    let (warp, kept) = device(&served, hang.clone(), relay).await;
    let started = Instant::now();
    let net = warp.connect().await.unwrap();
    let took = started.elapsed();
    assert!(took >= Duration::from_millis(300) && took < QUICK, "{took:?}");
    assert_eq!(hang.dials.load(Ordering::SeqCst), 1);
    assert_eq!(hang.dropped.load(Ordering::SeqCst), 1, "the loser is dropped");
    assert_eq!((through.dials.load(Ordering::SeqCst), metered.load(Ordering::SeqCst)), (1, 1));
    assert_eq!((warp.leg(), warp.relay_first()), (Some(Leg::Relay), true));
    assert_eq!((warp.take_leg_change(), warp.take_leg_change()), (Some(Leg::Relay), None));
    assert!(decided(&kept, "relay"), "{:?}", kept.decisions.lock().unwrap());
    assert!(!net.renew_due());
    spent.store(true, Ordering::SeqCst);
    assert!(net.renew_due(), "the budget of the relay session");
}

/// PodUtils/podnet issue 126: a leg file that names the relay puts it first
/// (its short delay); a direct path that opens before it takes the first
/// place back, and the leg file changes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_relay_goes_first_when_the_leg_says_so() {
    pod_testkit::need_loopback!();
    let (_api, served, edge) = world().await;
    let now = 1_800_000_000;
    let first = leg::relay_first_from(&leg::leg_text(Leg::Relay, now - 60, RELAY), now, RELAY);
    assert!(first);
    let through = to(edge.addr);
    let mut relay = Relay::new(through.clone(), RELAY);
    (relay.delay, relay.delay_known) = (Duration::from_secs(60), Duration::from_millis(200));
    let (warp, kept) = device(&served, Arc::new(Hang::default()), relay.clone()).await;
    let warp = warp.with_relay_first(first);
    let started = Instant::now();
    warp.open_session().await.unwrap();
    assert!(started.elapsed() < QUICK, "{:?}", started.elapsed());
    assert_eq!((warp.leg(), warp.take_leg_change()), (Some(Leg::Relay), None));
    assert!(decided(&kept, "relay-first"), "{:?}", kept.decisions.lock().unwrap());
    // A direct path that works again wins before the short delay.
    relay.delay_known = Duration::from_secs(60);
    let mut settings = warp.settings().clone();
    settings.way.dial = to(edge.addr);
    settings.relay = Some(relay);
    let again = Warp::from_parts(settings, warp.device().clone(), warp.identity().clone()).with_relay_first(true);
    again.open_session().await.unwrap();
    assert_eq!((again.leg(), again.relay_first()), (Some(Leg::Direct), false));
    assert_eq!(again.take_leg_change(), Some(Leg::Direct));
    assert_eq!(through.dials.load(Ordering::SeqCst), 1, "the relay of the first session alone");
}

/// When the direct path fails, the relay starts at once, before its delay;
/// when it fails too, the failure names both causes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failure_of_both_legs_names_both_causes() {
    pod_testkit::need_loopback!();
    let (_api, served, _edge) = world().await;
    let mut relay = Relay::new(Arc::new(Refuse), RELAY);
    relay.delay = Duration::from_secs(60);
    let (warp, kept) = device(&served, to(dead()), relay).await;
    let started = Instant::now();
    let failure = warp.open_session().await.err().expect("no leg opens");
    assert!(started.elapsed() < QUICK, "the relay waited for its delay: {:?}", started.elapsed());
    let chain = failure.chain();
    assert!(chain.contains("TCP to the MASQUE front door"), "{chain}");
    assert!(chain.contains(&format!("through the relay {RELAY}")), "{chain}");
    assert!(chain.contains("the relay of the test refused the stream"), "{chain}");
    assert_eq!(failure.stage, Stage::Connect, "the stage of the direct path");
    assert!(kept.kinds().contains(&"warp.leg"), "{:?}", kept.kinds());
    assert_eq!((warp.leg(), warp.take_leg_change()), (None, None));
}
