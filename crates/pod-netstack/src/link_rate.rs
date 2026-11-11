//! The rate of each congestion controller of smoltcp 0.14 over a delay link.
//!
//! Two interfaces of smoltcp over two [`LinkDevice::link`] move a download
//! of 8 MiB on a virtual clock: the measurement reads no wall time, and each
//! run gives the same numbers. The link holds each packet 20 ms in each
//! direction; the buffers are 512 KiB. Two kinds of link:
//!
//! * [`LinkKind::Burst`]: each packet that one poll sends arrives at one time
//!   (podnet's `tests/tcp_window.rs`). The receiver takes the burst in one
//!   poll and gives one ACK for it.
//! * [`LinkKind::Serial`]: the packets go out one by one at 100 Mbit/s, then
//!   take the delay. The receiver takes each packet in its own poll.

use std::collections::VecDeque;

use smoltcp::iface::{Config, Interface, PollIngressSingleResult, SocketHandle, SocketSet};
use smoltcp::socket::tcp::{self, CongestionControl};
use smoltcp::time::{Duration, Instant};
use smoltcp::wire::{HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address};

use crate::device::{LinkDevice, PacketQueue};

const MTU: usize = 1280;
/// The TCP segment: the MTU less 20 bytes of IPv4 and 20 of TCP.
const MSS: usize = MTU - 40;
const BUFFER: usize = 512 * 1024;
const ONE_WAY_US: u64 = 20_000;
const ROUND_TRIP_S: f64 = (2 * ONE_WAY_US) as f64 / 1e6;
const TOTAL: usize = 8 << 20;
/// The rate of the serial link in bytes for each second (100 Mbit/s).
const SERIAL_RATE: u64 = 12_500_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkKind {
    Burst,
    Serial,
}

/// How the receiver takes the packets that wait in its device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ingress {
    /// `Interface::poll`: each waiting packet, then one pass of egress.
    Poll,
    /// `poll_ingress_single` and a pass of egress after each packet.
    EachPacket,
}

/// One measurement.
#[derive(Clone, Copy, Debug)]
struct Case {
    link: LinkKind,
    /// The controller of both sockets; only the server sends data.
    controller: CongestionControl,
    /// The delayed ACK of the receiver (smoltcp default: 10 ms).
    ack_delay: Option<Duration>,
    ingress: Ingress,
}

impl Case {
    /// The receiver of smoltcp with its defaults.
    fn new(link: LinkKind, controller: CongestionControl) -> Self {
        Self { link, controller, ack_delay: Some(Duration::from_millis(10)), ingress: Ingress::Poll }
    }
}

/// The result of one measurement.
#[derive(Clone, Copy, Debug)]
struct Measure {
    /// MB/s of virtual time, from the first byte that the server queues to
    /// the last byte that the client reads.
    rate: f64,
    /// MB/s of the second half of the download: the rate after most of the
    /// slow start.
    half_rate: f64,
    /// The seconds of virtual time of the download.
    seconds: f64,
    /// The packets that the server sent (the data, the SYN-ACK).
    server_packets: u64,
    /// The packets that the client sent (the SYN, the ACKs).
    client_packets: u64,
}

/// The highest rate of the link in MB/s: the window over the round trip,
/// and the rate of the serial link.
fn limit(link: LinkKind) -> f64 {
    let window = BUFFER as f64 / ROUND_TRIP_S / 1e6;
    match link {
        LinkKind::Burst => window,
        LinkKind::Serial => window.min(SERIAL_RATE as f64 / 1e6),
    }
}

/// The data segments of the download when no segment is sent again.
fn data_segments() -> u64 {
    TOTAL.div_ceil(MSS) as u64
}

/// One direction of the link: the packets in flight with their time of
/// arrival, and the time at which a serial link is free again.
struct Line {
    link: LinkKind,
    packets: VecDeque<(u64, Vec<u8>)>,
    /// In nanoseconds: a packet takes 102.4 us on the serial link.
    free_at_ns: u64,
}

impl Line {
    fn new(link: LinkKind) -> Self {
        Self { link, packets: VecDeque::new(), free_at_ns: 0 }
    }

    /// Take each packet that `from` sent at `now`.
    fn take(&mut self, from: &PacketQueue, now: u64) {
        while let Some(packet) = from.pop() {
            let at = match self.link {
                LinkKind::Burst => now + ONE_WAY_US,
                LinkKind::Serial => {
                    let start = self.free_at_ns.max(now * 1000);
                    self.free_at_ns = start + packet.len() as u64 * 1_000_000_000 / SERIAL_RATE;
                    self.free_at_ns.div_ceil(1000) + ONE_WAY_US
                }
            };
            self.packets.push_back((at, packet));
        }
    }

    /// Give each packet that arrived at `now` to `to`.
    fn deliver(&mut self, to: &PacketQueue, now: u64) {
        while self.packets.front().is_some_and(|(at, _)| *at <= now) {
            let (_, packet) = self.packets.pop_front().expect("a packet at the front");
            assert!(to.push(packet), "the inbound queue is full");
        }
    }

    fn next(&self) -> Option<u64> {
        self.packets.front().map(|(at, _)| *at)
    }
}

