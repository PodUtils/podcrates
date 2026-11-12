//! A WARP session as a network of the caller (PodUtils/podnet#449): the
//! userspace stack of `pod-netstack` over a MASQUE session, given as a
//! [`Dial`] (TCP streams, names resolved inside the tunnel) and as
//! [`Datagrams`] (UDP). podnet's registry, router, groups and `--egress`
//! take it as one more network; podnet implements its own network traits
//! over these types (PodUtils/podnet#452).
//!
//! The stack owns the addresses of the device and routes each other address
//! to the session: its subnet has the prefix 0. The gateway of each family
//! (the host 1 of the prefix, `172.16.0.1` for WARP's IPv4 address) is an
//! address of the stack, and a packet to it stays in the stack.
//!
//! When the session closes (the edge ends it, a pump ends, the reactor of
//! the stack stops, [`WarpNet::close`]), each open stream and each UDP
//! socket fails at once with `ConnectionReset` and the first cause, and a
//! new dial fails with it (PodUtils/podnet issues 112, 125 and 185).
//!
//! [`NetSettings`] tunes the stack: the congestion controller, the TCP
//! burst, the counters of the link, the check of the data path and the
//! budget of a relay (podnet's `src/warp/ip.rs`).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use pod_core::{BoxFuture, Class, Connected, Datagrams, Dial, Event, Failure, Report, Stage, Target};
use pod_netstack::{NetStack, StackReset};
use tokio::task::JoinHandle;

mod settings;
pub use pod_netstack::device::{DeviceStats, DeviceStatsSnapshot};
pub use pod_netstack::smoltcp::socket::tcp::CongestionControl;
pub use settings::{NetSettings, Renew, TCP_BUFFER, TCP_BURST_MAX, parse_cc, parse_tcp_burst, tcp_burst_max};

use crate::session::{MasqueRx, MasqueSession, MasqueTx};
use crate::warp::Warp;

/// The interval of the liveness check of a session.
pub const KEEPALIVE: Duration = Duration::from_secs(15);
/// The most bytes of packets that go in one DATA frame (PodUtils/podnet
/// issue 122).
pub const BATCH_BYTES: usize = 16 * 1024;
/// The time limit of a lookup of a name inside the tunnel.
pub const DNS_TIMEOUT: Duration = Duration::from_secs(5);

/// The cause of the close of a session: the first cause stays.
#[derive(Clone)]
struct Closed {
    flag: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<String>>>,
    /// The reset of each socket of the stack at the first cause.
    reset: StackReset,
}

impl Closed {
    fn new(reset: StackReset) -> Closed {
        Closed { flag: Arc::default(), reason: Arc::default(), reset }
    }

    /// Mark the session closed for `why`. At the first cause, each open
    /// stream and UDP socket of the stack fails with it (PodUtils/podnet
    /// issue 125): before, a read waited for data that could not come.
    fn mark(&self, why: String) {
        let mut slot = self.reason.lock().unwrap_or_else(|e| e.into_inner());
        let first = slot.is_none();
        if first {
            *slot = Some(why.clone());
        }
        self.flag.store(true, Ordering::Relaxed);
        drop(slot);
        if first {
            self.reset.reset(closed_text(&why));
        }
    }
}

/// The text of an operation on a closed session.
fn closed_text(why: &str) -> String {
    format!("the WARP session is closed: {why}")
}

/// A WARP session with its stack: a [`Dial`] and a source of [`Datagrams`].
/// A clone is a handle on the same session; the session ends with the last
/// handle.
#[derive(Clone)]
pub struct WarpNet {
    inner: Arc<Inner>,
}

struct Inner {
    stack: NetStack,
    v4: Option<Ipv4Addr>,
    v6: Option<Ipv6Addr>,
    closed: Closed,
    pumps: Vec<JoinHandle<()>>,
    next_port: AtomicU16,
    resolvers: Vec<IpAddr>,
    renew: Option<Renew>,
    /// The time of the answer of the check of the data path, after one.
    checked: OnceLock<Duration>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        for pump in &self.pumps {
            pump.abort();
        }
    }
}

