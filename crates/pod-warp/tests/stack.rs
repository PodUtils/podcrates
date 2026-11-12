//! The stack of a session as podnet's `src/warp/ip.rs` tunes it (T-335),
//! against the stand-in API and the stand-in edge on the loopback: the
//! check of the data path and the door that carried nothing, the TCP burst
//! that the edge sees, the congestion controllers, the counters of the link
//! and the budget of a relay.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::edge::{self, Edge, Route};
use pod_core::{Class, Dial, Target};
use pod_testkit::stand;
use pod_warp::net::{CongestionControl, DeviceStats};
use pod_warp::{DataPathCheck, Settings, Warp, WarpNet};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const LIMIT: Duration = Duration::from_secs(20);
/// The port of the echo service inside the tunnel.
const ECHO_TCP: u16 = 80;
/// The payload of a TCP segment at the MTU of 1280 with no option.
const SEGMENT: usize = 1280 - 20 - 20;

/// The stand-in API whose records name `door_v4` and `door_v6` with the key
/// of the edge, and the edge with an echo route; the moved dial carries
/// both doors to the edge.
async fn world() -> (stand::warp::Api, stand::Stand, Edge, Arc<common::Moved>) {
    let echo = edge::echo_server().await;
    let api = stand::warp::Api::new("", "192.0.2.1", "2001:db8::1");
    let edge = Edge::start(api.clone(), vec![Route { port: ECHO_TCP, outside: echo }]).await;
    api.serve_key(&edge.pem());
    let served = stand::http(api.handler()).unwrap();
    let moved = common::Moved::new(vec![(edge::door_v4(), edge.addr), (edge::door_v6(), edge.addr)]);
    (api, served, edge, moved)
}

/// A check with short times, for a session that carries nothing.
fn quick_check() -> DataPathCheck {
    DataPathCheck { limit: Duration::from_millis(800), resend: Duration::from_millis(250), ..DataPathCheck::default() }
}

/// Send `data` through `net` to the echo route and read it back.
async fn echo(net: &WarpNet, data: &[u8]) -> Vec<u8> {
    let target = Target::Addr("203.0.113.9:80".parse().unwrap());
    let connected = net.dial(&target, tokio::time::Instant::now() + LIMIT).await.unwrap();
    let (mut read, mut write) = tokio::io::split(connected.io);
    let sent = data.to_vec();
    let writer = tokio::spawn(async move {
        write.write_all(&sent).await.unwrap();
        write
    });
    let mut got = vec![0u8; data.len()];
    tokio::time::timeout(LIMIT, read.read_exact(&mut got)).await.expect("the echo in the limit").unwrap();
    let _write = writer.await.unwrap();
    got
}

/// A registered device with `settings`.
async fn device(settings: Settings) -> Warp {
    Warp::register(settings).await.unwrap()
}

/// PodUtils/podnet issue 120: a session is up when a DNS question through
/// it gets an answer. A front door that takes the handshake and carries
/// nothing gives a failure that says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_check_of_the_data_path_needs_an_answer() {
    pod_testkit::need_loopback!();
    let (_api, served, edge, moved) = world().await;
    let (settings, kept) = common::settings(&format!("http://{}", served.addr()), moved);
    let warp = device(settings.clone()).await;
    let net = warp.connect().await.unwrap();
    assert!(net.checked().is_some());
    assert!(edge.watch.dns_questions.load(Ordering::SeqCst) >= 1);
    assert!(kept.decisions.lock().unwrap().contains(&("warp.check".into(), "answered".into())));
    let again = net.check_data_path(&DataPathCheck::default()).await.unwrap();
    assert!(again < LIMIT);
    edge.seen.silent.store(true, Ordering::SeqCst);
    let mut silent = settings;
    silent.net.check = Some(quick_check());
    let warp = Warp::from_parts(silent, warp.device().clone(), warp.identity().clone());
    let err = warp.connect().await.err().unwrap();
    assert_eq!(err.class, Class::Unavailable, "{}", err.chain());
    assert!(err.chain().contains("got no answer"), "{}", err.chain());
    assert!(err.chain().contains("1.1.1.1:53"), "{}", err.chain());
}

/// PodUtils/podnet issue 133: the door that answered the handshake and
/// carried nothing goes last for the next session; the other door opens it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_door_that_carries_nothing_goes_last() {
    pod_testkit::need_loopback!();
    let (_api, served, edge, moved) = world().await;
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), moved.clone());
    settings.net.check = Some(quick_check());
    let warp = device(settings).await;
    edge.seen.silent.store(true, Ordering::SeqCst);
    assert!(warp.connect().await.is_err());
    assert_eq!(moved.dials.load(Ordering::SeqCst), 1, "the IPv4 door opened, and carried nothing");
    assert_eq!(warp.won_door(), None);
    edge.seen.silent.store(false, Ordering::SeqCst);
    warp.connect().await.unwrap();
    assert_eq!(moved.dials.load(Ordering::SeqCst), 2, "the IPv6 door first, and alone");
    assert_eq!(warp.won_door(), Some(edge::door_v6()));
}

