//! The Wisp road past one connection, and the password extension of
//! version 2, against the stand-in of the crate (`pod_egress::standin::wisp`).
//! A `Reopening` road opens a new connection on the next dial after the
//! server closed the old one (planted: the ended connection kept, and the
//! dial fails); one dial opens `TRIES` connections at most, then each dial
//! fails at once until the backoff ends (planted: no bound); a refused
//! credential is not tried again. A server that asks for a password takes
//! the caller's login, and refuses a wrong one with its reason (planted: the
//! login not sent).

#[path = "common/way.rs"]
mod way;

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use pod_core::{Class, Dial, Stage, Target};
use pod_egress::standin::wisp::{Speaks, Stand};
use pod_egress::wisp::{AtEnd, Connect, Login, Reason, Reopening, Wisp, WispError};
use pod_tls::Trust;
use pod_ws::url::WsUrl;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const LIMIT: Duration = Duration::from_secs(30);
/// A made-up password of the stand-in.
const PASSWORD: &str = "made-up-wisp-pw";

fn how(login: Option<Login>) -> Connect {
    Connect { way: way::direct(), trust: Trust::Default, timeout: LIMIT, note: Arc::new(pod_core::NoEvents), login }
}

fn url(port: u16) -> WsUrl {
    WsUrl::parse(&format!("ws://127.0.0.1:{port}/")).unwrap()
}

fn confirming() -> Speaks {
    Speaks::V2 { extensions: vec![(0x05, Vec::new())], refuse: None }
}

/// An echo on the loopback: its port.
async fn echo() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = conn.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    port
}

/// A web server that answers each request with 503: its port, and the
/// connections that it took.
async fn unavailable() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            counted.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut head = [0u8; 4096];
                let _ = conn.read(&mut head).await;
                let _ = conn.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n").await;
            });
        }
    });
    (port, count)
}

/// A dial of an echo through `road`, and its bytes back.
async fn carries(road: &dyn Dial, port: u16) {
    let deadline = tokio::time::Instant::now() + LIMIT;
    let mut io = road.dial(&Target::new("127.0.0.1", port), deadline).await.unwrap_or_else(|e| panic!("{e}")).io;
    let mut back = [0u8; 5];
    let both = async {
        io.write_all(b"again").await.unwrap();
        io.read_exact(&mut back).await.unwrap();
    };
    tokio::time::timeout(LIMIT, both).await.expect("the bytes within the limit");
    assert_eq!(&back, b"again");
}

