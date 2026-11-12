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

use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use atomic_waker::AtomicWaker;
use smoltcp::iface::{Config as IfaceConfig, Interface, PollIngressSingleResult, PollResult, SocketHandle, SocketSet};
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::socket::{tcp, udp};
use smoltcp::time::{Duration as SmolDuration, Instant as SmolInstant};
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
    /// The datagrams and the bytes of each buffer of each UDP socket. The
    /// datagrams that wait for a slow app: 64 in the channel to the app,
    /// then at most `udp_packets` datagrams and `udp_payload` bytes in the
    /// backlog of the socket. The stack counts each datagram past them in
    /// `NetStats::udp_rx_dropped`.
    pub udp_packets: usize,
    pub udp_payload: usize,
    /// The TCP connections of the stack at one time, open and half-open. A
    /// connect past it is `WouldBlock`. A SYN past it waits: the stack drops
    /// it, and the peer sends it again (no RST). Below it, a listener adds a
    /// listen socket for each SYN, so a burst to one port gets no RST.
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
    /// Take each packet, also to an address that the stack does not own
    /// (`Interface::set_any_ip`): a router that answers for each
    /// destination of a program. A listener and a UDP socket on any
    /// address ([`NetStack::listen_any`], [`NetStack::bind_udp_any`]) then
    /// take each destination.
    pub any_ip: bool,
    /// The checksums that the stack computes and verifies
    /// ([`LinkDevice::with_checksum`]). The default computes and verifies
    /// each.
    pub checksum: ChecksumCapabilities,
    /// The longest time that a dropped or closed stream waits for its peer
    /// with no progress: no change of its state and of its send queue. Then
    /// the stack aborts it (a RST) and frees its socket, so that a dead peer,
    /// or a peer that never sends its FIN, does not hold the socket for
    /// ever. 60 s: the FIN-WAIT-2 time of Linux (`tcp_fin_timeout`).
    pub linger: Duration,
    /// The congestion controller of each TCP socket. None by default, as
    /// smoltcp before 0.12: the link holds the stack when its queue is full
    /// (PodUtils/podnet#134). Reno and CUBIC grow by one segment for each
    /// ACK in slow start (the section "The congestion controllers", T-197).
    pub congestion_control: tcp::CongestionControl,
    /// The burst limit of the link in segments; smoltcp also clamps the TCP
    /// window that the stack advertises to it (PodUtils/podnet#100). None:
    /// no limit.
    pub max_burst: Option<usize>,
    /// The counters of the link, shared with other stacks of the consumer
    /// (podnet adds the links of each WARP session of a process). None: the
    /// counters of this stack only.
    pub link_stats: Option<Arc<DeviceStats>>,
}

impl Default for StackConfig {
    /// The default subnet, `10.66.0.2` and `fd00:66::2`, an MTU of 1280,
    /// buffers of 64 KiB, no admit, no gateway service, no any IP, each
    /// checksum computed and verified, a linger of 60 s, no congestion
    /// controller, no burst limit and counters of its own.
    fn default() -> Self {
        Self {
            subnet: Subnet::default(),
            local_v4: Ipv4Addr::new(10, 66, 0, 2),
            local_v6: Ipv6Addr::new(0xfd00, 0x66, 0, 0, 0, 0, 0, 2),
            mtu: 1280,
            tcp_buffer: 64 * 1024,
            udp_packets: 64,
            udp_payload: 64 * 1024,
            max_connections: 4096,
            admit: None,
            gateway_service: None,
            random_seed: seed_from_time(),
            max_poll_interval: Duration::from_millis(100),
            any_ip: false,
            checksum: ChecksumCapabilities::default(),
            linger: Duration::from_secs(60),
            congestion_control: tcp::CongestionControl::None,
            max_burst: None,
            link_stats: None,
        }
    }
}

impl StackConfig {
    /// True when the admit of the consumer (if any) admits `target`.
    fn admits(&self, target: SocketAddr) -> bool {
        self.admit.as_ref().is_none_or(|admit| admit.admits(target))
    }

    /// The largest UDP payload that a stack with these settings sends to
    /// `to`; a larger send is `InvalidInput`. IPv6: `mtu - 48` (smoltcp does
    /// not fragment IPv6). IPv4: `mtu - 28`, or 65507 with the feature
    /// `ipv4-fragmentation`. Each at most `udp_payload`.
    pub fn max_datagram(&self, to: IpAddr) -> usize {
        Limits::of(self).to(to)
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
mod listen;
mod reactor;
mod service;
mod socket;
mod stats;
mod udp_service;
pub use datagrams::UdpDatagrams;
use listen::*;
use reactor::*;
use socket::*;
pub use socket::{SocketCount, TcpListener, TcpStream, UdpSocket};
pub use stats::{NetStats, NetStatsSnapshot};
use udp_service::*;

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
        let device = link_device(&config, loopback);
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
        self.listen_on(Local::of(addr.into())).await
    }

    /// Bind a TCP listener on `port` of each address, of both families. With
    /// [`StackConfig::any_ip`], it takes a connection to each destination,
    /// and the `local_addr` of each accepted stream is the destination that
    /// the peer dialed. Its own `local_addr` is `[::]:port`. A listener on
    /// one address of the same port is `AddrInUse`, both ways: smoltcp gives
    /// a SYN to the first socket of its set, so the two would race.
    pub async fn listen_any(&self, port: u16) -> io::Result<TcpListener> {
        self.listen_on(Local { ip: None, port }).await
    }

    async fn listen_on(&self, local: Local) -> io::Result<TcpListener> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Listen { local, reply }).await?;
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
        self.bind_udp_on(Local::of(addr.into())).await
    }

    /// Bind a UDP socket on `port` of each address, of both families. With
    /// [`StackConfig::any_ip`], it takes a datagram to each destination
    /// ([`UdpSocket::recv_from_to`] gives it), and
    /// [`UdpSocket::send_to_from`] answers from that destination. Its
    /// `local_addr` is `[::]:port`. A socket on one address of the same
    /// port is `AddrInUse`, both ways; the gateway service keeps its
    /// address, and the socket takes the other destinations of the port.
    pub async fn bind_udp_any(&self, port: u16) -> io::Result<UdpSocket> {
        self.bind_udp_on(Local { ip: None, port }).await
    }

    async fn bind_udp_on(&self, local: Local) -> io::Result<UdpSocket> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::BindUdp { local, reply }).await?;
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

/// The device of a stack with the settings of `config`: its checksums, its
/// burst limit and its counters.
fn link_device(config: &StackConfig, loopback: bool) -> LinkDevice {
    let device = if loopback { LinkDevice::loopback(config.mtu) } else { LinkDevice::link(config.mtu) };
    let device = device.with_checksum(config.checksum.clone()).with_max_burst(config.max_burst);
    match &config.link_stats {
        Some(stats) => device.with_stats(stats.clone()),
        None => device,
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
