//! Async `tokio` runner driving [`ProbeMachine`] over caller-injected seams (`pod_core::Datagrams`,
//! `Clock`, `Entropy`, `Events`). Single-flight per [`Prober`].

pub mod system;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use pod_core::clock::{Clock, SystemClock};
use pod_core::datagrams::{recv_from, send_to};
use pod_core::entropy::{Entropy, OsEntropy};
use pod_core::{Datagrams, Event, Failure};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::engine::{Input, Output, ProbeEvent, ProbeMachine};
use crate::error::NatError;
use crate::report::{Report, SockId, Tick};
use crate::seams::async_seams::{Nat64Resolver, Netenv, OnlineCheck, UdpFactory, UpnpHttp};
use crate::settings::Settings;

/// Caller-injected seams for [`Prober::run`].
pub struct Seams {
    pub udp: Arc<dyn UdpFactory>,
    pub netenv: Arc<dyn Netenv>,
    pub clock: Arc<dyn Clock>,
    pub entropy: Box<dyn Entropy + Send>,
    pub nat64: Option<Arc<dyn Nat64Resolver>>,
    pub upnp_http: Option<Arc<dyn UpnpHttp>>,
    pub online: Option<Arc<dyn OnlineCheck>>,
    pub report_sink: pod_core::Report,
}

impl Seams {
    /// Standard system seams using OS UDP sockets, zero-packet route discovery, system clock
    /// and OS entropy.
    pub fn system() -> Self {
        Self {
            udp: Arc::new(system::SystemUdpFactory),
            netenv: Arc::new(system::SystemNetenv),
            clock: Arc::new(SystemClock),
            entropy: Box::new(OsEntropy),
            nat64: None,
            upnp_http: None,
            online: None,
            report_sink: pod_core::Report::default(),
        }
    }
}

/// Single-flight async NAT prober. Keeps no process-global state.
#[derive(Debug, Default)]
pub struct Prober {
    active: AtomicBool,
}

struct ActiveGuard<'a>(&'a AtomicBool);
impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Drop guard aborting all spawned UDP reader and auxiliary tasks if `Prober::run` is dropped
/// or cancelled mid-flight.
#[derive(Default)]
struct TaskPool {
    sockets: BTreeMap<SockId, (Arc<dyn Datagrams>, JoinHandle<()>)>,
    aux: Vec<JoinHandle<()>>,
}

impl Drop for TaskPool {
    fn drop(&mut self) {
        for (_, (_, h)) in std::mem::take(&mut self.sockets) {
            h.abort();
        }
        for h in self.aux.drain(..) {
            h.abort();
        }
    }
}

impl Prober {
    /// Construct a new prober instance.
    pub fn new() -> Self {
        Self { active: AtomicBool::new(false) }
    }

    /// Execute one NAT probe run to completion or cancellation.
    pub async fn run(&self, settings: Settings, seams: Seams) -> Result<Report, Failure> {
        if self.active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err(NatError::Busy.into());
        }
        let _guard = ActiveGuard(&self.active);

        let Seams { udp, netenv, clock, entropy, nat64, upnp_http, online, report_sink } = seams;
        let local_env = netenv.snapshot();
        let mut machine = ProbeMachine::new(settings, local_env, entropy).map_err(Failure::from)?;
        let origin = clock.now();
        let tick_now = |c: &dyn Clock| -> Tick {
            c.now().saturating_duration_since(origin).as_micros().min(u128::from(u64::MAX)) as u64
        };

        // Keep control completions on an unbounded channel separate from the bounded datagram
        // channel so datagram bursts can never drop Bound/Dns64/Http/Online completions.
        let (ctl_tx, mut ctl_rx) = mpsc::unbounded_channel::<Input>();
        let (dg_tx, mut dg_rx) = mpsc::channel::<Input>(512);
        let mut pool = TaskPool::default();
        let io_seams = IoSeams { udp, nat64, upnp_http, online };

        machine.start(tick_now(clock.as_ref()));
        report_sink.events.event(Event::info("natprobe.start", "the NAT probe starts"));

        let final_report = loop {
            while let Some(out) = machine.poll_output() {
                perform_output(out, &io_seams, &ctl_tx, &dg_tx, &mut pool).await;
            }
            while let Some(ev) = machine.poll_event() {
                match ev {
                    ProbeEvent::Partial(q) => {
                        tracing::debug!(question = ?q, "natprobe question decided");
                    }
                    ProbeEvent::Final(report) => {
                        drop(pool);
                        report_sink.events.event(Event::info("natprobe.done", report.to_string()));
                        return Ok(*report);
                    }
                }
            }
            let Some(next_tick) = machine.poll_timeout() else {
                break None;
            };
            let now_t = tick_now(clock.as_ref()).max(machine.current_tick());
            if now_t >= next_tick {
                machine.handle_timeout(now_t);
                continue;
            }
            let wait = Duration::from_micros(next_tick.saturating_sub(now_t));
            let sleep = tokio::time::sleep(wait);
            tokio::pin!(sleep);
            tokio::select! {
                maybe_ctl = ctl_rx.recv() => {
                    let t = tick_now(clock.as_ref()).max(machine.current_tick());
                    match maybe_ctl {
                        Some(inp) => machine.handle_input(t, inp),
                        None => machine.handle_input(t, Input::Cancel),
                    }
                }
                maybe_dg = dg_rx.recv() => {
                    let t = tick_now(clock.as_ref()).max(machine.current_tick());
                    if let Some(inp) = maybe_dg {
                        machine.handle_input(t, inp);
                    }
                }
                () = &mut sleep => {
                    let t = tick_now(clock.as_ref()).max(next_tick);
                    machine.handle_timeout(t);
                }
            }
        };

