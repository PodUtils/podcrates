//! The accept loop of `pod_ws::serve` (T-439, GitHub #116): 1,000 upgrades
//! reach the caller with at most 64 before `ready`; a client that stalls in
//! TLS, in the head or before its first frame is cut at the deadline of
//! that stage; a session that the caller does not admit is cut at the
//! deadline of `ready` or at the drop of its gate; the socket options; a
//! refused key; a refused upgrade; the one subprotocol of a server
//! (T-506). The caps are in `tests/serve_caps.rs`.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::serve::{LIMIT, cut, id, long, next, session_from, start, tcp_from, tls_over, upgrade};
use pod_core::Class;
use pod_ws::serve::Serve;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

/// A stage's deadline far below the others: a cut in time is that stage's.
const SHORT: Duration = Duration::from_millis(300);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_thousand_upgrades_reach_the_caller_with_at_most_64_before_ready() {
    pod_testkit::need_loopback!();
    let mut cfg = Serve::default();
    cfg.caps.max_unauth = 64;
    cfg.caps.per_prefix = 64;
    // A loaded host of CI shakes hands slowly: no deadline of this test cuts.
    cfg.deadlines = long().deadlines;
    let (addr, mut rx, _loop) = start(cfg).await;
    let served = Arc::new(AtomicUsize::new(0));
    let count = served.clone();
    let caller = tokio::spawn(async move {
        while let Some(accepted) = rx.recv().await {
            assert_eq!(accepted.key, Some(id(2).public()), "the key that the peer proved");
            assert!(accepted.gate.ready(), "a session in time holds its slot");
            count.fetch_add(1, Ordering::SeqCst);
            let session = accepted.session;
            tokio::spawn(async move {
                assert_eq!(session.read_frame().await.unwrap().payload, b"hello");
                session.send_binary(b"ready").await.unwrap();
                let _ = session.read_frame().await;
            });
        }
    });
    // At most 64 clients at one time, and each holds its place until the
    // caller admitted it: the loop holds at most 64 before ready.
    let places = Arc::new(tokio::sync::Semaphore::new(64));
    let mut clients = tokio::task::JoinSet::new();
    for _ in 0..1000 {
        let place = places.clone().acquire_owned().await.unwrap();
        clients.spawn(async move {
            let session = session_from(None, addr).await;
            assert_eq!(session.read_frame().await.unwrap().payload, b"ready");
            drop(place);
        });
    }
    let all = async {
        while let Some(done) = clients.join_next().await {
            done.expect("each client upgrades and is admitted");
        }
    };
    timeout(Duration::from_secs(300), all).await.expect("1,000 upgrades in time");
    assert_eq!(served.load(Ordering::SeqCst), 1000, "each session reached the caller");
    caller.abort();
}

