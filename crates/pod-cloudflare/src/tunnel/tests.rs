use super::*;
#[cfg(unix)]
use crate::ingress::Service;

fn refused(retry: bool, after: Duration) -> pod_core::Failure {
    pod_core::Failure::from(rpc::Refused {
        client: "podnet".into(),
        cause: "Unauthorized: Tunnel not found".into(),
        should_retry: retry,
        retry_after: after,
    })
}

// A refusal with retry: yes in the first registration gets new tries for
// the time budget (the edge can answer "Tunnel not found" for a new quick
// tunnel). A refusal with retry: no ends the leg at once.
#[tokio::test(start_paused = true)]
async fn a_retryable_refusal_of_the_first_registration_is_tried_again() {
    let second = Duration::from_secs(1);
    let mut calls = 0;
    let got = register_retrying(|| {
        calls += 1;
        let n = calls;
        async move { if n < 6 { Err(refused(true, second)) } else { Ok(n) } }
    })
    .await
    .unwrap();
    assert_eq!(got, 6);
    let mut calls = 0;
    let err = register_retrying(|| {
        calls += 1;
        async { Err::<(), _>(refused(false, second)) }
    })
    .await
    .unwrap_err();
    assert_eq!(calls, 1);
    assert!(err.chain().contains("retry: no"));
    // Retry: yes at each try: the tries stop at the end of the budget, and
    // the last refusal comes back with its class and its type.
    let started = tokio::time::Instant::now();
    let mut calls = 0;
    let err = register_retrying(|| {
        calls += 1;
        async { Err::<(), _>(refused(true, second)) }
    })
    .await
    .unwrap_err();
    assert_eq!(started.elapsed(), REGISTER_BUDGET);
    assert!(calls > 6, "{calls}");
    assert_eq!(err.class, Class::TempFail);
    assert!(crate::fail::find::<rpc::Refused>(&err).is_some());
}

// Each wait is the wait of the edge or the backoff, whichever is longer,
// from 1 s to 5 s.
#[test]
fn the_wait_of_a_retry_is_the_longer_of_the_edge_and_the_backoff() {
    for retry in 1..10 {
        let wait = register_wait(Duration::ZERO, retry);
        assert!((REGISTER_WAIT_MIN..=REGISTER_WAIT_MAX).contains(&wait), "{retry}: {wait:?}");
        assert!(register_wait(Duration::from_secs(4), retry) >= Duration::from_secs(4));
        assert_eq!(register_wait(Duration::from_secs(60), retry), REGISTER_WAIT_MAX);
    }
    // The backoff of the sixth retry is 16 s to 48 s: the wait is 5 s.
    assert_eq!(register_wait(Duration::ZERO, 6), REGISTER_WAIT_MAX);
}

// The limit of a serial leg attempt is at least the sum of the budgets of
// the steps of the leg: the first try, the time budget of the retries and
// the last try.
#[test]
fn the_serial_limit_covers_the_budgets_of_each_leg() {
    let quic = (quic_dial::HANDSHAKE_TIMEOUT + REGISTER_TIMEOUT) * 2 + REGISTER_BUDGET;
    let h2 = (crate::edge_h2::DIAL_TIMEOUT * 3 + crate::edge_h2::REGISTER_TIMEOUT * 2) * 2 + REGISTER_BUDGET;
    let limit = |kind| attempt_limit(kind).expect("a limit");
    assert!(limit(LegKind::QuicDirect) >= quic, "quic");
    assert!(limit(LegKind::H2Direct) >= h2, "http2");
    assert!(limit(LegKind::H2Relay) >= h2, "http2+relay");
    assert_eq!(attempt_limit(LegKind::QuicWarp), None);
    // PodUtils/podnet:docs/tunnels.md and PodUtils/podnet:docs/protocol.md,
    // section 3.3, give these values (T-1117).
    assert_eq!(limit(LegKind::QuicDirect).as_secs(), 110);
    assert_eq!(limit(LegKind::H2Direct).as_secs(), 210);
}

// The worst registration completes in the serial limit: a first try that
// takes its full budget, refusals with retry: yes for the time budget, and
// a last try that starts at the end of the budget and takes its full
// budget.
#[tokio::test(start_paused = true)]
async fn a_slow_registration_completes_in_the_serial_limit() {
    let started = tokio::time::Instant::now();
    let last_start = started + QUIC_TRY_BUDGET + REGISTER_BUDGET;
    let mut calls = 0;
    let got = bounded(
        attempt_limit(LegKind::QuicDirect),
        register_retrying(|| {
            calls += 1;
            let n = calls;
            async move {
                let now = tokio::time::Instant::now();
                if n == 1 || now >= last_start {
                    tokio::time::sleep(QUIC_TRY_BUDGET).await;
                }
                if now >= last_start { Ok(n) } else { Err(refused(true, REGISTER_WAIT_MAX)) }
            }
        }),
    )
    .await
    .unwrap();
    assert!(got > 3, "{got}");
    assert_eq!(started.elapsed(), attempt_limit(LegKind::QuicDirect).unwrap());
}

