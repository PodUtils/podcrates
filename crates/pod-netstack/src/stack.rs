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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
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

mod config;
mod datagrams;
mod listen;
mod reactor;
mod reset;
mod service;
mod socket;
mod stats;
mod udp_service;
pub use config::StackConfig;
pub use datagrams::UdpDatagrams;
use listen::*;
use reactor::*;
pub use reset::StackReset;
use reset::*;
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
    halt: Arc<Halt>,
    reset: StackReset,
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
        let halt = core.halt.clone();
        let reset = StackReset { slot: core.reset_slot.clone(), kick: kick.clone() };
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
            halt,
            reset,
            events_rx: Some(events_rx),
            task: Some(task),
        }
    }

    async fn send(&self, command: Command) -> io::Result<()> {
        self.cmd.send(command).await.map_err(|_| self.shut_down())
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
        rx.await.map_err(|_| self.shut_down())?
    }

    /// Open a TCP connection from an ephemeral port.
    pub async fn connect(&self, remote: impl Into<SocketAddr>) -> io::Result<TcpStream> {
        self.connect_from(remote, 0).await
    }

    /// Open a TCP connection from `local_port` (0: an ephemeral port).
    pub async fn connect_from(&self, remote: impl Into<SocketAddr>, local_port: u16) -> io::Result<TcpStream> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Connect { remote: remote.into(), local_port, reply }).await?;
        rx.await.map_err(|_| self.shut_down())?
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
        rx.await.map_err(|_| self.shut_down())?
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
        core.take_reset();
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
                    // A reset that the consumer asked for before the command
                    // goes first: it does not end the new socket.
                    Some(command) => {
                        core.take_reset();
                        core.handle(command);
                    }
                }
            }
            _ = kick.notified() => {}
            _ = inbound_wait.wait() => {}
            _ = tokio::time::sleep(delay) => {}
            _ = shutdown.notified() => break,
        }
    }
    // The drop of the core gives the cause to each socket, then wakes each
    // task that still waits on a channel.
    core.halt.mark("the stack is shut down");
}

#[cfg(test)]
mod tests;
