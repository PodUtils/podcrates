//! The hold of each SYN on the wire of the netns hold (the feature
//! `syn-hold`): the stack of `pod-netstack` answers for each destination of
//! the payload, and it accepts a connection only after the consumer dialed
//! its upstream.
//!
//! The consumer reads the IP packets of the payload from its wire (the TUN or
//! the veth pair of [`crate::netns`]) and gives them to [`SynHold::pump`].
//! The pump holds each new TCP SYN of IPv4 and starts the dial of the
//! consumer ([`Dial`]) in a task of its own. When the dial succeeds, the pump
//! gives the SYN to the stack: a listener on any address of its port accepts
//! it, and the stream comes out with its upstream as a [`Flow`]. When the
//! dial fails, the pump answers the SYN with a RST: the `connect` of the
//! payload fails with `ECONNREFUSED` at once, and the stack never sees the
//! SYN. Each other packet goes to the stack. The packets of the stack and the
//! RSTs go back to the wire.
//!
//! The loop of the consumer:
//!
//! ```text
//! loop {
//!     read the wire into `from_payload` (the DNS and the UDP flows first);
//!     hold.pump(&mut from_payload, &mut to_payload, &dial).await;
//!     while let Some(flow) = hold.next_flow() { spawn the copy of the flow }
//!     write `to_payload` to the wire;
//!     wait for the wire, `hold.activity()` or a timer (the time limits);
//! }
//! ```
//!
//! The copy of a flow, the rules, the names of the targets, the DNS, the UDP
//! flows and the log of the flows are the consumer's. Each binary that turns
//! the feature on sets `SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"` (`pod-netstack`).

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::net::{SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use pod_netstack::{NetStack, PacketQueue, StackConfig, TcpStream};
use tokio::sync::{Notify, mpsc};
use tokio::task::{JoinError, JoinHandle};

use crate::packet::{parse_tcp_syn, rst_reply_for_syn};

mod flows;
#[cfg(test)]
mod tests;

pub use flows::{Flow, Open};
use flows::{Late, Opened};

/// The addresses of a TCP connection of the payload: the source and the
/// target (the destination) of its SYN.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Syn {
    pub source: SocketAddrV4,
    pub target: SocketAddrV4,
}

impl Syn {
    /// The connection of a bare IPv4 TCP SYN ([`parse_tcp_syn`]); `None` for
    /// each other packet.
    pub fn of(packet: &[u8]) -> Option<Self> {
        let (source, source_port, target, target_port) = parse_tcp_syn(packet)?;
        Some(Self { source: SocketAddrV4::new(source, source_port), target: SocketAddrV4::new(target, target_port) })
    }

    /// The connection of a stream that the stack accepted: its peer is the
    /// source and its local address is the target. `None` for IPv6.
    fn of_stream(stream: &TcpStream) -> Option<Self> {
        match (stream.peer_addr(), stream.local_addr()) {
            (SocketAddr::V4(source), SocketAddr::V4(target)) => Some(Self { source, target }),
            _ => None,
        }
    }
}

/// The dial of the consumer: the upstream of each held SYN.
pub trait Dial {
    /// What a dial gives: the upstream, with what the consumer keeps for the
    /// flow (a name, the number of a log line).
    type Upstream: Send + 'static;
    /// The failure of a dial. A dial task that panicked becomes one.
    type Error: From<JoinError> + Send + 'static;

    /// Start the dial of the upstream of `syn`. The pump calls it one time
    /// for each held SYN, and runs the future in a task of its own, so that
    /// a slow dial does not stop the other packets.
    fn dial(&self, syn: Syn) -> impl Future<Output = Result<Self::Upstream, Self::Error>> + Send + 'static;

    /// The dial of `syn` failed with `error`, and the payload got `refused`.
    /// The default does nothing.
    fn refused(&self, syn: Syn, refused: Refused, error: Self::Error) {
        let _ = (syn, refused, error);
    }
}

/// A closure `Fn(Syn) -> Future` is a [`Dial`] that keeps no note of a
/// refusal.
impl<F, Fut, U, E> Dial for F
where
    F: Fn(Syn) -> Fut,
    Fut: Future<Output = Result<U, E>> + Send + 'static,
    U: Send + 'static,
    E: From<JoinError> + Send + 'static,
{
    type Upstream = U;
    type Error = E;

    fn dial(&self, syn: Syn) -> impl Future<Output = Result<U, E>> + Send + 'static {
        self(syn)
    }
}

/// What the payload got for a failed dial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// A RST to its SYN: its `connect` fails with `ECONNREFUSED`.
    Reset,
    /// The stack accepted the connection before its dial (a late dial,
    /// [`Stats::late`]): the stream is aborted, and the payload reads a
    /// reset.
    Aborted,
}

/// The settings of a hold. The defaults are podnet's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The connections of the payload at one time: held, dialed and open. A
    /// new SYN past it is dropped, and the payload sends it again (no RST).
    pub max_flows: usize,
    /// The time that the upstream of a released SYN waits for its stream.
    pub dialed_ttl: Duration,
    /// The time that a released SYN keeps the listener of its port while its
    /// handshake runs.
    pub handshake_ttl: Duration,
}