#[tokio::test]
async fn a_client_that_stalls_in_tls_is_cut_at_the_deadline_of_tls() {
    pod_testkit::need_loopback!();
    let mut cfg = long();
    cfg.deadlines.tls = SHORT;
    let (addr, mut rx, _loop) = start(cfg).await;
    let mut silent = tcp_from(None, addr).await.unwrap();
    assert!(cut(&mut silent).await, "a client silent in TLS is cut at the deadline of TLS");
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn a_client_that_stalls_in_the_head_is_cut_at_the_deadline_of_the_head() {
    pod_testkit::need_loopback!();
    let mut cfg = long();
    cfg.deadlines.head = SHORT;
    let (addr, _rx, _loop) = start(cfg).await;
    let mut tls = tls_over(tcp_from(None, addr).await.unwrap(), 2).await.unwrap();
    tls.write_all(b"GET /serve HTTP/1.1\r\nHost: loop.test\r\n").await.unwrap();
    assert!(cut(&mut tls).await, "a head that stalls is cut at the deadline of the head");
}

#[tokio::test]
async fn a_client_that_stalls_before_its_first_frame_is_cut_at_that_deadline() {
    pod_testkit::need_loopback!();
    let mut cfg = long();
    cfg.deadlines.first = SHORT;
    let (addr, mut rx, _loop) = start(cfg).await;
    let mut tls = tls_over(tcp_from(None, addr).await.unwrap(), 2).await.unwrap();
    let pending = upgrade(&mut tls).await;
    assert!(pending.is_empty());
    assert!(cut(&mut tls).await, "a client with no first frame is cut at the deadline of the first frame");
    assert!(rx.try_recv().is_err(), "a session with no first frame does not reach the caller");
}

#[tokio::test]
async fn a_session_that_the_caller_does_not_admit_is_cut_at_the_deadline_of_ready() {
    pod_testkit::need_loopback!();
    let mut cfg = long();
    cfg.deadlines.ready = Duration::from_secs(1);
    let (addr, mut rx, _loop) = start(cfg).await;
    let held = session_from(None, addr).await;
    let accepted = next(&mut rx).await;
    let admitted = session_from(None, addr).await;
    let ready = next(&mut rx).await;
    assert!(ready.gate.ready());
    let cut = timeout(LIMIT, held.read_frame()).await.expect("the deadline of ready cuts");
    assert!(cut.is_err(), "a session with no ready is cut: {cut:?}");
    assert!(!accepted.gate.ready(), "a ready after the deadline finds the session cut");
    // The admitted session lives past the deadline of ready.
    tokio::time::sleep(Duration::from_millis(500)).await;
    ready.session.send_binary(b"after the deadline").await.unwrap();
    let got = timeout(LIMIT, admitted.read_frame()).await.unwrap().unwrap();
    assert_eq!(got.payload, b"after the deadline");
    assert_eq!(timeout(LIMIT, ready.session.read_frame()).await.unwrap().unwrap().payload, b"hello");
}

#[tokio::test]
async fn a_gate_dropped_with_no_ready_cuts_its_session() {
    pod_testkit::need_loopback!();
    let (addr, mut rx, _loop) = start(long()).await;
    let client = session_from(None, addr).await;
    let accepted = next(&mut rx).await;
    let session = accepted.session;
    drop(accepted.gate);
    let cut = timeout(LIMIT, client.read_frame()).await.expect("the drop cuts");
    assert!(cut.is_err(), "a session whose gate dropped is cut: {cut:?}");
    drop(session);
}

#[tokio::test]
async fn each_accepted_socket_has_nodelay_and_its_low_water_mark() {
    pod_testkit::need_loopback!();
    let mut cfg = long();
    cfg.lowat = Some(12345);
    let (addr, mut rx, _loop) = start(cfg).await;
    let _client = session_from(None, addr).await;
    let accepted = next(&mut rx).await;
    assert!(accepted.options.nodelay, "TCP_NODELAY on the accepted socket");
    if cfg!(any(target_os = "linux", target_os = "android")) {
        assert_eq!(accepted.options.notsent_lowat, Some(12345), "TCP_NOTSENT_LOWAT on the accepted socket");
    }
    assert_eq!(accepted.request.target, "/serve");
    assert!(format!("{accepted:?}").contains("/serve"));
}

#[tokio::test]
async fn a_client_of_another_key_gets_no_session() {
    pod_testkit::need_loopback!();
    let (addr, mut rx, _loop) = start(long()).await;
    // In TLS 1.3 the client ends its handshake before the server checks its
    // key: the refusal comes at its first read.
    let mut tls = tls_over(tcp_from(None, addr).await.unwrap(), 3).await.unwrap();
    let _ = tls.write_all(b"GET /serve HTTP/1.1\r\nHost: loop.test\r\n\r\n").await;
    let mut byte = [0u8; 1];
    let read = timeout(LIMIT, tls.read(&mut byte)).await.expect("the refusal in time");
    read.expect_err("the server refuses a key that it does not pin");
    assert!(rx.try_recv().is_err());
    // The loop goes on.
    let _good = session_from(None, addr).await;
    assert_eq!(next(&mut rx).await.key, Some(id(2).public()));
}

#[tokio::test]
async fn a_refused_upgrade_gets_its_status_and_no_session() {
    pod_testkit::need_loopback!();
    let mut cfg = long();
    cfg.caps.head_bytes = 256;
    cfg.caps.head_lines = 4;
    let (addr, mut rx, _loop) = start(cfg).await;
    let ok = "GET /serve HTTP/1.1\r\nHost: loop.test\r\n";
    for (head, status) in [
        (format!("{ok}\r\n"), "426"),
        (format!("{ok}X-A: 1\r\nX-B: 2\r\nX-C: 3\r\nX-D: 4\r\n\r\n"), "431"),
        (format!("{ok}X-Long: {}\r\n\r\n", "a".repeat(300)), "431"),
        ("POST /serve HTTP/1.1\r\nHost: loop.test\r\n\r\n".to_string(), "405"),
    ] {
        let mut tls = tls_over(tcp_from(None, addr).await.unwrap(), 2).await.unwrap();
        tls.write_all(head.as_bytes()).await.unwrap();
        let mut answer = Vec::new();
        let _ = timeout(LIMIT, tls.read_to_end(&mut answer)).await.expect("the answer in time");
        let answer = String::from_utf8_lossy(&answer);
        assert!(answer.starts_with(&format!("HTTP/1.1 {status} ")), "{head:?}: {answer}");
    }
    assert!(rx.try_recv().is_err());
}

/// The head of the answer to an upgrade that offers `protocol`, read up to
/// its empty line; after a 101, the session that sent its first frame.
async fn answer_to(addr: std::net::SocketAddr, protocol: Option<&str>) -> (String, Option<Session>) {
    let mut tls = tls_over(tcp_from(None, addr).await.unwrap(), 2).await.unwrap();
    let key = pod_ws::handshake::generate_key().unwrap();
    let url = pod_ws::url::WsUrl::parse("wss://loop.test/serve").unwrap();
    tls.write_all(&pod_ws::url::offering(&url, &key, "pod-ws/test", protocol).unwrap()).await.unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match timeout(LIMIT, tls.read(&mut byte)).await.expect("the answer in time") {
            Ok(1) => head.push(byte[0]),
            _ => break,
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    if !head.starts_with("HTTP/1.1 101 ") {
        return (head, None);
    }
    assert!(pod_ws::handshake::check_response(&head, &key).is_ok(), "{head}");
    let session = pod_ws::RelaySession::new(tls, Vec::new(), None, LIMIT);
    session.send_binary(b"hello").await.unwrap();
    (head, Some(session))
}

type Session = pod_ws::RelaySession<common::serve::ClientTls>;

#[tokio::test]
async fn a_server_of_one_subprotocol_refuses_a_request_that_does_not_offer_it() {
    pod_testkit::need_loopback!();
    let mut cfg = long();
    cfg.protocol = Some("podnet/1");
    let (addr, mut rx, _loop) = start(cfg).await;
    for offered in [None, Some("chat"), Some("PODNET/1")] {
        let (head, _) = answer_to(addr, offered).await;
        assert!(head.starts_with("HTTP/1.1 426 "), "{offered:?}: {head}");
        assert!(head.contains("\r\nSec-WebSocket-Protocol: podnet/1\r\n"), "the refusal names it: {head}");
    }
    assert!(rx.try_recv().is_err(), "a refused request reaches no caller");
    let (head, _session) = answer_to(addr, Some("chat, podnet/1")).await;
    assert!(head.starts_with("HTTP/1.1 101 "), "{head}");
    assert!(pod_ws::handshake::check_chosen_protocol(&head, Some("podnet/1")).is_ok(), "the 101 names it: {head}");
    assert_eq!(next(&mut rx).await.key, Some(id(2).public()));

    let (addr, _rx, _loop) = start(long()).await;
    let (head, _session) = answer_to(addr, Some("podnet/1")).await;
    assert!(
        head.starts_with("HTTP/1.1 101 ") && pod_ws::handshake::check_chosen_protocol(&head, None).is_ok(),
        "{head}"
    );
}

#[tokio::test]
async fn a_setting_out_of_its_limits_is_a_usage_error() {
    let listener = || async { tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap() };
    let tls = common::serve::server_tls(&[]);
    let mut cfg = Serve::default();
    assert!(cfg.check().is_ok());
    assert_eq!(cfg.reserve(), 64 << 10, "the default reserve fills 64 MiB with 1,024 connections");
    cfg.caps.preauth_bytes = cfg.reserve() - 1;
    assert!(cfg.check().unwrap_err().contains("preauth_bytes"));
    for bad in [
        Serve {
            deadlines: pod_ws::serve::Deadlines { first: Duration::ZERO, ..Default::default() },
            ..Default::default()
        },
        Serve { caps: pod_ws::serve::Caps { per_prefix: 0, ..Default::default() }, ..Default::default() },
        Serve { caps: pod_ws::serve::Caps { head_bytes: 10, ..Default::default() }, ..Default::default() },
        Serve { protocol: Some("two words"), ..Default::default() },
        Serve { protocol: Some(""), ..Default::default() },
    ] {
        assert!(bad.check().is_err(), "{bad:?}");
    }
    if pod_testkit::host::loopback().is_ok() {
        let e = pod_ws::serve::serve(vec![listener().await], tls.clone(), cfg, |_| {}).await.unwrap_err();
        assert_eq!(e.class, Class::Usage);
    }
    let e = pod_ws::serve::serve(Vec::new(), tls, Serve::default(), |_| {}).await.unwrap_err();
    assert_eq!(e.class, Class::Usage, "{e}");
}