struct Node {
    device: LinkDevice,
    iface: Interface,
    sockets: SocketSet<'static>,
    handle: SocketHandle,
    ingress: Ingress,
}

impl Node {
    fn new(addr: Ipv4Address, controller: CongestionControl, ingress: Ingress) -> Self {
        let mut device = LinkDevice::link(MTU);
        let mut config = Config::new(HardwareAddress::Ip);
        config.random_seed = 7;
        let mut iface = Interface::new(config, &mut device, Instant::ZERO);
        iface.update_ip_addrs(|addrs| {
            addrs.push(IpCidr::new(IpAddress::Ipv4(addr), 24)).expect("room for an address");
        });
        let mut socket =
            tcp::Socket::new(tcp::SocketBuffer::new(vec![0; BUFFER]), tcp::SocketBuffer::new(vec![0; BUFFER]));
        socket.set_congestion_control(controller);
        let mut sockets = SocketSet::new(Vec::new());
        let handle = sockets.add(socket);
        Self { device, iface, sockets, handle, ingress }
    }

    fn socket(&mut self) -> &mut tcp::Socket<'static> {
        self.sockets.get_mut::<tcp::Socket>(self.handle)
    }

    fn poll(&mut self, now: u64) {
        let ts = Instant::from_micros(now as i64);
        match self.ingress {
            Ingress::Poll => {
                self.iface.poll(ts, &mut self.device, &mut self.sockets);
            }
            Ingress::EachPacket => {
                self.iface.poll_maintenance(ts);
                while self.iface.poll_ingress_single(ts, &mut self.device, &mut self.sockets)
                    != PollIngressSingleResult::None
                {
                    self.iface.poll_egress(ts, &mut self.device, &mut self.sockets);
                }
                self.iface.poll_egress(ts, &mut self.device, &mut self.sockets);
            }
        }
    }

    /// The next time at which the interface wants a poll, after `now`.
    fn poll_at(&mut self, now: u64) -> Option<u64> {
        let at = self.iface.poll_at(Instant::from_micros(now as i64), &self.sockets)?;
        Some((at.total_micros().max(0) as u64).max(now + 1))
    }
}

/// Download [`TOTAL`] bytes from a server to a client over the link of
/// `case`, and measure it.
fn measure(case: Case) -> Measure {
    let (server_addr, client_addr) = (Ipv4Address::new(10, 66, 0, 2), Ipv4Address::new(10, 66, 0, 3));
    let mut server = Node::new(server_addr, case.controller, Ingress::Poll);
    let mut client = Node::new(client_addr, case.controller, case.ingress);
    server.socket().listen(80).expect("listen");
    client.socket().set_ack_delay(case.ack_delay);
    let cx = client.iface.context();
    let remote = IpEndpoint::new(IpAddress::Ipv4(server_addr), 80);
    let local = (IpAddress::Ipv4(client_addr), 40_000);
    client.sockets.get_mut::<tcp::Socket>(client.handle).connect(cx, remote, local).expect("connect");
    let (mut to_server, mut to_client) = (Line::new(case.link), Line::new(case.link));
    let chunk = vec![0xab_u8; 64 * 1024];
    let mut buf = vec![0_u8; 64 * 1024];
    let (mut sent, mut got) = (0_usize, 0_usize);
    let (mut started, mut half) = (None, None);
    let mut now = 0_u64;
    while got < TOTAL {
        // 120 s of virtual time: far above the slowest case, so only a stall
        // reaches it.
        assert!(now < 120_000_000, "{case:?}: the download stalled at {got} bytes");
        loop {
            to_client.deliver(&client.device.inbound(), now);
            to_server.deliver(&server.device.inbound(), now);
            client.poll(now);
            server.poll(now);
            to_server.take(&client.device.outbound(), now);
            to_client.take(&server.device.outbound(), now);
            let mut moved = false;
            let socket = server.socket();
            if socket.can_send() && sent < TOTAL {
                let n = socket.send_slice(&chunk[..chunk.len().min(TOTAL - sent)]).expect("send");
                sent += n;
                moved |= n > 0;
                started.get_or_insert(now);
            }
            let socket = client.socket();
            if socket.can_recv() {
                let n = socket.recv_slice(&mut buf).expect("recv");
                got += n;
                moved |= n > 0;
            }
            if !moved {
                break;
            }
        }
        if got >= TOTAL / 2 {
            half.get_or_insert((now, got));
        }
        // The download ends at the last read, not at the next event.
        if got >= TOTAL {
            break;
        }
        let next =
            [client.poll_at(now), server.poll_at(now), to_server.next(), to_client.next()].into_iter().flatten().min();
        now = next.unwrap_or_else(|| panic!("{case:?}: no event at {got} bytes")).max(now + 1);
    }
    let seconds = (now - started.expect("a start")) as f64 / 1e6;
    let (half_at, half_got) = half.expect("a half");
    Measure {
        rate: got as f64 / seconds / 1e6,
        half_rate: (got - half_got) as f64 / ((now - half_at) as f64 / 1e6) / 1e6,
        seconds,
        server_packets: server.device.stats().snapshot().tx_packets,
        client_packets: client.device.stats().snapshot().tx_packets,
    }
}

mod tests;