// The class of "no edge leg worked": a temporary failure when each leg
// failed with a refusal with retry: yes, else the service is unavailable.
// The cause of the last leg stays in the chain.
#[test]
fn the_class_of_no_edge_leg_worked() {
    use LegKind::*;
    let second = Duration::from_secs(1);
    let timeout = || crate::fail::new(Stage::EdgeRegister, Class::Timeout, "the registration did not end in 30 s");
    let plan = [QuicDirect, H2Direct, QuicWarp];
    let cases: Vec<(Vec<pod_core::Failure>, Class, bool)> = vec![
        (vec![refused(true, second)], Class::TempFail, true),
        (vec![refused(true, second), refused(true, second), refused(true, second)], Class::TempFail, true),
        (vec![refused(true, second), refused(false, second)], Class::Unavailable, true),
        (vec![refused(false, second)], Class::Unavailable, true),
        (vec![refused(true, second), timeout()], Class::Unavailable, false),
        (vec![timeout()], Class::Unavailable, false),
    ];
    for (failures, class, typed) in cases {
        let n = failures.len();
        let last = failures.last().map(|f| f.stage);
        let err = no_leg_worked(&plan, failures.into_iter().enumerate().collect());
        assert_eq!(err.class, class, "{err}");
        assert_eq!(crate::fail::find::<rpc::Refused>(&err).is_some(), typed, "{err}");
        assert!(err.to_string().starts_with("no edge leg worked: quic: "), "{err}");
        assert_eq!(err.to_string().matches("quic: ").count() + err.to_string().matches("http2: ").count(), n.min(2));
        assert_eq!(Some(err.stage), last);
        assert_eq!(err.chain(), err.to_string());
    }
}

#[test]
fn podnet_udp_and_the_probe_shape_the_plan() {
    use crate::udp::{Choice, Egress};
    use LegKind::*;
    let words = crate::testing::podnet_words();
    let all = || vec![QuicDirect, H2Direct, QuicWarp];
    let plan = |udp, egress: Option<Egress>| plan_with_udp(all(), udp, egress.as_ref(), &words);
    assert_eq!(plan(Choice::Off, None).unwrap().0, vec![H2Direct, QuicWarp]);
    assert_eq!(plan(Choice::On, None).unwrap().0, all());
    let refused = Egress::Refused("EPERM".into());
    let (kept, note) = plan(Choice::Probe, Some(refused.clone())).unwrap();
    assert_eq!(kept, vec![H2Direct, QuicWarp]);
    assert!(note.unwrap().contains("refused"));
    // No answer, or an answer, keeps the order.
    for egress in [Egress::Unknown("x".into()), Egress::Answered("x".into())] {
        assert_eq!(plan(Choice::Probe, Some(egress)).unwrap().0, all());
    }
    // PODNET_UDP=off with QUIC on UDP 7844 as the only leg is an error.
    let err = plan_with_udp(vec![QuicDirect], Choice::Off, None, &words).unwrap_err();
    assert_eq!(crate::fail::class_of(&err), Some(Class::Config));
    assert_eq!(plan_with_udp(vec![H2Direct], Choice::Off, None, &words).unwrap().0, vec![H2Direct]);
    let _ = refused;
}

// With a proxy, auto drops QUIC on UDP 7844; an explicit
// quic keeps it, with a note.
#[test]
fn a_proxy_and_the_quic_leg() {
    let words = crate::testing::podnet_words();
    let proxy = pod_proxy::Proxy::parse_trusted("http://127.0.0.1:3128").unwrap().label();
    let auto = leg_plan(Protocol::Auto, Transport::Auto, &words).unwrap();
    let (plan, note) = plan_with_proxy(auto, Protocol::Auto, Some(&proxy), &words);
    assert!(!plan.contains(&LegKind::QuicDirect));
    assert!(note.unwrap().contains("does not try QUIC"));
    let quic = leg_plan(Protocol::Quic, Transport::Direct, &words).unwrap();
    let (plan, note) = plan_with_proxy(quic, Protocol::Quic, Some(&proxy), &words);
    assert_eq!(plan, vec![LegKind::QuicDirect]);
    assert!(note.unwrap().contains("goes around the proxy"));
    let quic = leg_plan(Protocol::Quic, Transport::Direct, &words).unwrap();
    assert_eq!(plan_with_proxy(quic, Protocol::Quic, None, &words).1, None);
}

