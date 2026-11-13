use std::sync::Mutex as StdMutex;

use super::*;

fn fails(text: &str) -> Failure {
    Failure::new(Stage::Warp, Class::Unavailable, text)
}

struct Fake {
    id: u32,
    closed: Arc<AtomicBool>,
}

impl Closable for Fake {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

/// A session that closes is replaced; a caller during the gap waits for
/// the new one; failed opens are tried again.
#[tokio::test]
async fn a_closed_session_is_replaced() {
    let first_closed = Arc::new(AtomicBool::new(false));
    let opens = Arc::new(AtomicU32::new(0));
    let failures_left = Arc::new(AtomicU32::new(2));
    let connect: Connect<Fake> = {
        let opens = opens.clone();
        let failures_left = failures_left.clone();
        Box::new(move || {
            let opens = opens.clone();
            let failures_left = failures_left.clone();
            Box::pin(async move {
                // No fetch_update: a later Rust deprecates it
                // (PodUtils/podnet issue 214).
                let mut left = failures_left.load(Ordering::SeqCst);
                let took = loop {
                    if left == 0 {
                        break false;
                    }
                    match failures_left.compare_exchange_weak(left, left - 1, Ordering::SeqCst, Ordering::SeqCst) {
                        Ok(_) => break true,
                        Err(now) => left = now,
                    }
                };
                if took {
                    return Err(fails("the front door does not answer"));
                }
                let id = opens.fetch_add(1, Ordering::SeqCst) + 2;
                Ok(Fake { id, closed: Arc::new(AtomicBool::new(false)) })
            })
        })
    };
    let first = Fake { id: 1, closed: first_closed.clone() };
    let live = Live::start_with(first, connect, Duration::from_millis(50), Duration::from_millis(200));
    assert_eq!(live.get().await.unwrap().id, 1);
    first_closed.store(true, Ordering::SeqCst);
    let next = live.get_within(Duration::from_secs(10)).await.unwrap();
    assert_eq!(next.id, 2, "two failed opens, then a new session");
    assert_eq!(live.reconnects(), 1);
    assert_eq!(opens.load(Ordering::SeqCst), 1);
}

struct Budgeted {
    id: u32,
    closed: AtomicBool,
    due: Arc<AtomicBool>,
}

impl Closable for Budgeted {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    fn close(&self, _why: &str) {
        self.closed.store(true, Ordering::SeqCst);
    }
    fn renew_due(&self) -> bool {
        self.due.load(Ordering::SeqCst)
    }
}

/// The owner ruling of podnet (2026-11-04): at the relay budget, a new
/// session takes the new flows with no gap; the old one closes after the
/// drain.
#[tokio::test(start_paused = true)]
async fn a_spent_relay_budget_renews_with_no_gap() {
    let opens = Arc::new(AtomicU32::new(1));
    let connect: Connect<Budgeted> = {
        let opens = opens.clone();
        Box::new(move || {
            let opens = opens.clone();
            Box::pin(async move {
                let id = opens.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(Budgeted { id, closed: AtomicBool::new(false), due: Arc::new(AtomicBool::new(false)) })
            })
        })
    };
    let due = Arc::new(AtomicBool::new(false));
    let first = Budgeted { id: 1, closed: AtomicBool::new(false), due: due.clone() };
    let live = Live::start_with(first, connect, Duration::from_millis(50), Duration::from_millis(200));
    let old = live.get().await.unwrap();
    assert_eq!(old.id, 1);
    due.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(2)).await;
    // A new flow gets the new session; the old one is still open.
    let next = live.get_within(Duration::ZERO).await.unwrap();
    assert_eq!(next.id, 2);
    assert!(!old.is_closed(), "the open flows go on in the old session");
    assert_eq!(live.reconnects(), 1);
    tokio::time::sleep(RENEW_DRAIN + Duration::from_secs(1)).await;
    assert!(old.is_closed(), "the old session closes after the drain");
    assert_eq!(live.get_within(Duration::ZERO).await.unwrap().id, 2);
    assert_eq!(opens.load(Ordering::SeqCst), 2, "one new session only");
}

struct Aged {
    id: u32,
    closed: AtomicBool,
}

