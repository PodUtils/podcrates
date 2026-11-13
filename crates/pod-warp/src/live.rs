//! A WARP session that comes back (PodUtils/podnet issue 75).
//!
//! A proxy, a forward or a TUN that holds one session fails each new
//! connection after the session ends (a change of the network, the front
//! door, the limit of a relay). [`Live`] holds the current session, and a
//! task opens a new one when it closes: the first try at once, then a
//! backoff from [`LiveSettings::base`], doubled to [`LiveSettings::max`]. A
//! caller that asks for the session while it is down waits for the new one,
//! for [`WAIT_FOR_NEW`] at most.
//!
//! A session that must be renewed while it is open (the budget of its relay
//! is spent, or the TCP probe found no data path) gets a new session for the
//! new flows with no gap; the old one carries its open flows for
//! [`RENEW_DRAIN`], then closes. [`Report`] gives the state of the data path
//! for the caller's status (podnet's `warp status`).
//!
//! The crate prints nothing: each line for the user is an event of the
//! caller's [`pod_core::Report`] (`warp.live.*`), and each renewal a
//! decision `warp.live`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use pod_core::{BoxFuture, Class, Dial, Event, Failure, Stage};
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;

use crate::net::WarpNet;
use crate::warp::Warp;

/// How long a caller waits for a new session while the session is down.
pub const WAIT_FOR_NEW: Duration = Duration::from_secs(20);
/// How often the watcher looks at the session.
const CHECK: Duration = Duration::from_millis(500);
/// After a planned replacement, the old session carries its open flows for
/// this long, then it is closed.
pub const RENEW_DRAIN: Duration = Duration::from_secs(90);
/// The first delay of the backoff after a failed open.
pub const BACKOFF_BASE: Duration = Duration::from_secs(1);
/// The largest delay of the backoff.
pub const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// A session that can close.
pub trait Closable: Send + Sync + 'static {
    fn is_closed(&self) -> bool;
    /// Why it closed (PodUtils/podnet issue 178), when it is known.
    fn reason(&self) -> Option<String> {
        None
    }
    /// Close it on purpose (PodUtils/podnet issue 185).
    fn close(&self, _why: &str) {}
    /// Whether a new session must take the new flows while this one goes
    /// on: the budget of the relay is spent.
    fn renew_due(&self) -> bool {
        false
    }
    /// The TCP probe of the data path: whether a TCP connect to `target`
    /// through the session succeeds within `limit`.
    fn probe_tcp(&self, _target: SocketAddr, _limit: Duration) -> ProbeFuture<'_> {
        Box::pin(async { true })
    }
}

/// The result of [`Closable::probe_tcp`].
pub type ProbeFuture<'a> = BoxFuture<'a, bool>;

/// The TCP probe of the data path (PodUtils/podnet@T-068). A MASQUE session
/// can carry ICMP and UDP but no TCP; the pongs and the UDP replies keep it
/// alive. Each `every`, a task connects to `target` through the session.
/// After `misses` failures in a row, the data path is `tcp-dead`, and a new
/// session takes the new flows (the path of the relay budget).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub target: SocketAddr,
    pub every: Duration,
    pub limit: Duration,
    pub misses: u32,
}

/// The default probe: TCP to 1.1.1.1:443 (the endpoint `trace` of
/// `cloudflare-dns`) each 30 s, 5 s each, 3 misses.
pub const DEFAULT_PROBE: Probe = Probe {
    target: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 443),
    every: Duration::from_secs(30),
    limit: Duration::from_secs(5),
    misses: 3,
};

/// The probe of `spec`: `IP:PORT` (the target, with the rest of
/// [`DEFAULT_PROBE`]) or `off` (no probe). Another text is `Usage`.
pub fn parse_probe(spec: &str) -> Result<Option<Probe>, Failure> {
    if spec == "off" {
        return Ok(None);
    }
    match spec.parse() {
        Ok(target) => Ok(Some(Probe { target, ..DEFAULT_PROBE })),
        Err(_) => Err(Failure::new(Stage::Warp, Class::Usage, format!("{spec:?} is no probe: give IP:PORT or off"))),
    }
}