/// Until `ready` holds, within the limit.
async fn until(what: &str, ready: impl Fn() -> bool) {
    let started = Instant::now();
    while !ready() {
        assert!(started.elapsed() < LIMIT, "{what}: not within {LIMIT:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Planted: the ended connection kept, and the dial after the end fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reopening_road_opens_a_new_connection_on_the_next_dial_after_the_end() {
    pod_testkit::need_loopback!();
    let stand = Stand::start(confirming(), 16).await.unwrap();
    let target = echo().await;
    let road = Reopening::connect(url(stand.port), how(None), AtEnd::Now).await.unwrap();
    assert!(road.is_open() && road.opened().is_some_and(|o| o.confirm), "{road:?}");
    tokio::join!(carries(&road, target), carries(&road, target));
    assert_eq!(stand.seen.lock().unwrap().protocols.len(), 1, "two dials, one connection");
    // A plain connection says its end too.
    let plain = Wisp::connect(&url(stand.port), &how(None)).await.unwrap();
    assert!(plain.is_open());
    stand.end_all().await;
    until("the end of both connections", || !road.is_open() && !plain.is_open()).await;
    assert!(road.opened().is_none());
    carries(&road, target).await;
    assert_eq!(stand.seen.lock().unwrap().protocols.len(), 3, "a new connection for the dial after the end");
    assert!(road.is_open());
}

/// Planted: no bound on the tries of a dial, and the server takes more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dial_opens_a_bounded_number_of_connections_and_the_next_dial_waits_for_the_backoff() {
    pod_testkit::need_loopback!();
    let (port, taken) = unavailable().await;
    let road = Reopening::new(url(port), how(None), AtEnd::Now).with_tries(2);
    let aim = Target::new("127.0.0.1", 22);
    let started = Instant::now();
    let failure = road.dial(&aim, tokio::time::Instant::now() + LIMIT).await.expect_err("HTTP 503");
    assert_eq!(taken.load(Ordering::SeqCst), 2, "two tries: {failure}");
    assert!(started.elapsed() >= Duration::from_millis(400), "a backoff between the tries");
    assert_eq!((failure.stage, failure.class), (Stage::WebSocket, Class::TempFail), "{failure}");
    // In the backoff after a failed dial, a dial fails at once.
    let again = road.dial(&aim, tokio::time::Instant::now() + LIMIT).await.expect_err("in the backoff");
    assert!(again.text().contains("no new connection to the Wisp server within"), "{again}");
    assert_eq!(taken.load(Ordering::SeqCst), 2, "no connection in the backoff");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_credential_is_not_tried_again() {
    pod_testkit::need_loopback!();
    let speaks = Speaks::Password { user: "ann".into(), password: PASSWORD.into(), required: true };
    let stand = Stand::start(speaks, 16).await.unwrap();
    let wrong = Login::new("ann", "not-the-made-up-pw").unwrap();
    let road = Reopening::new(url(stand.port), how(Some(wrong)), AtEnd::Now);
    let failure = road.dial(&Target::new("127.0.0.1", 22), tokio::time::Instant::now() + LIMIT).await.unwrap_err();
    assert_eq!((failure.stage, failure.class), (Stage::Relay, Class::Refused), "{failure}");
    assert!(failure.text().contains("0xc0, a wrong user name or password"), "{failure}");
    assert!(!failure.text().contains("not-the-made-up-pw"), "{failure}");
    assert_eq!(stand.seen.lock().unwrap().logins.len(), 1, "one try");
}

/// Planted: the login not sent, and the server refuses the connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_asks_for_a_password_takes_the_login_and_refuses_a_wrong_one() {
    pod_testkit::need_loopback!();
    let speaks = Speaks::Password { user: "ann".into(), password: PASSWORD.into(), required: true };
    let stand = Stand::start(speaks, 16).await.unwrap();
    // No login: this side refuses, with a CLOSE 0x04 on stream 0.
    let none = Wisp::connect(&url(stand.port), &how(None)).await.unwrap_err();
    assert!(none.to_string().contains("requires a user name and a password, and none was given"), "{none}");
    until("the client's CLOSE", || !stand.seen.lock().unwrap().closes.is_empty()).await;
    assert_eq!(stand.seen.lock().unwrap().closes, [(0, 0x04)]);
    // The right login opens the connection, and a stream carries.
    let login = Login::parse(&format!("ann:{PASSWORD}\n")).unwrap();
    let wisp = Wisp::connect(&url(stand.port), &how(Some(login))).await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(wisp.opened().version, (2, 1));
    let road = pod_egress::wisp::Road { wisp, at_end: AtEnd::Now, shown: "ws://127.0.0.1/".into() };
    carries(&road, echo().await).await;
    // A wrong one: the server's CLOSE 0xc0, said with no password.
    let wrong = Login::new("ann", "not-the-made-up-pw").unwrap();
    let e = Wisp::connect(&url(stand.port), &how(Some(wrong))).await.unwrap_err();
    assert_eq!(e, WispError::Refused(Reason::WRONG_PASSWORD));
    assert_eq!(e.to_string(), "the Wisp server refused the connection: 0xc0, a wrong user name or password");
    let seen = stand.seen.lock().unwrap();
    assert_eq!(seen.logins, [Some("ann".to_string()), Some("ann".to_string())]);
    let mut extension = vec![0x02, 19, 0, 0, 0, 3];
    extension.extend_from_slice(format!("ann{PASSWORD}").as_bytes());
    assert!(seen.client_infos[0].windows(extension.len()).any(|w| w == extension), "the extension's bytes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_optional_password_is_not_needed_and_a_login_goes_over_wss_alone() {
    pod_testkit::need_loopback!();
    let speaks = Speaks::Password { user: "ann".into(), password: PASSWORD.into(), required: false };
    let stand = Stand::start(speaks, 16).await.unwrap();
    Wisp::connect(&url(stand.port), &how(None)).await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(stand.seen.lock().unwrap().logins, [None]);
    // Over ws:// to another host, the login does not go: no dial.
    let clear = WsUrl::parse("ws://192.0.2.1/").unwrap();
    let login = Login::new("ann", PASSWORD).unwrap();
    let e = Wisp::connect(&clear, &how(Some(login))).await.unwrap_err();
    assert!(e.to_string().contains("goes over wss:// alone"), "{e}");
    assert_eq!(e.stage_and_class().1, Class::Config);
}