impl Closable for Aged {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    fn reason(&self) -> Option<String> {
        self.is_closed().then(|| "max age".into())
    }
    fn close(&self, _why: &str) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

/// PodUtils/podnet issue 185: a session is replaced at its max age, with no
/// backoff.
#[tokio::test]
async fn a_session_is_replaced_at_max_age() {
    let opens = Arc::new(AtomicU32::new(1));
    let connect: Connect<Aged> = {
        let opens = opens.clone();
        Box::new(move || {
            let opens = opens.clone();
            Box::pin(async move {
                let id = opens.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(Aged { id, closed: AtomicBool::new(false) })
            })
        })
    };
    let first = Aged { id: 1, closed: AtomicBool::new(false) };
    let (base, age) = (Duration::from_secs(30), Some(Duration::from_millis(200)));
    let live = Live::start_aged(first, connect, base, base, age);
    assert_eq!(live.get().await.unwrap().id, 1);
    let mut changes = live.generation.subscribe();
    tokio::time::timeout(Duration::from_secs(5), changes.changed()).await.expect("no renewal").unwrap();
    assert!(live.reconnects() >= 1);
    assert!(live.get().await.unwrap().id >= 2);
}

/// A caller does not wait for ever while no session opens.
#[tokio::test]
async fn a_caller_gets_an_error_when_no_session_opens() {
    let connect: Connect<Fake> = Box::new(|| Box::pin(async { Err(fails("no route to the front door")) }));
    let first = Fake { id: 1, closed: Arc::new(AtomicBool::new(true)) };
    let live = Live::start_with(first, connect, Duration::from_millis(20), Duration::from_millis(50));
    let err = live.get_within(Duration::from_millis(400)).await.err().unwrap();
    assert!(err.text().contains("no new session"), "{}", err.chain());
    assert_eq!(err.class, Class::Unavailable);
}

/// A session that carries no TCP: its probe fails.
struct Silenced {
    id: u32,
    closed: AtomicBool,
    tcp: bool,
    probes: Arc<AtomicU32>,
}

impl Closable for Silenced {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    fn close(&self, _why: &str) {
        self.closed.store(true, Ordering::SeqCst);
    }
    fn probe_tcp(&self, _target: SocketAddr, _limit: Duration) -> ProbeFuture<'_> {
        self.probes.fetch_add(1, Ordering::SeqCst);
        let tcp = self.tcp;
        Box::pin(async move { tcp })
    }
}

/// PodUtils/podnet@T-068: a session that carries ICMP and UDP but no TCP
/// stays open. After three failed probes, a new session takes the new
/// flows, and the old one drains.
#[tokio::test(start_paused = true)]
async fn warp_probe_renews_a_session_with_no_tcp() {
    let probes = Arc::new(AtomicU32::new(0));
    let opens = Arc::new(AtomicU32::new(1));
    let connect: Connect<Silenced> = {
        let (opens, probes) = (opens.clone(), probes.clone());
        Box::new(move || {
            let (opens, probes) = (opens.clone(), probes.clone());
            Box::pin(async move {
                let id = opens.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(Silenced { id, closed: AtomicBool::new(false), tcp: true, probes })
            })
        })
    };
    let first = Silenced { id: 1, closed: AtomicBool::new(false), tcp: false, probes: probes.clone() };
    let live = Live::start_with(first, connect, Duration::from_millis(50), Duration::from_millis(200))
        .probed(Some(DEFAULT_PROBE));
    let old = live.get().await.unwrap();
    // Two misses: the session stays.
    tokio::time::sleep(DEFAULT_PROBE.every * 2 + Duration::from_secs(1)).await;
    assert_eq!(live.get_within(Duration::ZERO).await.unwrap().id, 1);
    assert_eq!(live.data_path(), "ok");
    // The third miss: a new session takes the new flows.
    tokio::time::sleep(DEFAULT_PROBE.every + Duration::from_secs(1)).await;
    assert_eq!(live.get_within(Duration::ZERO).await.unwrap().id, 2);
    assert_eq!(live.reconnects(), 1);
    assert_eq!(live.data_path(), "ok");
    assert!(!old.is_closed(), "the open flows go on in the old session");
    tokio::time::sleep(RENEW_DRAIN + Duration::from_secs(1)).await;
    assert!(old.is_closed(), "the old session closes after the drain");
    // The new session passes its probes: no other renewal.
    tokio::time::sleep(DEFAULT_PROBE.every * 4).await;
    assert_eq!(live.reconnects(), 1);
    assert!(probes.load(Ordering::SeqCst) >= 6);
}

#[test]
fn the_probe_option() {
    assert_eq!(parse_probe("off").unwrap(), None);
    let p = parse_probe("9.9.9.9:53").unwrap().unwrap();
    assert_eq!(p.target, "9.9.9.9:53".parse().unwrap());
    assert_eq!(p.misses, 3);
    assert_eq!(parse_probe("one.one.one.one:443").unwrap_err().class, Class::Usage);
}

/// The target of the default probe is the trace of `pod-services`.
#[test]
fn the_default_probe_is_the_trace_of_the_services() {
    let trace = crate::exit::trace_address();
    assert_eq!(DEFAULT_PROBE.target, SocketAddr::new(trace, 443));
}

/// A session whose TCP probe fails when `tcp` is false.
struct Probed {
    tcp: bool,
    closed: AtomicBool,
}