impl WarpNet {
    /// The stack over `session` with the addresses `v4` and `v6` of the
    /// device (one at least) and the settings `settings` (the check of the
    /// data path is [`Warp::connect`]'s). It spawns its tasks: call it in a
    /// runtime of tokio.
    pub fn start(
        session: MasqueSession,
        v4: Option<Ipv4Addr>,
        v6: Option<Ipv6Addr>,
        settings: &NetSettings,
        report: Report,
    ) -> Result<WarpNet, Failure> {
        let config = settings.stack_config(v4, v6)?;
        tracing::debug!(
            segments = ?settings.tcp_burst,
            cc = ?settings.congestion_control,
            "the TCP window and the congestion controller of the WARP stack"
        );
        let stack = NetStack::over_link(config);
        let closed = Closed::new(stack.resetter());
        let (tx, rx) = session.split();
        let pumps = pumps(tx, rx, &stack, &closed, report, NetSettings::ours(v4, v6));
        let resolvers = crate::dns::RESOLVERS.to_vec();
        let next_port = AtomicU16::new(49152);
        let renew = settings.renew.clone();
        let checked = OnceLock::new();
        let inner = Inner { stack, v4, v6, closed, pumps, next_port, resolvers, renew, checked };
        Ok(WarpNet { inner: Arc::new(inner) })
    }

    /// The addresses of the device in the tunnel.
    pub fn addresses(&self) -> Vec<IpAddr> {
        self.inner.v4.map(IpAddr::V4).into_iter().chain(self.inner.v6.map(IpAddr::V6)).collect()
    }

