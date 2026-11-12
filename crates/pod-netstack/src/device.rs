//! The link devices of the stack.
//!
//! The link layer of the stack is a pair of in-process packet queues: the
//! packets that the stack sends go into the outbound queue, and the packets
//! from the outside go into the inbound queue. A pump (a task of the
//! consumer) moves them between the queues and the real link: a unix
//! socket, a QUIC stream, a WebSocket.
//!
//! The smoltcp `Device` stays in the process, so it is not generic over an
//! async transport, and one device serves the loopback (the outbound path
//! feeds the inbound queue), a tunnel and a capture ([`RecordingDevice`]).

use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use smoltcp::phy::{ChecksumCapabilities, Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;

mod queue;
mod tap;
pub use queue::PacketQueue;
pub use tap::{Direction, PacketTap, RecordRx, RecordTx, RecordingDevice};

/// The counters of a link, shared by each clone.
#[derive(Debug, Default)]
pub struct DeviceStats {
    pub rx_packets: AtomicU64,
    pub tx_packets: AtomicU64,
    pub rx_bytes: AtomicU64,
    pub tx_bytes: AtomicU64,
    pub rx_dropped: AtomicU64,
    pub tx_dropped: AtomicU64,
    /// The times that the stack kept its packets because the outbound queue
    /// was full (PodUtils/podnet#134).
    pub tx_held: AtomicU64,
}

/// The values of [`DeviceStats`] at one time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceStatsSnapshot {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
    pub tx_held: u64,
}

impl DeviceStats {
    pub fn snapshot(&self) -> DeviceStatsSnapshot {
        DeviceStatsSnapshot {
            rx_packets: self.rx_packets.load(Ordering::Relaxed),
            tx_packets: self.tx_packets.load(Ordering::Relaxed),
            rx_bytes: self.rx_bytes.load(Ordering::Relaxed),
            tx_bytes: self.tx_bytes.load(Ordering::Relaxed),
            rx_dropped: self.rx_dropped.load(Ordering::Relaxed),
            tx_dropped: self.tx_dropped.load(Ordering::Relaxed),
            tx_held: self.tx_held.load(Ordering::Relaxed),
        }
    }
}

/// The packets that the outbound queue of a link device holds.
pub const LINK_OUTBOUND: usize = 512;
/// The packets that the inbound queue of a link device holds.
pub const LINK_INBOUND: usize = 4096;

/// The link device of the stack: an inbound queue, an outbound queue, and
/// the loopback as an option.
pub struct LinkDevice {
    rx: PacketQueue,
    tx: PacketQueue,
    stats: Arc<DeviceStats>,
    mtu: usize,
    medium: Medium,
    loopback: bool,
    /// The addresses of this stack: a packet to one of them goes back to the
    /// inbound queue, not to the link (a switch drops a packet that comes
    /// back to its sender).
    local: Arc<RwLock<Vec<IpAddr>>>,
    /// The burst limit of smoltcp. smoltcp also clamps the TCP window that
    /// the stack advertises to this many segments (PodUtils/podnet#100).
    /// None: no limit.
    max_burst: Option<usize>,
    /// Send no new packet while the outbound queue is full
    /// (PodUtils/podnet#134).
    hold_when_full: bool,
    /// The checksums that smoltcp computes and verifies.
    checksum: ChecksumCapabilities,
}

impl std::fmt::Debug for LinkDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkDevice")
            .field("rx", &self.rx)
            .field("tx", &self.tx)
            .field("mtu", &self.mtu)
            .field("loopback", &self.loopback)
            .finish()
    }
}

impl LinkDevice {
    /// A device whose outbound path feeds its own inbound queue: a stack
    /// that talks only to itself (a proof, a test, a network in one process).
    pub fn loopback(mtu: usize) -> Self {
        Self::new(mtu, true)
    }

    /// A device that a pump connects to an external link.
    pub fn tunnel(mtu: usize) -> Self {
        Self::new(mtu, false)
    }

    /// A tunnel device whose outbound queue holds at most `packets`; a full
    /// queue drops a packet, as the queue of a router does.
    ///
    /// While the queue is full, the stack sends no new packet (the data stays
    /// in its sockets), so the queue drops only the packets of a receive (the
    /// acknowledgements) (PodUtils/podnet#134).
    pub fn tunnel_bounded(mtu: usize, packets: usize) -> Self {
        let mut device = Self::new(mtu, false);
        device.tx = PacketQueue::bounded(packets);
        device.hold_when_full = true;
        device
    }

    /// The device of a stack over a link: both queues are bounded
    /// ([`LINK_OUTBOUND`], [`LINK_INBOUND`]). The stack holds while the
    /// outbound queue is full. A packet that the pump cannot put in the
    /// inbound queue is dropped and counted in `rx_dropped`.
    pub fn link(mtu: usize) -> Self {
        let mut device = Self::tunnel_bounded(mtu, LINK_OUTBOUND).with_inbound_limit(LINK_INBOUND);
        device.rx = device.rx.clone().counting_in(device.stats.clone());
        device
    }

    /// The inbound queue holds at most `packets` (PodUtils/podnet#278): a
    /// peer that sends faster than the stack reads loses packets (TCP sends
    /// them again), and the heap does not grow without a limit.
    pub fn with_inbound_limit(mut self, packets: usize) -> Self {
        self.rx = PacketQueue::bounded(packets);
        self
    }

