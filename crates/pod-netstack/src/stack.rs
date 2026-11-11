//! The userspace IP stack.
//!
//! A [`NetStack`] owns a private IPv4 and IPv6 address space and runs the
//! TCP and UDP state machines for it. Its link layer is an in-process packet
//! queue ([`crate::device::LinkDevice`]), so the stack makes no `AF_INET`
//! call of the kernel. Two modes:
//!
//! * **loopback**: the outbound path of the link feeds its own inbound
//!   queue, so a process can open a listener and a client to it in one
//!   stack. The tests use it.
//! * **over a link**: a pump of the consumer moves the packets between
//!   [`NetStack::link_in`] and [`NetStack::link_out`] and a real byte
//!   stream: a unix socket, a QUIC connection, a WebSocket.
//!
//! The clock is monotonic and real (`std::time::Instant`). The reactor waits
//! for the waker of the device, a command or the next deadline of the
//! interface; it does not spin.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use atomic_waker::AtomicWaker;
use smoltcp::iface::{Config as IfaceConfig, Interface, SocketHandle, SocketSet};
use smoltcp::socket::{tcp, udp};
use smoltcp::time::Instant as SmolInstant;
use smoltcp::wire::{HardwareAddress, IpAddress, IpCidr, IpEndpoint, IpListenEndpoint};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{Notify, mpsc, oneshot};

use crate::device::{DeviceStats, DeviceStatsSnapshot, LinkDevice, PacketQueue};
use crate::hooks::{Admit, GATEWAY_DATAGRAM, GatewayService};
use crate::subnet::Subnet;

/// A monotonic clock in milliseconds for smoltcp.
#[derive(Clone, Debug)]
pub struct Clock {
    start: std::time::Instant,
    base_ms: i64,
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock {
    pub fn new() -> Self {
        Self { start: std::time::Instant::now(), base_ms: 0 }
    }

    /// A clock whose first reading is `base_ms`, for tests.
    pub fn starting_at(base_ms: i64) -> Self {
        Self { start: std::time::Instant::now(), base_ms }
    }

    pub fn now_ms(&self) -> i64 {
        self.base_ms + self.start.elapsed().as_millis() as i64
    }

    pub fn now(&self) -> SmolInstant {
        SmolInstant::from_millis(self.now_ms())
    }
}

/// The settings of a stack.
#[derive(Clone)]
pub struct StackConfig {
    pub subnet: Subnet,
    /// The address that this stack owns (and answers for) in the IPv4 space.
    pub local_v4: Ipv4Addr,
    /// The address that this stack owns in the IPv6 space.
    pub local_v6: Ipv6Addr,
    pub mtu: usize,
    /// The bytes of the send buffer and of the receive buffer of each TCP
    /// socket.
    pub tcp_buffer: usize,
    pub udp_packets: usize,
    pub udp_payload: usize,
    /// A listener keeps at most this many listening sockets: the limit of
    /// the half-open inbound connections at one time.
    pub accept_pool: usize,
    pub max_connections: usize,
    /// The admit of each target, at the bind, the connect and each UDP send.
    pub admit: Option<Arc<dyn Admit>>,
    /// A UDP service at the IPv4 gateway. When set, the stack owns the
    /// gateway address and gives each datagram to the service.
    pub gateway_service: Option<Arc<dyn GatewayService>>,
    /// The seed of smoltcp for its ports and sequence numbers.
    pub random_seed: u64,
    /// The longest sleep of the reactor between two polls.
    pub max_poll_interval: Duration,
}

impl Default for StackConfig {
    /// The default subnet, `10.66.0.2` and `fd00:66::2`, an MTU of 1280,
    /// buffers of 64 KiB, no admit and no gateway service.
    fn default() -> Self {
        Self {
            subnet: Subnet::default(),
            local_v4: Ipv4Addr::new(10, 66, 0, 2),
            local_v6: Ipv6Addr::new(0xfd00, 0x66, 0, 0, 0, 0, 0, 2),
            mtu: 1280,
            tcp_buffer: 64 * 1024,
            udp_packets: 64,
            udp_payload: 64 * 1024,
            accept_pool: 8,
            max_connections: 4096,
            admit: None,
            gateway_service: None,
            random_seed: seed_from_time(),
            max_poll_interval: Duration::from_millis(100),
        }
    }
}

impl StackConfig {
    /// True when the admit of the consumer (if any) admits `target`.
    fn admits(&self, target: SocketAddr) -> bool {
        self.admit.as_ref().is_none_or(|admit| admit.admits(target))
    }
}

fn seed_from_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15)
}