impl Default for Settings {
    /// 4096 connections (podnet's `MAX_TRACKED_FDS`), 30 s for a dialed
    /// upstream, 10 s for a handshake.
    fn default() -> Self {
        Self { max_flows: 4096, dialed_ttl: Duration::from_secs(30), handshake_ttl: Duration::from_secs(10) }
    }
}

/// The counts of a hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// The SYNs held, each with its dial.
    pub held: u64,
    /// The held SYNs whose dial succeeded: the stack took them.
    pub released: u64,
    /// The held SYNs whose dial failed: each got a RST.
    pub reset: u64,
    /// A SYN again while the dial of its connection ran: dropped.
    pub again: u64,
    /// A new SYN past `max_flows`: dropped.
    pub over_budget: u64,
    /// A SYN that found no listener (the stack refused it): dropped.
    pub no_listener: u64,
    /// A stream that the stack accepted with no upstream (its upstream
    /// passed `dialed_ttl`): its dial started after the accept.
    pub late: u64,
    /// A late dial that failed: its stream was aborted.
    pub aborted: u64,
    /// The flows given to the consumer.
    pub flows: u64,
}

/// A held SYN and the dial of its upstream.
struct Held<U, E> {
    packet: Vec<u8>,
    task: JoinHandle<Result<U, E>>,
}

/// The hold of each SYN of a payload, over its stack (the module docs).
/// `U` is the upstream of a dial and `E` its failure ([`Dial`]).
pub struct SynHold<U, E> {
    stack: NetStack,
    /// The packets of the payload for the stack, and from it.
    link_in: PacketQueue,
    link_out: PacketQueue,
    settings: Settings,
    /// The destination ports with a listener, and the task of each that
    /// takes its accepted streams.
    ports: HashMap<u16, JoinHandle<()>>,
    accepted_tx: mpsc::UnboundedSender<TcpStream>,
    accepted_rx: mpsc::UnboundedReceiver<TcpStream>,
    /// Wakes [`SynHold::activity`]: a stream accepted, a dial ended, a flow
    /// ended.
    wake: Arc<Notify>,
    /// The released SYNs that the stack has, until their stream is accepted.
    handshakes: HashMap<Syn, Instant>,
    /// The connections that the stack accepted, until their flow ends.
    open: Opened,
    /// Each new SYN waits here while its upstream is dialed.
    held: HashMap<Syn, Held<U, E>>,
    /// The upstreams of the released SYNs, until their stream is accepted.
    dialed: HashMap<Syn, (U, Instant)>,
    /// The accepted streams whose dial started after the accept.
    late: Vec<Late<U, E>>,
    /// The flows for the consumer.
    ready: VecDeque<Flow<U>>,
    stats: Stats,
}