    /// True when the session can carry no more packets.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.flag.load(Ordering::Relaxed)
    }

    /// Why the session closed: the first cause.
    pub fn close_reason(&self) -> Option<String> {
        self.inner.closed.reason.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Close the session on purpose for `why` (PodUtils/podnet issue 185:
    /// the most age of a session), so that the caller opens a new one. Each
    /// open stream and UDP socket fails with `ConnectionReset` and the
    /// first cause, a new dial and a new UDP socket fail, and the tasks of
    /// the session stop. A second close keeps the first cause.
    pub fn close(&self, why: &str) {
        self.inner.closed.mark(why.to_string());
        for pump in &self.inner.pumps {
            pump.abort();
        }
    }

    /// The failure of a new operation on a closed session.
    fn closed_failure(&self) -> Option<Failure> {
        self.is_closed().then(|| {
            let why = self.close_reason().unwrap_or_default();
            Failure::new(Stage::Warp, Class::Unavailable, closed_text(&why))
        })
    }

    /// The stack, for a listener or a socket that the dial does not give.
    pub fn stack(&self) -> &NetStack {
        &self.inner.stack
    }

    /// The counters of the link of the session (PodUtils/podnet issue 134):
    /// the packets and the bytes each way, the drops, and `tx_held`, the
    /// times that the stack kept its packets for a full queue to the
    /// session. With [`NetSettings::link_stats`], the shared counters.
    pub fn link_stats(&self) -> DeviceStatsSnapshot {
        self.inner.stack.device_stats()
    }

    /// Whether the budget of the relay of this session is spent
    /// ([`NetSettings::renew`]): a new session must take the new flows.
    pub fn renew_due(&self) -> bool {
        self.inner.renew.as_ref().is_some_and(|spent| spent())
    }

    /// The time of the answer of the check of the data path, after a check
    /// that passed.
    pub fn checked(&self) -> Option<Duration> {
        self.inner.checked.get().copied()
    }

    pub(crate) fn note_checked(&self, took: Duration) {
        let _ = self.inner.checked.set(took);
    }

    /// The resolvers that a name asks inside the tunnel ([`crate::dns`]).
    pub fn with_resolvers(self, resolvers: Vec<IpAddr>) -> WarpNet {
        match Arc::try_unwrap(self.inner) {
            Ok(mut inner) => {
                inner.resolvers = resolvers;
                WarpNet { inner: Arc::new(inner) }
            }
            Err(inner) => WarpNet { inner },
        }
    }

    fn family_failure(&self, ip: IpAddr) -> Option<Failure> {
        let has = if ip.is_ipv4() { self.inner.v4.is_some() } else { self.inner.v6.is_some() };
        let family = if ip.is_ipv4() { "IPv4" } else { "IPv6" };
        (!has)
            .then(|| Failure::new(Stage::Warp, Class::Unavailable, format!("the WARP device has no {family} address")))
    }

    /// A TCP stream to `addr` inside the tunnel, established by `deadline`.
    pub(crate) async fn connect_addr(
        &self,
        addr: SocketAddr,
        deadline: tokio::time::Instant,
    ) -> Result<Connected, Failure> {
        if let Some(failure) = self.family_failure(addr.ip()) {
            return Err(failure);
        }
        if let Some(failure) = self.closed_failure() {
            return Err(failure);
        }
        let open = async {
            let mut stream = self.inner.stack.connect(addr).await?;
            stream.established().await?;
            Ok::<_, std::io::Error>(stream)
        };
        let stream = tokio::time::timeout_at(deadline, open)
            .await
            .map_err(|_| {
                Failure::new(Stage::Connect, Class::Timeout, format!("{addr} through WARP: no connection in time"))
            })?
            .map_err(|e| {
                let text = format!("{addr} through WARP");
                Failure::caused(Stage::Connect, Class::of_io(e.kind()), text, e)
            })?;
        let local = stream.local_addr();
        Ok(Connected { io: Box::new(stream), peer: Some(addr), local: Some(local) })
    }

    /// A UDP socket of the stack on an ephemeral port of each address.
    pub(crate) async fn bind_udp(&self) -> Result<pod_netstack::UdpSocket, Failure> {
        if let Some(failure) = self.closed_failure() {
            return Err(failure);
        }
        let mut last = None;
        for _ in 0..64 {
            let port = self.inner.next_port.fetch_add(1, Ordering::Relaxed);
            let port = if port < 49152 { 49152 + port % 16384 } else { port };
            match self.inner.stack.bind_udp_any(port).await {
                Ok(socket) => return Ok(socket),
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => last = Some(e),
                Err(e) => return Err(Failure::io(Stage::Warp, e)),
            }
        }
        let e = last.unwrap_or_else(|| std::io::Error::from(std::io::ErrorKind::AddrInUse));
        Err(Failure::caused(Stage::Warp, Class::TempFail, "no free UDP port in the WARP stack", e))
    }
}

impl Dial for WarpNet {
    fn label(&self) -> String {
        "WARP".into()
    }

    fn dial<'a>(
        &'a self,
        target: &'a Target,
        deadline: tokio::time::Instant,
    ) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let addrs = match target {
                Target::Addr(addr) => vec![*addr],
                Target::Name { host, port } => {
                    let limit = deadline.saturating_duration_since(tokio::time::Instant::now()).min(DNS_TIMEOUT);
                    let ips = crate::dns::lookup(self, &self.inner.resolvers, host, limit).await?;
                    ips.into_iter().map(|ip| SocketAddr::new(ip, *port)).collect()
                }
            };
            self.dial_addrs(&target.host(), &addrs, deadline).await
        })
    }

    fn dial_addrs<'a>(
        &'a self,
        host: &'a str,
        addrs: &'a [SocketAddr],
        deadline: tokio::time::Instant,
    ) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let mut last = None;
            for addr in addrs.iter().filter(|a| self.family_failure(a.ip()).is_none()) {
                match self.connect_addr(*addr, deadline).await {
                    Ok(connected) => return Ok(connected),
                    Err(e) => last = Some(e),
                }
            }
            Err(last.unwrap_or_else(|| {
                let text = format!("{host} has no address of a family of the WARP device");
                Failure::new(Stage::Dns, Class::Unavailable, text)
            }))
        })
    }

    fn names_inside(&self) -> bool {
        true
    }

    fn datagrams<'a>(&'a self) -> BoxFuture<'a, Result<Arc<dyn Datagrams>, Failure>> {
        Box::pin(async move {
            let socket = self.bind_udp().await?;
            Ok(Arc::new(socket.into_datagrams()) as Arc<dyn Datagrams>)
        })
    }
}

