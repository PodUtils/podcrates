use super::*;

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
    /// The time limit of each TCP socket with no packet of its peer: a
    /// connect, data with no ACK, or a keepalive with no answer. Then
    /// smoltcp aborts the socket (a RST), and the app reads `TimedOut`.
    /// smoltcp also counts it on an idle connection: give a
    /// [`StackConfig::tcp_keepalive`] shorter than it, so that an idle
    /// connection to a live peer stays. None: no limit.
    pub tcp_timeout: Option<Duration>,
    /// The interval of the TCP keepalive of an idle connection. None: no
    /// keepalive.
    pub tcp_keepalive: Option<Duration>,
    /// No Nagle on each TCP socket (`TCP_NODELAY`): a small write goes at
    /// once, with no wait for the ACK of the data before it.
    /// [`TcpStream::set_nodelay`] changes it for one stream.
    pub nodelay: bool,
}

impl Default for StackConfig {
    /// The default subnet, `10.66.0.2` and `fd00:66::2`, an MTU of 1280,
    /// buffers of 64 KiB, no admit, no gateway service, no any IP, each
    /// checksum computed and verified, a linger of 60 s, no congestion
    /// controller, no burst limit, counters of its own, and no TCP timeout,
    /// no keepalive and Nagle on.
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
            tcp_timeout: None,
            tcp_keepalive: None,
            nodelay: false,
        }
    }
}

impl StackConfig {
    /// True when the admit of the consumer (if any) admits `target`.
    pub(super) fn admits(&self, target: SocketAddr) -> bool {
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