/// An event of the stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackEvent {
    ListenerBound { addr: SocketAddr },
    Accepted { id: u32, local: SocketAddr, remote: SocketAddr },
    Connected { id: u32, local: SocketAddr, remote: SocketAddr },
    Refused { remote: SocketAddr, reason: String },
    Closed { id: u32 },
    Datagram { from: SocketAddr, to: SocketAddr, len: usize },
}

/// The counters of the connections, readable while the reactor runs.
#[derive(Debug, Default)]
pub struct NetStats {
    pub accepted: AtomicU64,
    pub connected: AtomicU64,
    pub refused: AtomicU64,
    pub closed: AtomicU64,
    pub datagrams: AtomicU64,
    /// The inbound connections that the accept queue could not take. A drop
    /// is no acceptance: an operator who reads `accepted` sees fewer
    /// connections than the peer made, and this counter tells why.
    pub dropped: AtomicU64,
}

/// The values of [`NetStats`] at one time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetStatsSnapshot {
    pub accepted: u64,
    pub connected: u64,
    pub refused: u64,
    pub closed: u64,
    pub datagrams: u64,
    pub dropped: u64,
}

impl NetStats {
    fn snapshot(&self) -> NetStatsSnapshot {
        NetStatsSnapshot {
            accepted: self.accepted.load(Ordering::Relaxed),
            connected: self.connected.load(Ordering::Relaxed),
            refused: self.refused.load(Ordering::Relaxed),
            closed: self.closed.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            datagrams: self.datagrams.load(Ordering::Relaxed),
        }
    }
}

fn to_smoltcp(ip: IpAddr) -> IpAddress {
    IpAddress::from(ip)
}

fn endpoint_to_socket(endpoint: IpEndpoint) -> SocketAddr {
    SocketAddr::from(endpoint)
}

fn shut_down() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "the stack is shut down")
}

mod datagrams;
mod reactor;
mod service;
mod socket;
pub use datagrams::UdpDatagrams;
use reactor::*;
use socket::*;
pub use socket::{SocketCount, TcpListener, TcpStream, UdpSocket};