/// PodUtils/podnet issue 114: a lost question goes again after the resend
/// time, and the check passes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_check_question_is_sent_again() {
    pod_testkit::need_loopback!();
    let (_api, served, edge, moved) = world().await;
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    let resend = Duration::from_millis(300);
    settings.net.check = Some(DataPathCheck { resend, ..DataPathCheck::default() });
    edge.watch.dns_drop.store(1, Ordering::SeqCst);
    let net = device(settings).await.connect().await.unwrap();
    assert!(net.checked().unwrap() >= resend, "{:?}", net.checked());
    assert_eq!(edge.watch.dns_questions.load(Ordering::SeqCst), 2);
}

/// With no check (podnet's `PODNET_WARP_NO_CHECK`), a session that carries
/// nothing opens.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_check_takes_a_session_that_carries_nothing() {
    pod_testkit::need_loopback!();
    let (_api, served, edge, moved) = world().await;
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    settings.net.check = None;
    edge.seen.silent.store(true, Ordering::SeqCst);
    let net = device(settings).await.connect().await.unwrap();
    assert_eq!(net.checked(), None);
    assert_eq!(edge.watch.dns_questions.load(Ordering::SeqCst), 0);
}

/// PodUtils/podnet issue 100: a TCP burst of 2 segments clamps each window
/// that the edge sees to 2 segments; with no burst the window is open.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tcp_burst_clamps_the_window_that_the_edge_sees() {
    pod_testkit::need_loopback!();
    for burst in [None, Some(2)] {
        let (_api, served, edge, moved) = world().await;
        let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
        settings.net.tcp_burst = burst;
        let net = device(settings).await.connect().await.unwrap();
        assert_eq!(echo(&net, b"a window").await, b"a window");
        let window = edge.watch.window_max.load(Ordering::SeqCst);
        match burst {
            Some(n) => assert!(window <= n * SEGMENT, "a window of {window} with {n} segments"),
            None => assert!(window > 2 * SEGMENT, "a window of {window} with no limit"),
        }
    }
}

/// PodUtils/podnet issue 134: a session with Reno and with CUBIC carries
/// each byte of a large echo, in order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_with_each_controller_carries_a_large_echo() {
    pod_testkit::need_loopback!();
    let (_api, served, _edge, moved) = world().await;
    let data: Vec<u8> = (0..(1 << 20) as u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    for cc in [CongestionControl::Reno, CongestionControl::Cubic] {
        let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), moved.clone());
        settings.net.congestion_control = cc;
        let net = device(settings).await.connect().await.unwrap();
        assert!(echo(&net, &data).await == data, "{cc:?}: the echo has other bytes");
    }
}

/// PodUtils/podnet issue 134: the counters of the link of a session, and
/// counters that two sessions share.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_counters_of_the_link_count_each_session() {
    pod_testkit::need_loopback!();
    let (_api, served, _edge, moved) = world().await;
    let (settings, _) = common::settings(&format!("http://{}", served.addr()), moved.clone());
    let own = device(settings).await.connect().await.unwrap();
    echo(&own, &[7u8; 4000]).await;
    let counted = own.link_stats();
    assert!(counted.tx_packets >= 4 && counted.rx_packets >= 4, "{counted:?}");
    assert!(counted.tx_bytes >= 4000 && counted.rx_bytes >= 4000, "{counted:?}");
    let shared = Arc::new(DeviceStats::default());
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    settings.net.link_stats = Some(shared.clone());
    let warp = device(settings).await;
    let first = warp.connect().await.unwrap();
    echo(&first, &[1u8; 4000]).await;
    let after_first = shared.snapshot();
    assert!(after_first.tx_bytes >= 4000, "{after_first:?}");
    let second = warp.connect().await.unwrap();
    echo(&second, &[2u8; 4000]).await;
    let after_second = shared.snapshot();
    assert!(after_second.tx_bytes >= after_first.tx_bytes + 4000, "{after_first:?} then {after_second:?}");
    assert!(first.link_stats().tx_bytes >= after_second.tx_bytes, "one set of counters");
}

/// The budget of the relay of a session is the caller's function; the
/// session reads it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_budget_of_a_relay_says_when_to_renew() {
    pod_testkit::need_loopback!();
    let (_api, served, _edge, moved) = world().await;
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), moved);
    let warp = device(settings.clone()).await;
    assert!(!warp.connect().await.unwrap().renew_due(), "no relay");
    let spent = Arc::new(AtomicBool::new(false));
    let flag = spent.clone();
    settings.net.renew = Some(Arc::new(move || flag.load(Ordering::SeqCst)));
    let warp = Warp::from_parts(settings, warp.device().clone(), warp.identity().clone());
    let net = warp.connect().await.unwrap();
    assert!(!net.renew_due());
    spent.store(true, Ordering::SeqCst);
    assert!(net.renew_due());
}