#[test]
fn the_leg_plan_tries_quic_then_http2_then_warp() {
    use LegKind::*;
    let words = crate::testing::podnet_words();
    let plan = |p, t| leg_plan(p, t, &words).unwrap();
    assert_eq!(plan(Protocol::Auto, Transport::Auto), vec![QuicDirect, H2Direct, QuicWarp]);
    assert_eq!(plan(Protocol::Quic, Transport::Auto), vec![QuicDirect, QuicWarp]);
    assert_eq!(plan(Protocol::Http2, Transport::Auto), vec![H2Direct]);
    assert_eq!(plan(Protocol::Auto, Transport::Direct), vec![QuicDirect, H2Direct]);
    assert_eq!(plan(Protocol::Quic, Transport::Warp), vec![QuicWarp]);
    assert!(leg_plan(Protocol::Http2, Transport::Warp, &words).is_err());
    assert_eq!(plan(Protocol::Auto, Transport::Relay), vec![H2Relay]);
    assert_eq!(plan(Protocol::Http2, Transport::Relay), vec![H2Relay]);
    assert!(leg_plan(Protocol::Quic, Transport::Relay, &words).is_err());
}

/// The relay of the host goes to the leg http2+relay alone, as podnet's
/// `--transport relay`; that leg with no relay is an error of the
/// configuration.
#[test]
fn only_the_relay_leg_takes_the_relay() {
    let mut host = crate::testing::host();
    assert_eq!(relay_for(LegKind::H2Relay, &host).unwrap_err().class, Class::Config);
    host.relay = Some(crate::testing::relay(crate::testing::dial()));
    assert!(relay_for(LegKind::H2Relay, &host).unwrap().is_some());
    for kind in [LegKind::QuicDirect, LegKind::H2Direct, LegKind::QuicWarp] {
        assert!(relay_for(kind, &host).unwrap().is_none(), "{kind}");
    }
}

/// PodUtils/podnet#685: the edge refuses a new quick tunnel with
/// "Tunnel not found" and retry: yes more than three times, then accepts.
/// The leg tries again in its time budget, with the same quick tunnel.
#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn a_quick_tunnel_that_the_edge_does_not_know_yet_starts_with_one_provision() {
    pod_testkit::need_loopback!();
    let standin = crate::standin::Standin::start_here().await;
    let not_found = || crate::standin::Reply::RefuseRetry {
        cause: "Unauthorized: Tunnel not found".into(),
        after: Duration::from_secs(1),
    };
    standin.edge.script((0..5).map(|_| not_found()));
    let (host, _) = standin.host(true, None);
    let start = standin.start_tunnel(host, standin.config(Service::HelloWorld));
    let handle = on_a_stepped_clock(start).await.expect("the tunnel starts");
    assert_eq!(handle.leg(), "http2");
    assert_eq!(standin.api.issued.lock().unwrap().len(), 1, "one provision");
    let registrations = standin.edge.registrations.lock().unwrap().clone();
    let first: Vec<bool> = registrations.iter().filter(|r| r.conn_index == 0).map(|r| r.accepted).collect();
    assert_eq!(first, [false, false, false, false, false, true], "{registrations:?}");
    handle.shutdown().await;
}

/// A refusal with retry: yes at each try ends the start after the time
/// budget, as a temporary failure (exit 75) with the typed refusal in the
/// chain.
#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn a_refusal_with_retry_at_each_try_fails_after_the_budget_as_tempfail() {
    pod_testkit::need_loopback!();
    let standin = crate::standin::Standin::start_here().await;
    standin.edge.answer(crate::standin::Reply::RefuseRetry {
        cause: "Unauthorized: Tunnel not found".into(),
        after: Duration::from_secs(1),
    });
    let (host, _) = standin.host(true, None);
    let started = tokio::time::Instant::now();
    let start = standin.start_tunnel(host, standin.config(Service::HelloWorld));
    let err = on_a_stepped_clock(start).await.err().expect("the start fails");
    assert!(started.elapsed() >= Duration::from_secs(30), "{:?}", started.elapsed());
    assert_eq!((err.class, err.class.exit_code()), (Class::TempFail, 75), "{err:#}");
    let refused = crate::fail::find::<rpc::Refused>(&err).expect("the typed refusal");
    assert!(refused.should_retry);
    assert!(err.to_string().starts_with("no edge leg worked: http2: "), "{err}");
    assert_eq!(standin.api.issued.lock().unwrap().len(), 1, "one provision");
    assert!(standin.edge.registrations.lock().unwrap().len() > 3);
}

/// Run `future` on the paused clock, with steps of 50 ms. A blocking task
/// stops the auto-advance of the clock: with it, a wait for the loopback
/// fires the timeouts of the handshakes at once. Between two steps, the
/// thread sleeps 1 ms of real time, so the loopback can deliver.
#[cfg(unix)]
async fn on_a_stepped_clock<F: std::future::Future>(future: F) -> F::Output {
    let (release, held) = std::sync::mpsc::channel::<()>();
    let hold = tokio::task::spawn_blocking(move || held.recv());
    tokio::pin!(future);
    loop {
        tokio::select! {
            biased;
            out = &mut future => {
                drop(release);
                let _ = hold.await;
                return out;
            }
            () = tokio::task::yield_now() => {}
        }
        std::thread::sleep(Duration::from_millis(1));
        tokio::time::advance(Duration::from_millis(50)).await;
    }
}