/// The settings of a [`Live`] (podnet's `--probe` and `--session-max-age`).
#[derive(Clone, Debug)]
pub struct LiveSettings {
    /// The first delay of the backoff ([`BACKOFF_BASE`]).
    pub base: Duration,
    /// The largest delay of the backoff ([`BACKOFF_MAX`]).
    pub max: Duration,
    /// Each session is closed on purpose at this age, and a new one opens
    /// (PodUtils/podnet issue 185). `None`: never.
    pub max_age: Option<Duration>,
    /// The TCP probe of the data path ([`DEFAULT_PROBE`]). `None`: no probe.
    pub probe: Option<Probe>,
    /// The events and the decisions of the caller.
    pub report: pod_core::Report,
}

impl Default for LiveSettings {
    fn default() -> LiveSettings {
        LiveSettings {
            base: BACKOFF_BASE,
            max: BACKOFF_MAX,
            max_age: None,
            probe: Some(DEFAULT_PROBE),
            report: pod_core::Report::default(),
        }
    }
}

/// Opens a new session.
pub type Connect<T> = Box<dyn Fn() -> BoxFuture<'static, Result<T, Failure>> + Send + Sync>;

/// The state of the data path of a [`Live`], for the caller's status
/// (PodUtils/podnet@T-140).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// [`Live::data_path`]: `ok` or `tcp-dead`.
    pub data_path: &'static str,
    /// The Unix time of the last TCP probe, when one ran.
    pub last_probe: Option<u64>,
}

/// The current session, and a watcher that replaces it when it closes.
pub struct Live<T: Closable> {
    current: Mutex<Arc<T>>,
    generation: watch::Sender<u64>,
    reconnects: AtomicU32,
    /// The TCP probe found no data path: renew the session.
    tcp_dead: AtomicBool,
    report: watch::Sender<Report>,
    events: pod_core::Report,
    tasks: std::sync::Mutex<Vec<JoinHandle<()>>>,
}

impl<T: Closable> Live<T> {
    /// Start with `first`; `connect` opens each new session. It spawns its
    /// tasks: call it in a runtime of tokio.
    pub fn start(first: T, connect: Connect<T>, settings: &LiveSettings) -> Arc<Self> {
        Self::spawn(first, connect, settings.base, settings.max, settings.max_age, settings.report.clone())
            .probed(settings.probe)
    }

    /// [`Live::start`] with the first and the largest delay of the backoff,
    /// no age limit, no probe and no events.
    pub fn start_with(first: T, connect: Connect<T>, base: Duration, max: Duration) -> Arc<Self> {
        Self::start_aged(first, connect, base, max, None)
    }

    /// [`Live::start_with`], and each session ends at `max_age`.
    pub fn start_aged(
        first: T,
        connect: Connect<T>,
        base: Duration,
        max: Duration,
        max_age: Option<Duration>,
    ) -> Arc<Self> {
        Self::spawn(first, connect, base, max, max_age, pod_core::Report::default())
    }