impl Closable for Probed {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    fn close(&self, _why: &str) {
        self.closed.store(true, Ordering::SeqCst);
    }
    fn probe_tcp(&self, _target: SocketAddr, _limit: Duration) -> ProbeFuture<'_> {
        let tcp = self.tcp;
        Box::pin(async move { tcp })
    }
}

/// PodUtils/podnet@T-140: the reports of a `Live`, and so the caller's
/// status, name the data path: `tcp-dead` after the failed probes, `ok`
/// after the new session, with the time of the last probe.
#[tokio::test(start_paused = true)]
async fn warp_status_names_the_data_path() {
    // The new session opens only when the test allows it.
    let allow = Arc::new(AtomicBool::new(false));
    let connect: Connect<Probed> = {
        let allow = allow.clone();
        Box::new(move || {
            let allow = allow.clone();
            Box::pin(async move {
                if !allow.load(Ordering::SeqCst) {
                    return Err(fails("the front door does not answer"));
                }
                Ok(Probed { tcp: true, closed: AtomicBool::new(false) })
            })
        })
    };
    let first = Probed { tcp: false, closed: AtomicBool::new(false) };
    let live = Live::start_with(first, connect, Duration::from_millis(50), Duration::from_millis(200))
        .probed(Some(DEFAULT_PROBE));
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let sink = seen.clone();
    publish(&live, move |report| sink.lock().unwrap().push(report));
    let last = || *seen.lock().unwrap().last().expect("a report");
    let started = unix_now();
    // One failed probe: the data path is still ok.
    tokio::time::sleep(DEFAULT_PROBE.every + Duration::from_secs(1)).await;
    assert_eq!(last().data_path, "ok");
    assert!(last().last_probe.unwrap() >= started);
    // The third failed probe: tcp-dead, while no new session opens.
    tokio::time::sleep(DEFAULT_PROBE.every * 2).await;
    assert_eq!(last().data_path, "tcp-dead");
    // The new session: ok.
    allow.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(live.reconnects(), 1);
    assert_eq!(last().data_path, "ok");
    assert!(last().last_probe.is_some(), "the time of the last probe stays");
}

/// A session that is also a dial: its label is its id.
struct Dialed {
    id: u32,
    closed: Arc<AtomicBool>,
}

impl Closable for Dialed {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

impl Dial for Dialed {
    fn label(&self) -> String {
        format!("session {}", self.id)
    }
    fn dial<'a>(
        &'a self,
        _target: &'a pod_core::Target,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'a, Result<pod_core::Connected, Failure>> {
        Box::pin(async { Err(fails("no dial in this test")) })
    }
}

/// The proxy takes the session of now: after a close, the new session.
#[tokio::test]
async fn the_tunnel_of_a_live_session_is_the_session_of_now() {
    let closed = Arc::new(AtomicBool::new(false));
    let connect: Connect<Dialed> =
        Box::new(|| Box::pin(async { Ok(Dialed { id: 2, closed: Arc::new(AtomicBool::new(false)) }) }));
    let live = Live::start_with(Dialed { id: 1, closed: closed.clone() }, connect, Duration::ZERO, Duration::ZERO);
    let tunnel: Arc<dyn crate::proxy::Tunnel> = live.clone();
    assert_eq!(tunnel.session().await.unwrap().label(), "session 1");
    closed.store(true, Ordering::SeqCst);
    assert_eq!(tunnel.session().await.unwrap().label(), "session 2");
}

/// Each step of the watcher is an event of the caller.
#[tokio::test(start_paused = true)]
async fn the_watcher_tells_each_step_to_the_caller() {
    let kinds = Arc::new(StdMutex::new(Vec::new()));
    let keep = kinds.clone();
    let report = pod_core::Report {
        events: Arc::new(move |e: Event| keep.lock().unwrap().push(e.kind)),
        ..pod_core::Report::default()
    };
    let settings = LiveSettings { base: Duration::from_millis(50), probe: None, report, ..LiveSettings::default() };
    let closed = Arc::new(AtomicBool::new(false));
    let opens = Arc::new(AtomicU32::new(0));
    let connect: Connect<Fake> = {
        let opens = opens.clone();
        Box::new(move || {
            let n = opens.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                match n {
                    0 => Err(fails("the front door does not answer")),
                    _ => Ok(Fake { id: 2, closed: Arc::new(AtomicBool::new(false)) }),
                }
            })
        })
    };
    let live = Live::start(Fake { id: 1, closed: closed.clone() }, connect, &settings);
    closed.store(true, Ordering::SeqCst);
    assert_eq!(live.get().await.unwrap().id, 2);
    assert_eq!(*kinds.lock().unwrap(), ["warp.live.closed", "warp.live.failed", "warp.live.open"]);
}
