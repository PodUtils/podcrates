//! The counters of a stack.

use std::sync::atomic::{AtomicU64, Ordering};

/// The counters of the connections and of the datagrams, readable while the
/// reactor runs.
#[derive(Debug, Default)]
pub struct NetStats {
    pub accepted: AtomicU64,
    pub connected: AtomicU64,
    pub refused: AtomicU64,
    pub closed: AtomicU64,
    /// The UDP datagrams of the apps that the stack sent.
    pub datagrams: AtomicU64,
    /// The inbound SYNs that the stack dropped at `max_connections`; the
    /// peer sends each again. An operator who reads `accepted` sees fewer
    /// connections than the peer made, and this counter tells why.
    pub dropped: AtomicU64,
    /// The inbound UDP datagrams that a socket of the stack lost: its app
    /// read too slowly, and the channel to the app and the backlog of the
    /// socket (`udp_packets`, `udp_payload`) were full. Also a request to
    /// the gateway service over `GATEWAY_DATAGRAM` bytes.
    pub udp_rx_dropped: AtomicU64,
    /// The outbound UDP datagrams that the stack lost: a
    /// `UdpDatagrams::try_send` that found the queue to the stack full, or an
    /// answer of the gateway service that found its send buffer full. A
    /// `UdpSocket::send_to` waits; it loses nothing.
    pub udp_tx_dropped: AtomicU64,
    /// The UDP datagrams over what the stack can carry: a send over
    /// `StackConfig::max_datagram` (refused with `InvalidInput`), an answer
    /// of the gateway service over it, and an inbound datagram to a socket of
    /// the stack in IP fragments that the stack cannot put together (IPv6,
    /// IPv4 with no feature `ipv4-fragmentation`) or over `udp_payload`.
    pub udp_oversize: AtomicU64,
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
    pub udp_rx_dropped: u64,
    pub udp_tx_dropped: u64,
    pub udp_oversize: u64,
}

impl NetStats {
    pub(super) fn snapshot(&self) -> NetStatsSnapshot {
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        NetStatsSnapshot {
            accepted: read(&self.accepted),
            connected: read(&self.connected),
            refused: read(&self.refused),
            closed: read(&self.closed),
            datagrams: read(&self.datagrams),
            dropped: read(&self.dropped),
            udp_rx_dropped: read(&self.udp_rx_dropped),
            udp_tx_dropped: read(&self.udp_tx_dropped),
            udp_oversize: read(&self.udp_oversize),
        }
    }

    /// Add one to `counter`.
    pub(super) fn count(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}