/// The handle of the reactor task of a stack.
pub struct NetStack {
    cmd: mpsc::Sender<Command>,
    link_in: PacketQueue,
    link_out: PacketQueue,
    stats: Arc<NetStats>,
    device_stats: Arc<DeviceStats>,
    socket_count: Arc<SocketCount>,
    shutdown: Arc<Notify>,
    kick: Arc<Notify>,
    events_rx: Option<mpsc::UnboundedReceiver<StackEvent>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl NetStack {
    /// A stack whose link feeds itself, with the default settings. It spawns
    /// its reactor: call it in a runtime of tokio.
    pub fn loopback() -> Self {
        Self::new(StackConfig::default(), true)
    }

    /// A stack whose link feeds itself.
    pub fn loopback_with(config: StackConfig) -> Self {
        Self::new(config, true)
    }

    /// A stack whose packets leave and come in through a link that a pump of
    /// the consumer moves ([`NetStack::link_in`], [`NetStack::link_out`]).
    pub fn over_link(config: StackConfig) -> Self {
        Self::new(config, false)
    }

    fn new(config: StackConfig, loopback: bool) -> Self {
        let device = if loopback { LinkDevice::loopback(config.mtu) } else { LinkDevice::link(config.mtu) };
        // A packet to an address of this stack stays in it
        // (PodUtils/podnet#65).
        device.add_local(IpAddr::V4(config.local_v4));
        device.add_local(IpAddr::V6(config.local_v6));
        device.add_local(IpAddr::V4(config.subnet.gateway_v4()));
        device.add_local(IpAddr::V6(config.subnet.gateway_v6()));
        let link_in = device.inbound();
        let link_out = device.outbound();
        let device_stats = device.stats();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let kick = Arc::new(Notify::new());
        let max_poll_interval = config.max_poll_interval;
        let socket_count = Arc::new(SocketCount::default());
        let core = Core::new(config, device, events_tx, kick.clone(), socket_count.clone());
        let stats = core.stats.clone();
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let shutdown = Arc::new(Notify::new());
        let inbound_wait = link_in.clone();
        let shutdown_flag = shutdown.clone();
        let run_kick = kick.clone();
        let task = tokio::spawn(async move {
            run(core, cmd_rx, inbound_wait, shutdown_flag, run_kick, max_poll_interval).await;
        });
        Self {
            cmd: cmd_tx,
            link_in,
            link_out,
            stats,
            device_stats,
            socket_count,
            shutdown,
            kick,
            events_rx: Some(events_rx),
            task: Some(task),
        }
    }

    async fn send(&self, command: Command) -> io::Result<()> {
        self.cmd.send(command).await.map_err(|_| shut_down())
    }

    /// Bind a TCP listener.
    pub async fn listen(&self, addr: impl Into<SocketAddr>) -> io::Result<TcpListener> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Listen { addr: addr.into(), reply }).await?;
        rx.await.map_err(|_| shut_down())?
    }

    /// Open a TCP connection from an ephemeral port.
    pub async fn connect(&self, remote: impl Into<SocketAddr>) -> io::Result<TcpStream> {
        self.connect_from(remote, 0).await
    }

    /// Open a TCP connection from `local_port` (0: an ephemeral port).
    pub async fn connect_from(&self, remote: impl Into<SocketAddr>, local_port: u16) -> io::Result<TcpStream> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Connect { remote: remote.into(), local_port, reply }).await?;
        rx.await.map_err(|_| shut_down())?
    }

    /// Bind a UDP socket.
    pub async fn bind_udp(&self, addr: impl Into<SocketAddr>) -> io::Result<UdpSocket> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::BindUdp { addr: addr.into(), reply }).await?;
        rx.await.map_err(|_| shut_down())?
    }

    /// The packets that come from the external link (the pump writes, the
    /// stack reads).
    pub fn link_in(&self) -> PacketQueue {
        self.link_in.clone()
    }

    /// The packets that leave the stack (the pump reads and writes them to
    /// the link).
    pub fn link_out(&self) -> PacketQueue {
        self.link_out.clone()
    }

    pub fn stats(&self) -> NetStatsSnapshot {
        self.stats.snapshot()
    }

    pub fn device_stats(&self) -> DeviceStatsSnapshot {
        self.device_stats.snapshot()
    }

    /// The sockets that the reactor holds now, and the most that it held at
    /// one time.
    ///
    /// A `SocketSet` uses a free slot again on `add` but frees no slot
    /// itself, so a set that the reactor does not prune at the close of a
    /// connection grows with no limit. With pruning, `live` goes back to its
    /// resting value after the connections close; with none, it climbs.
    pub fn socket_count(&self) -> (u64, u64) {
        (self.socket_count.live(), self.socket_count.peak())
    }

    /// Take the stream of events, at most once.
    pub fn take_events(&mut self) -> Option<mpsc::UnboundedReceiver<StackEvent>> {
        self.events_rx.take()
    }

    /// Stop the reactor, and wait at most 2 s for its task.
    pub async fn shutdown(&mut self) {
        let _ = self.cmd.try_send(Command::Shutdown);
        self.shutdown.notify_one();
        self.kick.notify_one();
        if let Some(task) = self.task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
        }
    }
}

impl Drop for NetStack {
    fn drop(&mut self) {
        // `notify_one`, not `notify_waiters`: it stores a permit, so a notify
        // while the reactor is between two polls is not lost.
        self.shutdown.notify_one();
        self.kick.notify_one();
    }
}

async fn run(
    mut core: Core,
    mut cmd_rx: mpsc::Receiver<Command>,
    inbound_wait: PacketQueue,
    shutdown: Arc<Notify>,
    kick: Arc<Notify>,
    max_poll_interval: Duration,
) {
    loop {
        core.poll_iface();
        core.service();
        core.poll_iface();

        // `next_delay` clamps to the configured maximum already; a second
        // fixed clamp would make each larger `max_poll_interval` do nothing.
        // The floor only keeps a configuration of zero from a busy spin.
        let floor = Duration::from_micros(200).min(max_poll_interval);
        let delay = core.next_delay().unwrap_or(max_poll_interval).max(floor);

        tokio::select! {
            biased;
            command = cmd_rx.recv() => {
                match command {
                    Some(Command::Shutdown) | None => break,
                    // The command can make a socket; the loop polls at once.
                    Some(command) => core.handle(command),
                }
            }
            _ = kick.notified() => {}
            _ = inbound_wait.wait() => {}
            _ = tokio::time::sleep(delay) => {}
            _ = shutdown.notified() => break,
        }
    }
    // Wake each task that still waits on a channel.
    core.conns.clear();
    core.udp.clear();
}

#[cfg(test)]
mod tests;