    fn spawn(
        first: T,
        connect: Connect<T>,
        base: Duration,
        max: Duration,
        max_age: Option<Duration>,
        events: pod_core::Report,
    ) -> Arc<Self> {
        let (generation, _) = watch::channel(0u64);
        let live = Arc::new(Self {
            current: Mutex::new(Arc::new(first)),
            generation,
            reconnects: AtomicU32::new(0),
            tcp_dead: AtomicBool::new(false),
            report: watch::Sender::new(Report { data_path: "ok", last_probe: None }),
            events,
            tasks: std::sync::Mutex::new(Vec::new()),
        });
        let weak = Arc::downgrade(&live);
        let task = tokio::spawn(async move {
            let mut delay = Duration::ZERO;
            let mut opened = tokio::time::Instant::now();
            let check = max_age.map_or(CHECK, |age| CHECK.min(age / 4).max(Duration::from_millis(10)));
            loop {
                tokio::time::sleep(check).await;
                let Some(live) = weak.upgrade() else { return };
                // The owner ruling of podnet (2026-11-04): before the relay
                // ends a session (64 MiB, 12 h), a new session takes the new
                // flows; the open flows go on in the old one, which closes
                // after RENEW_DRAIN. The old one stays current until the new
                // one is open. A failed TCP probe takes the same path.
                if let Some((old, reason)) = live.renewal().await {
                    match connect().await {
                        Ok(next) => {
                            live.replace(next).await;
                            opened = tokio::time::Instant::now();
                            live.events.decision("warp.live", reason);
                            let text = format!(
                                "a new WARP session takes the new flows ({reason}); the old one carries its open flows for {} s",
                                RENEW_DRAIN.as_secs()
                            );
                            live.events.event(Event::info("warp.live.renew", text));
                            tokio::spawn(async move {
                                tokio::time::sleep(RENEW_DRAIN).await;
                                old.close(&format!("{reason}: the drain ended"));
                            });
                            delay = Duration::ZERO;
                        }
                        Err(err) => {
                            let text = format!(
                                "the WARP session must be renewed ({reason}); a new one failed, the next try follows: {}",
                                err.chain()
                            );
                            live.events.event(Event::warn("warp.live.renew", text));
                            tokio::time::sleep(base.max(Duration::from_secs(1))).await;
                        }
                    }
                    continue;
                }
                {
                    let current = live.current.lock().await;
                    if !current.is_closed() {
                        match max_age {
                            Some(age) if opened.elapsed() >= age => {
                                live.events.decision("warp.live", "max-age");
                                current.close("the session reached its most age");
                            }
                            _ => {
                                delay = Duration::ZERO;
                                continue;
                            }
                        }
                    }
                }
                if delay.is_zero() {
                    // PodUtils/podnet issues 116 and 178: each close, with its
                    // cause.
                    let reason = live.current.lock().await.reason();
                    let reason = reason.as_deref().unwrap_or("unknown");
                    live.events.decision("warp.live", "closed");
                    let text = format!("the WARP session is closed ({reason}); a new one opens");
                    live.events.event(Event::info("warp.live.closed", text));
                } else {
                    tokio::time::sleep(delay).await;
                }
                match connect().await {
                    Ok(next) => {
                        let n = live.replace(next).await;
                        opened = tokio::time::Instant::now();
                        live.events.event(Event::info("warp.live.open", format!("a new WARP session is open ({n})")));
                        delay = Duration::ZERO;
                    }
                    Err(err) => {
                        delay = if delay.is_zero() { base } else { (delay * 2).min(max) };
                        let text = format!(
                            "the WARP session is closed; a new one failed, the next try in {} ms: {}",
                            delay.as_millis(),
                            err.chain()
                        );
                        live.events.event(Event::warn("warp.live.failed", text));
                    }
                }
            }
        });
        live.keep(task);
        live
    }

    /// The session to renew while it is open, and why: the budget of its
    /// relay is spent, or the TCP probe found no data path.
    async fn renewal(&self) -> Option<(Arc<T>, &'static str)> {
        let current = self.current.lock().await;
        if current.is_closed() {
            return None;
        }
        let reason = if current.renew_due() {
            "relay-budget"
        } else if self.tcp_dead.load(Ordering::SeqCst) {
            "tcp-probe"
        } else {
            return None;
        };
        Some((current.clone(), reason))
    }

    /// Make `next` the current session; the count of the new sessions.
    async fn replace(&self, next: T) -> u32 {
        *self.current.lock().await = Arc::new(next);
        self.tell(false, false);
        let n = self.reconnects.fetch_add(1, Ordering::SeqCst) + 1;
        self.generation.send_modify(|g| *g += 1);
        n
    }

    fn keep(&self, task: JoinHandle<()>) {
        self.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(task);
    }

    /// Start the TCP probe of the data path on each session.
    pub fn probed(self: Arc<Self>, probe: Option<Probe>) -> Arc<Self> {
        let Some(probe) = probe else { return self };
        let weak = Arc::downgrade(&self);
        let task = tokio::spawn(async move {
            let mut missed = 0u32;
            let mut generation = 0u64;
            loop {
                tokio::time::sleep(probe.every).await;
                let Some(live) = weak.upgrade() else { return };
                let current = live.current.lock().await.clone();
                let now = *live.generation.borrow();
                if now != generation {
                    generation = now;
                    missed = 0;
                }
                if current.is_closed() || live.tcp_dead.load(Ordering::SeqCst) {
                    continue;
                }
                drop(live);
                let ok = current.probe_tcp(probe.target, probe.limit).await;
                let Some(live) = weak.upgrade() else { return };
                if ok || *live.generation.borrow() != generation {
                    if ok {
                        live.tell(false, true);
                    }
                    missed = 0;
                    continue;
                }
                missed += 1;
                live.tell(missed >= probe.misses, true);
                let text = format!("the TCP probe of the WARP session to {} failed ({missed} in a row)", probe.target);
                live.events.event(Event::warn("warp.live.probe", text));
                if missed >= probe.misses {
                    let text = "the WARP session carries no TCP; a new session takes the new flows";
                    live.events.event(Event::warn("warp.live.probe", text));
                    missed = 0;
                }
            }
        });
        self.keep(task);
        self
    }