impl<U, E> SynHold<U, E>
where
    U: Send + 'static,
    E: From<JoinError> + Send + 'static,
{
    /// A hold on a new stack of `config` over a link, with any IP on: the
    /// stack answers for each destination of the payload. Call it in a
    /// runtime of tokio: the stack spawns its reactor.
    pub fn new(mut config: StackConfig, settings: Settings) -> Self {
        config.any_ip = true;
        let stack = NetStack::over_link(config);
        let (accepted_tx, accepted_rx) = mpsc::unbounded_channel();
        Self {
            link_in: stack.link_in(),
            link_out: stack.link_out(),
            stack,
            settings,
            ports: HashMap::new(),
            accepted_tx,
            accepted_rx,
            wake: Arc::new(Notify::new()),
            handshakes: HashMap::new(),
            open: Opened::default(),
            held: HashMap::new(),
            dialed: HashMap::new(),
            late: Vec::new(),
            ready: VecDeque::new(),
            stats: Stats::default(),
        }
    }

    /// The stack of the hold (its counters, its sockets).
    pub fn stack(&self) -> &NetStack {
        &self.stack
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The SYNs that wait for their dial now.
    pub fn held_count(&self) -> usize {
        self.held.len()
    }

    /// The flows now: the accepted connections whose [`Open`] lives.
    pub fn flow_count(&self) -> usize {
        self.open.len()
    }

    /// The destination ports with a listener now.
    pub fn listening_ports(&self) -> usize {
        self.ports.len()
    }

    /// Wait until the stack has a packet for the wire, the stack accepted a
    /// stream, a dial ended or a flow ended. Then call [`SynHold::pump`].
    pub async fn activity(&self) {
        tokio::select! {
            _ = self.link_out.wait() => {}
            _ = self.wake.notified() => {}
        }
    }

    /// One pass of the hold. It takes each packet of `from_payload` (the IP
    /// packets that the payload sent, with no DNS and no UDP flow that the
    /// consumer took first): it holds each new SYN and starts its dial, and
    /// gives the other packets to the stack. It releases each SYN whose dial
    /// succeeded, and puts a RST for each SYN whose dial failed into
    /// `to_payload`, with each packet of the stack.
    pub async fn pump<D>(&mut self, from_payload: &mut VecDeque<Vec<u8>>, to_payload: &mut VecDeque<Vec<u8>>, dial: &D)
    where
        D: Dial<Upstream = U, Error = E>,
    {
        self.accept_flows(dial);
        self.end_late_dials(dial).await;
        self.hold_syns(from_payload, to_payload, dial).await;
        self.feed(from_payload).await;
        self.close_idle_ports();
        self.collect(to_payload);
    }

    /// The next flow for the consumer: an accepted stream and its upstream.
    pub fn next_flow(&mut self) -> Option<Flow<U>> {
        self.ready.pop_front()
    }

    /// Stop the listeners and the dials, then the stack.
    pub async fn shutdown(&mut self) {
        self.abort_tasks();
        self.stack.shutdown().await;
    }

    /// The dial of `syn` in a task that wakes the hold at its end.
    fn spawn_dial<D: Dial<Upstream = U, Error = E>>(&self, dial: &D, syn: Syn) -> JoinHandle<Result<U, E>> {
        let future = dial.dial(syn);
        let wake = self.wake.clone();
        tokio::spawn(async move {
            let result = future.await;
            wake.notify_one();
            result
        })
    }

    /// Hold each new SYN until its upstream is dialed. Then give the SYN to
    /// the stack, or answer it with a RST when the dial failed.
    async fn hold_syns<D: Dial<Upstream = U, Error = E>>(
        &mut self,
        from_payload: &mut VecDeque<Vec<u8>>,
        to_payload: &mut VecDeque<Vec<u8>>,
        dial: &D,
    ) {
        let mut kept = VecDeque::with_capacity(from_payload.len());
        while let Some(packet) = from_payload.pop_front() {
            let Some(syn) = Syn::of(&packet) else {
                kept.push_back(packet);
                continue;
            };
            if self.dialed.contains_key(&syn) || self.open.contains(&syn) {
                // A released SYN, or a SYN again for a known connection.
                kept.push_back(packet);
            } else if self.held.contains_key(&syn) {
                // A SYN again while the dial runs: the first one is held.
                self.stats.again += 1;
            } else if self.held.len() + self.dialed.len() + self.open.len() >= self.settings.max_flows {
                // No room: the payload sends the SYN again (call C91).
                self.stats.over_budget += 1;
            } else {
                let task = self.spawn_dial(dial, syn);
                self.held.insert(syn, Held { packet, task });
                self.stats.held += 1;
            }
        }
        *from_payload = kept;

        let done: Vec<Syn> =
            self.held.iter().filter(|(_, held)| held.task.is_finished()).map(|(syn, _)| *syn).collect();
        for syn in done {
            let Some(held) = self.held.remove(&syn) else { continue };
            match held.task.await.unwrap_or_else(|e| Err(E::from(e))) {
                Ok(upstream) => {
                    self.dialed.insert(syn, (upstream, Instant::now()));
                    from_payload.push_front(held.packet);
                    self.stats.released += 1;
                }
                Err(error) => {
                    if let Some(rst) = rst_reply_for_syn(&held.packet) {
                        to_payload.push_back(rst);
                    }
                    self.stats.reset += 1;
                    dial.refused(syn, Refused::Reset, error);
                }
            }
        }
        let (dialed_ttl, handshake_ttl) = (self.settings.dialed_ttl, self.settings.handshake_ttl);
        self.dialed.retain(|_, (_, at)| at.elapsed() < dialed_ttl);
        self.handshakes.retain(|_, at| at.elapsed() < handshake_ttl);
    }

    /// Give each packet of the payload to the stack. A new SYN needs a
    /// listener on its port. The stack adds a listen socket for each SYN, so
    /// a burst to one port gets no RST.
    async fn feed(&mut self, from_payload: &mut VecDeque<Vec<u8>>) {
        while let Some(packet) = from_payload.pop_front() {
            if let Some(syn) = Syn::of(&packet) {
                let known = self.handshakes.contains_key(&syn) || self.open.contains(&syn);
                if !known {
                    if !self.listen_on(syn.target.port()).await {
                        // The stack drops the SYN.
                        self.stats.no_listener += 1;
                        continue;
                    }
                    self.handshakes.insert(syn, Instant::now());
                }
            }
            // A full queue drops the packet: TCP sends it again.
            self.link_in.push(packet);
        }
    }
}

impl<U, E> SynHold<U, E> {
    /// Abort each task of the hold: the listeners and the dials.
    fn abort_tasks(&mut self) {
        for task in self.ports.values() {
            task.abort();
        }
        for held in self.held.values() {
            held.task.abort();
        }
        for late in &self.late {
            late.task.abort();
        }
    }
}

impl<U, E> Drop for SynHold<U, E> {
    /// No listener and no dial outlives the hold.
    fn drop(&mut self) {
        self.abort_tasks();
    }
}

/// A poisoned lock still holds a consistent table: each change under it is
/// one insert or one remove.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