        drop(pool);
        final_report.ok_or_else(|| {
            NatError::ReportInvariant("the probe machine stopped with no final report".to_string()).into()
        })
    }
}

struct IoSeams {
    udp: Arc<dyn UdpFactory>,
    nat64: Option<Arc<dyn Nat64Resolver>>,
    upnp_http: Option<Arc<dyn UpnpHttp>>,
    online: Option<Arc<dyn OnlineCheck>>,
}

async fn perform_output(
    out: Output,
    seams: &IoSeams,
    ctl_tx: &mpsc::UnboundedSender<Input>,
    dg_tx: &mpsc::Sender<Input>,
    pool: &mut TaskPool,
) {
    match out {
        Output::Bind { sock, spec } => match seams.udp.bind(spec).await {
            Ok(bound) => {
                let dg = bound.socket;
                let local = bound.local;
                let reader_dg = Arc::clone(&dg);
                let reader_tx = dg_tx.clone();
                let handle = tokio::spawn(async move {
                    let mut buf = vec![0u8; 2048];
                    while let Ok((n, from)) = recv_from(reader_dg.as_ref(), &mut buf).await {
                        if reader_tx.send(Input::Datagram { sock, from, bytes: buf[..n].to_vec() }).await.is_err() {
                            break;
                        }
                    }
                });
                if let Some((_, prev)) = pool.sockets.insert(sock, (dg, handle)) {
                    prev.abort();
                }
                let _ = ctl_tx.send(Input::Bound { sock, local });
            }
            Err(e) => {
                let _ = ctl_tx.send(Input::BindFailed { sock, reason: e.chain() });
            }
        },
        Output::Send { sock, dest, bytes } => {
            if let Some((dg, _)) = pool.sockets.get(&sock) {
                let dg = Arc::clone(dg);
                let _ = send_to(dg.as_ref(), &bytes, dest).await;
            }
        }
        Output::Close { sock } => {
            if let Some((_, handle)) = pool.sockets.remove(&sock) {
                handle.abort();
            }
        }
        Output::QueryDns64 { domain } => {
            if let Some(resolver) = seams.nat64.clone() {
                let tx = ctl_tx.clone();
                pool.aux.push(tokio::spawn(async move {
                    let res = resolver.query_ipv4only(&domain).await;
                    let _ = tx.send(Input::Dns64Answer {
                        aaaa: res.aaaa,
                        has_a: res.has_a,
                        resolver_is_doh: res.resolver_is_doh,
                        failed: res.failed,
                    });
                }));
            } else {
                let _ = ctl_tx.send(Input::Dns64Answer {
                    aaaa: Vec::new(),
                    has_a: false,
                    resolver_is_doh: false,
                    failed: true,
                });
            }
        }
        Output::HttpGet { id, url } => {
            if let Some(http) = seams.upnp_http.clone() {
                let tx = ctl_tx.clone();
                pool.aux.push(tokio::spawn(async move {
                    let body = http.get(&url).await;
                    let _ = tx.send(Input::HttpReply { id, body });
                }));
            } else {
                let _ = ctl_tx.send(Input::HttpReply { id, body: Err("no http client".to_string()) });
            }
        }
        Output::HttpSoap { id, url, action, body } => {
            if let Some(http) = seams.upnp_http.clone() {
                let tx = ctl_tx.clone();
                pool.aux.push(tokio::spawn(async move {
                    let res = http.soap_post(&url, &action, &body).await;
                    let _ = tx.send(Input::HttpReply { id, body: res });
                }));
            } else {
                let _ = ctl_tx.send(Input::HttpReply { id, body: Err("no http client".to_string()) });
            }
        }
        Output::CheckOnline => {
            if let Some(online) = seams.online.clone() {
                let tx = ctl_tx.clone();
                pool.aux.push(tokio::spawn(async move {
                    let ok = online.check().await;
                    let _ = tx.send(Input::OnlineResult { ok });
                }));
            } else {
                let _ = ctl_tx.send(Input::OnlineResult { ok: false });
            }
        }
    }
}

/// Convert a monotonic `Instant` delta relative to `origin` into microseconds.
pub fn tick_since(origin: Instant, now: Instant) -> Tick {
    now.saturating_duration_since(origin).as_micros().min(u128::from(u64::MAX)) as u64
}