/// The tasks of a session: the liveness check, the send loop, the receive
/// loop and the watch of the reactor of the stack.
fn pumps(
    mut tx: MasqueTx,
    mut rx: MasqueRx,
    stack: &NetStack,
    closed: &Closed,
    report: Report,
    ours: Vec<IpAddr>,
) -> Vec<JoinHandle<()>> {
    let mut tasks = Vec::new();
    // PodUtils/podnet issue 112: a stack whose reactor stopped, also by a
    // panic, carries nothing: the session is closed.
    let stopped = stack.stopped();
    let stop_closed = closed.clone();
    tasks.push(tokio::spawn(async move {
        let why = stopped.await;
        stop_closed.mark(format!("the stack of the session stopped: {why}"));
    }));
    // PodUtils/podnet issue 104: a session that gives no pong and no data is
    // closed, so that the caller opens a new one.
    if let Some(keepalive) = tx.keepalive(&rx) {
        let closed = closed.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(err) = keepalive.run(KEEPALIVE).await {
                closed.mark(format!("no pong: {}", err.chain()));
            }
        }));
    }
    let outbound = stack.link_out();
    let send_closed = closed.clone();
    tasks.push(tokio::spawn(async move {
        let why = loop {
            let mut batch = vec![outbound.recv().await];
            let mut bytes = batch[0].len();
            while bytes < BATCH_BYTES {
                let Some(next) = outbound.pop() else { break };
                bytes += next.len();
                batch.push(next);
            }
            if let Err(err) = tx.send_packets(&batch).await {
                break format!("a send failed: {}", err.chain());
            }
        };
        send_closed.mark(why);
    }));
    let inbound = stack.link_in();
    let recv_closed = closed.clone();
    tasks.push(tokio::spawn(async move {
        let why = loop {
            let packet = rx.recv_packet().await;
            // PodUtils/podnet issue 121: the addresses that the front door
            // assigned.
            if let Some(assigned) = rx.take_assigned() {
                note_assigned(&report, &assigned, &ours);
            }
            match packet {
                Ok(Some(packet)) => {
                    inbound.push(packet.to_vec());
                }
                Ok(None) => break "the front door ended the session".to_string(),
                Err(err) => break format!("a receive failed: {}", err.chain()),
            }
        };
        recv_closed.mark(why);
    }));
    tasks
}

/// An event for an ADDRESS_ASSIGN: a warning when the front door assigned an
/// address that the stack does not have (the stack uses the record's).
fn note_assigned(report: &Report, assigned: &[(IpAddr, u8)], ours: &[IpAddr]) {
    let shown: Vec<String> = assigned.iter().map(|(ip, p)| format!("{ip}/{p}")).collect();
    if assigned.iter().all(|(ip, _)| ours.contains(ip)) {
        tracing::info!(assigned = %shown.join(" "), "the front door assigned the addresses of the stack");
    } else {
        let text = format!("the front door assigned {} and the stack has {ours:?}", shown.join(" "));
        report.event(Event::warn("warp.address", text));
    }
}

impl Warp {
    /// Open a session ([`Warp::open_session`]), start the stack over it, and
    /// check that a packet crosses it ([`NetSettings::check`],
    /// PodUtils/podnet issue 120). A session that carries no packet is a
    /// failure, and its door goes last for the next session
    /// ([`Warp::mark_door_bad`], issue 133).
    pub async fn connect(&self) -> Result<WarpNet, Failure> {
        let session = self.open_session().await?;
        let (device, settings) = (&self.device, &self.settings);
        let net = WarpNet::start(session, device.ipv4(), device.ipv6(), &settings.net, settings.report.clone())?;
        let Some(check) = &settings.net.check else {
            tracing::debug!("no check of the data path");
            return Ok(net);
        };
        match net.check_data_path(check).await {
            Ok(took) => {
                tracing::info!(ms = took.as_millis() as u64, "a packet crossed the MASQUE session");
                settings.report.decision("warp.check", "answered");
                Ok(net)
            }
            Err(failure) => {
                self.mark_door_bad();
                settings.report.decision("warp.check", "silent");
                Err(failure)
            }
        }
    }
}