    /// The state of the data path: `ok`, or `tcp-dead` until a new session
    /// opens.
    pub fn data_path(&self) -> &'static str {
        if self.tcp_dead.load(Ordering::SeqCst) { "tcp-dead" } else { "ok" }
    }

    /// The data path and the time of the last probe, on each change.
    pub fn report(&self) -> watch::Receiver<Report> {
        self.report.subscribe()
    }

    /// Set the state of the data path; `probed`: a probe ended now.
    fn tell(&self, tcp_dead: bool, probed: bool) {
        self.tcp_dead.store(tcp_dead, Ordering::SeqCst);
        let now = probed.then(unix_now);
        self.report.send_modify(|r| {
            r.data_path = self.data_path();
            r.last_probe = now.or(r.last_probe);
        });
    }

    /// The current session. While it is closed, wait for the new one (at
    /// most [`WAIT_FOR_NEW`]).
    pub async fn get(&self) -> Result<Arc<T>, Failure> {
        self.get_within(WAIT_FOR_NEW).await
    }

    /// [`Live::get`] with a time limit. No new session in time is
    /// `Unavailable`.
    pub async fn get_within(&self, limit: Duration) -> Result<Arc<T>, Failure> {
        let mut changes = self.generation.subscribe();
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            {
                let current = self.current.lock().await;
                if !current.is_closed() {
                    return Ok(current.clone());
                }
            }
            match tokio::time::timeout_at(deadline, changes.changed()).await {
                Ok(Ok(())) => continue,
                _ => {
                    let text =
                        format!("the WARP session is closed, and no new session opened in {} s", limit.as_secs());
                    return Err(Failure::new(Stage::Warp, Class::Unavailable, text));
                }
            }
        }
    }

    /// How many new sessions were opened.
    pub fn reconnects(&self) -> u32 {
        self.reconnects.load(Ordering::SeqCst)
    }
}

impl<T: Closable> Drop for Live<T> {
    fn drop(&mut self) {
        for task in self.tasks.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
            task.abort();
        }
    }
}

/// Give each new [`Report`] of `live` to `sink` (podnet: the record of its
/// instance), until `live` ends.
pub fn publish<T: Closable>(live: &Live<T>, sink: impl Fn(Report) + Send + 'static) {
    let mut changes = live.report();
    tokio::spawn(async move {
        while changes.changed().await.is_ok() {
            let report = *changes.borrow_and_update();
            sink(report);
        }
    });
}

/// The seconds since the Unix epoch of this host's clock.
fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Closable for WarpNet {
    fn is_closed(&self) -> bool {
        WarpNet::is_closed(self)
    }
    fn reason(&self) -> Option<String> {
        self.close_reason()
    }
    fn close(&self, why: &str) {
        WarpNet::close(self, why)
    }
    fn renew_due(&self) -> bool {
        WarpNet::renew_due(self)
    }
    fn probe_tcp(&self, target: SocketAddr, limit: Duration) -> ProbeFuture<'_> {
        Box::pin(async move {
            let deadline = tokio::time::Instant::now() + limit;
            self.connect_addr(target, deadline).await.is_ok()
        })
    }
}

/// The session of now for the clients of a proxy or a forward.
impl<T: Closable + Dial> crate::proxy::Tunnel for Live<T> {
    fn session(&self) -> BoxFuture<'_, Result<Arc<dyn Dial>, Failure>> {
        Box::pin(async move { Ok(self.get().await? as Arc<dyn Dial>) })
    }
}

impl Warp {
    /// Open a session, and keep it open: see [`Live`]. Each new session is
    /// [`Warp::connect`] of this device, with its check of the data path.
    pub async fn connect_live(self: Arc<Self>, settings: &LiveSettings) -> Result<Arc<Live<WarpNet>>, Failure> {
        let first = self.connect().await?;
        let warp = self.clone();
        let connect: Connect<WarpNet> = Box::new(move || {
            let warp = warp.clone();
            Box::pin(async move { warp.connect().await })
        });
        Ok(Live::start(first, connect, settings))
    }
}

#[cfg(test)]
mod tests;