    /// Count in `stats`, shared with other devices, not in counters of this
    /// device only.
    pub fn with_stats(mut self, stats: Arc<DeviceStats>) -> Self {
        self.stats = stats;
        self
    }

    fn new(mtu: usize, loopback: bool) -> Self {
        Self {
            rx: PacketQueue::unbounded(),
            tx: PacketQueue::unbounded(),
            stats: Arc::new(DeviceStats::default()),
            // 576: the minimum of IPv4 (a carried stack has 1200,
            // PodUtils/podnet#154).
            mtu: mtu.max(576),
            medium: Medium::Ip,
            loopback,
            local: Arc::new(RwLock::new(Vec::new())),
            max_burst: None,
            hold_when_full: false,
            checksum: ChecksumCapabilities::default(),
        }
    }

    /// Set the burst limit (and so the TCP window) in segments.
    pub fn with_max_burst(mut self, max_burst: Option<usize>) -> Self {
        self.max_burst = max_burst;
        self
    }

    /// Set the checksums that smoltcp computes and verifies. A link that
    /// carries partial checksums (the end of a veth pair with its offload
    /// on) needs `Checksum::Tx`: compute each sum, verify none.
    pub fn with_checksum(mut self, checksum: ChecksumCapabilities) -> Self {
        self.checksum = checksum;
        self
    }

    /// Add an address of this stack (see `local`).
    pub fn add_local(&self, ip: IpAddr) {
        if let Ok(mut local) = self.local.write()
            && !local.contains(&ip)
        {
            local.push(ip);
        }
    }

    /// Remove an address of this stack: a packet to it goes to the link
    /// again.
    pub fn remove_local(&self, ip: IpAddr) {
        if let Ok(mut local) = self.local.write() {
            local.retain(|own| *own != ip);
        }
    }

    pub fn inbound(&self) -> PacketQueue {
        self.rx.clone()
    }

    pub fn outbound(&self) -> PacketQueue {
        self.tx.clone()
    }

    pub fn stats(&self) -> Arc<DeviceStats> {
        self.stats.clone()
    }

    pub fn mtu(&self) -> usize {
        self.mtu
    }

    pub fn is_loopback(&self) -> bool {
        self.loopback
    }

    fn tx_token(&mut self) -> LinkTx {
        LinkTx {
            tx: self.tx.clone(),
            rx: self.loopback.then(|| self.rx.clone()),
            inbound: self.rx.clone(),
            local: self.local.clone(),
            stats: self.stats.clone(),
        }
    }
}

impl Device for LinkDevice {
    type RxToken<'a> = LinkRx;
    type TxToken<'a> = LinkTx;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let packet = self.rx.pop()?;
        self.stats.rx_packets.fetch_add(1, Ordering::Relaxed);
        self.stats.rx_bytes.fetch_add(packet.len() as u64, Ordering::Relaxed);
        Some((LinkRx { packet }, self.tx_token()))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.hold_when_full && self.tx.is_full() {
            self.stats.tx_held.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        Some(self.tx_token())
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = self.medium;
        capabilities.max_transmission_unit = self.mtu;
        // smoltcp clamps the TCP window that it advertises to max_burst_size
        // segments (PodUtils/podnet#100). Some(1) gave a window of one
        // segment (about 10 KB with the window scale): 0.5 MB/s at an RTT of
        // 40 ms. The default is no limit.
        capabilities.max_burst_size = self.max_burst;
        capabilities.checksum = self.checksum.clone();
        capabilities
    }
}

/// The receive token: a packet from the inbound queue.
pub struct LinkRx {
    packet: Vec<u8>,
}

impl RxToken for LinkRx {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.packet)
    }
}

/// The transmit token: it writes the packet into the outbound queue (or, on
/// the loopback, into the inbound queue).
pub struct LinkTx {
    tx: PacketQueue,
    rx: Option<PacketQueue>,
    inbound: PacketQueue,
    local: Arc<RwLock<Vec<IpAddr>>>,
    stats: Arc<DeviceStats>,
}

/// The destination address of an IPv4 or IPv6 packet.
fn destination(packet: &[u8]) -> Option<IpAddr> {
    match packet.first()? >> 4 {
        4 if packet.len() >= 20 => {
            let b: [u8; 4] = packet[16..20].try_into().ok()?;
            Some(IpAddr::from(b))
        }
        6 if packet.len() >= 40 => {
            let b: [u8; 16] = packet[24..40].try_into().ok()?;
            Some(IpAddr::from(b))
        }
        _ => None,
    }
}

impl TxToken for LinkTx {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0u8; len];
        let result = f(&mut buffer);
        self.stats.tx_packets.fetch_add(1, Ordering::Relaxed);
        self.stats.tx_bytes.fetch_add(buffer.len() as u64, Ordering::Relaxed);
        let to_self = self.rx.is_none()
            && destination(&buffer).is_some_and(|dst| self.local.read().map(|l| l.contains(&dst)).unwrap_or(false));
        match &self.rx {
            Some(rx) => {
                if !rx.push(buffer) {
                    self.stats.rx_dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            None if to_self => {
                if !self.inbound.push(buffer) {
                    self.stats.rx_dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            None => {
                if !self.tx.push(buffer) {
                    self.stats.tx_dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests;
