use super::*;

// The core of the reactor: the interface, the socket set and the state of
// each connection.

pub(super) struct Conn {
    pub(super) handle: SocketHandle,
    pub(super) local: SocketAddr,
    pub(super) remote: SocketAddr,
    pub(super) to_app: mpsc::Sender<AppEvent>,
    pub(super) from_app: mpsc::Receiver<Vec<u8>>,
    pub(super) write_waker: Arc<AtomicWaker>,
    /// Set when the channel to the app was full: the next read of the app
    /// kicks the reactor, so that the data in the socket moves at once.
    pub(super) read_stalled: Arc<AtomicBool>,
    pub(super) ctl: mpsc::Receiver<ConnCmd>,
    pub(super) closing: bool,
    /// Set when the app dropped its stream: the stack ends it alone.
    pub(super) orphan: Option<Orphan>,
    /// Set at a reset: the reason that the app reads. After an abort, the
    /// socket leaves the set when smoltcp sent its RST.
    pub(super) reset: Option<String>,
    /// The state of the socket at the end of the last pass: a reset of the
    /// peer moves a synchronized socket to CLOSED at once.
    pub(super) state: tcp::State,
    pub(super) recv_eof_sent: bool,
    /// [`AppEvent::Connected`] was sent.
    pub(super) announced: bool,
    /// Bytes from the app that the socket did not take yet: `send_slice` can
    /// take part of a chunk when the send buffer is full.
    pub(super) pending: Vec<u8>,
    /// Why the stack ended the connection (a reset of each socket, the stop
    /// of the reactor); shared with the stream.
    pub(super) cause: Cause,
}

/// The watch of a stream that the app dropped: the last progress, and when.
/// No progress for `StackConfig::linger` aborts the connection.
pub(super) struct Orphan {
    pub(super) since: SmolInstant,
    /// The state, the send queue, and the bytes in `pending` with the chunks
    /// in the channel from the app.
    pub(super) seen: (tcp::State, usize, usize),
}

/// The local end of a listener or a UDP socket: one address, or each
/// address (`ip: None`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Local {
    pub(super) ip: Option<IpAddr>,
    pub(super) port: u16,
}

impl Local {
    pub(super) fn of(addr: SocketAddr) -> Self {
        Self { ip: Some(addr.ip()), port: addr.port() }
    }

    /// The address that the app sees: `[::]:port` for each address.
    pub(super) fn addr(self) -> SocketAddr {
        SocketAddr::new(self.ip.unwrap_or(IpAddr::V6(Ipv6Addr::UNSPECIFIED)), self.port)
    }

    pub(super) fn endpoint(self) -> IpListenEndpoint {
        IpListenEndpoint { addr: self.ip.map(to_smoltcp), port: self.port }
    }

    /// True when a packet can match both: smoltcp gives it to the first
    /// socket of its set, so the two would race.
    pub(super) fn overlaps(self, other: Local) -> bool {
        self.port == other.port && (self.ip.is_none() || other.ip.is_none() || self.ip == other.ip)
    }
}

/// A datagram from the stack to the app: its source and its destination.
pub(super) struct Inbound {
    pub(super) data: Vec<u8>,
    pub(super) from: SocketAddr,
    pub(super) to: SocketAddr,
}

/// A datagram from the app: its destination and, as an option, its source
/// address (the port is the port of the socket).
pub(super) struct Outbound {
    pub(super) data: Vec<u8>,
    pub(super) to: SocketAddr,
    pub(super) from: Option<IpAddr>,
}

pub(super) struct UdpConn {
    pub(super) handle: SocketHandle,
    pub(super) local: Local,
    pub(super) to_app: mpsc::Sender<Inbound>,
    pub(super) from_app: mpsc::Receiver<Outbound>,
    /// A datagram of the app that the send buffer of smoltcp did not take:
    /// it goes first at the next pass. A full buffer holds the app (the
    /// channel from the app fills), and loses nothing.
    pub(super) pending: Option<Outbound>,
    /// The datagrams that came in while the channel to the app was full,
    /// oldest first: at most `udp_packets` datagrams and `udp_payload`
    /// bytes. The reactor takes each datagram from smoltcp at once, so the
    /// receive buffer of smoltcp never fills and drops one with no count.
    pub(super) backlog: VecDeque<Inbound>,
    pub(super) backlog_bytes: usize,
    /// Set when the channel to the app was full: the next receive of the app
    /// kicks the reactor, so that the backlog moves at once.
    pub(super) read_stalled: Arc<AtomicBool>,
    /// When the reactor saw that the app dropped its socket.
    pub(super) gone: Option<SmolInstant>,
    /// Why the stack ended the socket; shared with the handle.
    pub(super) cause: Cause,
}

/// The UDP service of the consumer at the gateway.
pub(super) struct GatewayUdp {
    pub(super) handle: SocketHandle,
    pub(super) service: Arc<dyn GatewayService>,
}

pub(super) enum Command {
    Listen { local: Local, reply: oneshot::Sender<io::Result<TcpListener>> },
    Connect { remote: SocketAddr, local_port: u16, reply: oneshot::Sender<io::Result<TcpStream>> },
    BindUdp { local: Local, reply: oneshot::Sender<io::Result<UdpSocket>> },
    Shutdown,
}

pub(super) struct Core {
    /// The stop of the reactor, shared with the handle (`Drop for Core`).
    pub(super) halt: Arc<Halt>,
    /// The reason of a reset that the consumer asked for
    /// (`NetStack::reset_all`); the reactor takes it at its next pass.
    pub(super) reset_slot: Arc<Mutex<Option<String>>>,
    pub(super) iface: Interface,
    pub(super) device: LinkDevice,
    pub(super) sockets: SocketSet<'static>,
    /// The live and the highest socket counts, shared with the handle.
    ///
    /// The set lives in the task of the reactor, and it only grows unless the
    /// reactor removes the socket of each closed connection. The count shows
    /// that from outside: a test asserts on it, and does not infer a leak
    /// from a later failure.
    pub(super) socket_count: Arc<SocketCount>,
    pub(super) clock: Clock,
    pub(super) config: StackConfig,
    pub(super) listeners: Vec<Listener>,
    pub(super) conns: HashMap<u32, Conn>,
    pub(super) udp: HashMap<u32, UdpConn>,
    pub(super) next_id: u32,
    pub(super) events: mpsc::UnboundedSender<StackEvent>,
    pub(super) stats: Arc<NetStats>,
    pub(super) kick: Arc<Notify>,
    pub(super) gateway: Option<GatewayUdp>,
    /// The addresses that a listener made the stack own (`Core::own`).
    pub(super) owned: Vec<IpAddr>,
    /// The inbound queue of the device: the reactor reads the head of each
    /// packet before smoltcp takes it (`Core::admit_syn`).
    pub(super) inbound: PacketQueue,
}

pub(super) fn new_tcp_socket(config: &StackConfig) -> tcp::Socket<'static> {
    let size = config.tcp_buffer;
    let mut socket = tcp::Socket::new(tcp::SocketBuffer::new(vec![0u8; size]), tcp::SocketBuffer::new(vec![0u8; size]));
    // smoltcp 0.12 and later take CUBIC when its feature is on: the stack
    // sets the controller of its settings, none by default
    // (PodUtils/podnet#134).
    socket.set_congestion_control(config.congestion_control);
    socket
}

pub(super) fn new_udp_socket(config: &StackConfig) -> udp::Socket<'static> {
    let buffer =
        || udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; config.udp_packets], vec![0u8; config.udp_payload]);
    udp::Socket::new(buffer(), buffer())
}

pub(super) fn refuse(kind: io::ErrorKind, message: String) -> io::Error {
    io::Error::new(kind, message)
}

impl Core {
    pub(super) fn new(
        config: StackConfig,
        mut device: LinkDevice,
        events: mpsc::UnboundedSender<StackEvent>,
        kick: Arc<Notify>,
        socket_count: Arc<SocketCount>,
    ) -> Self {
        let mut iface_config = IfaceConfig::new(HardwareAddress::Ip);
        iface_config.random_seed = config.random_seed;
        let mut iface = Interface::new(iface_config, &mut device, SmolInstant::ZERO);
        iface.set_any_ip(config.any_ip);
        let gateway_v4 = config.subnet.gateway_v4();
        let gateway_v6 = config.subnet.gateway_v6();
        iface.update_ip_addrs(|addrs| {
            let prefix = config.subnet.v4_prefix;
            let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(config.local_v4), prefix));
            let v6_prefix = config.subnet.v6_prefix;
            let _ = addrs.push(IpCidr::new(IpAddress::Ipv6(config.local_v6), v6_prefix));
            // The stack answers for the gateway too, where the service of the
            // consumer lives, so that a client reaches `10.66.0.1:53`.
            if gateway_v4 != config.local_v4 {
                let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(gateway_v4), prefix));
            }
            if gateway_v6 != config.local_v6 {
                let _ = addrs.push(IpCidr::new(IpAddress::Ipv6(gateway_v6), v6_prefix));
            }
        });
        let mut sockets: SocketSet<'static> = SocketSet::new(Vec::new());
        // The first socket of the set, for the life of the stack: smoltcp
        // gives a datagram to the first socket that takes it, so the service
        // keeps the gateway from a UDP socket on any address of its port.
        let gateway = config.gateway_service.clone().and_then(|service| {
            let handle = sockets.add(new_udp_socket(&config));
            let socket = sockets.get_mut::<udp::Socket>(handle);
            let listen = IpListenEndpoint { addr: Some(IpAddress::Ipv4(gateway_v4)), port: service.port() };
            socket.bind(listen).ok()?;
            Some(GatewayUdp { handle, service })
        });
        let inbound = device.inbound();
        Self {
            halt: Arc::default(),
            reset_slot: Arc::default(),
            iface,
            device,
            sockets,
            socket_count: socket_count.clone(),
            clock: Clock::starting_at(0),
            config,
            listeners: Vec::new(),
            conns: HashMap::new(),
            udp: HashMap::new(),
            next_id: 1,
            events,
            stats: Arc::new(NetStats::default()),
            kick,
            gateway,
            owned: Vec::new(),
            inbound,
        }
    }

    /// `Interface::poll`, one inbound packet at a time. The reactor reads the
    /// head of each packet first: a SYN finds a free listen socket of its
    /// listener, so a burst to one port gets no RST (`Core::admit_syn`); a
    /// datagram goes from smoltcp to its socket's backlog at once
    /// (`Core::after_ingress`).
    pub(super) fn poll_iface(&mut self) {
        let now = self.clock.now();
        self.iface.poll_maintenance(now);
        loop {
            let head = self.inbound.peek(Head::parse);
            if let Some(head) = head
                && !self.admit_head(head)
            {
                continue;
            }
            let ingress = self.iface.poll_ingress_single(now, &mut self.device, &mut self.sockets);
            if ingress == PollIngressSingleResult::None {
                break;
            }
            if let Some(head) = head {
                self.after_ingress(head);
            }
        }
        while self.iface.poll_egress(now, &mut self.device, &mut self.sockets) == PollResult::SocketStateChanged {}
    }

    pub(super) fn next_delay(&mut self) -> Option<Duration> {
        let now = self.clock.now();
        self.iface
            .poll_delay(now, &self.sockets)
            .map(|delay| Duration::from_millis(delay.total_millis()))
            .map(|delay| delay.min(self.config.max_poll_interval))
    }

    pub(super) fn tcp_socket(&self) -> tcp::Socket<'static> {
        new_tcp_socket(&self.config)
    }

    pub(super) fn udp_socket(&self) -> udp::Socket<'static> {
        new_udp_socket(&self.config)
    }

    pub(super) fn emit(&self, event: StackEvent) {
        let _ = self.events.send(event);
    }

    pub(super) fn in_subnet(&self, addr: &SocketAddr) -> bool {
        self.config.subnet.contains(addr.ip())
    }

    pub(super) fn handle(&mut self, command: Command) {
        match command {
            Command::Listen { local, reply } => {
                let _ = reply.send(self.listen(local));
            }
            Command::Connect { remote, local_port, reply } => {
                let _ = reply.send(self.connect(remote, local_port));
            }
            Command::BindUdp { local, reply } => {
                let _ = reply.send(self.bind_udp(local));
            }
            Command::Shutdown => {}
        }
    }

    /// The checks of a bind: one address in the subnet, and admitted. The
    /// admit sees `[::]:port` for each address.
    pub(super) fn bindable(&self, local: Local) -> io::Result<()> {
        let addr = local.addr();
        if local.ip.is_some() && !self.in_subnet(&addr) {
            return Err(refuse(io::ErrorKind::AddrNotAvailable, format!("{addr} is outside the subnet of the stack")));
        }
        if !self.config.admits(addr) {
            return Err(refuse(io::ErrorKind::PermissionDenied, format!("the policy refuses a bind of {addr}")));
        }
        Ok(())
    }

    /// Remove a socket from the set: a set frees no slot itself.
    pub(super) fn drop_socket(&mut self, handle: SocketHandle) {
        self.sockets.remove(handle);
        self.socket_count.remove();
    }

    fn refused(&self, remote: SocketAddr, reason: &str) {
        self.stats.refused.fetch_add(1, Ordering::Relaxed);
        self.emit(StackEvent::Refused { remote, reason: reason.into() });
    }

    pub(super) fn connect(&mut self, remote: SocketAddr, local_port: u16) -> io::Result<TcpStream> {
        if !self.in_subnet(&remote) {
            self.refused(remote, "outside the subnet");
            return Err(refuse(
                io::ErrorKind::AddrNotAvailable,
                format!("{remote} is outside the subnet of the stack"),
            ));
        }
        if !self.config.admits(remote) {
            self.refused(remote, "refused by the policy");
            return Err(refuse(io::ErrorKind::PermissionDenied, format!("the policy refuses a connect to {remote}")));
        }
        if self.connections() >= self.config.max_connections {
            return Err(io::Error::new(io::ErrorKind::WouldBlock, "the stack is at its connection limit"));
        }
        let local_port =
            if local_port == 0 { ephemeral_port(self.next_id, self.config.random_seed) } else { local_port };
        // The local address has the family of the remote: smoltcp connects
        // no IPv4 socket to an IPv6 peer, and no IPv6 socket to an IPv4 peer.
        let local_ip =
            if remote.is_ipv4() { IpAddr::V4(self.config.local_v4) } else { IpAddr::V6(self.config.local_v6) };
        let local = SocketAddr::new(local_ip, local_port);
        let handle = self.sockets.add(self.tcp_socket());
        self.socket_count.add();
        let socket = self.sockets.get_mut::<tcp::Socket>(handle);
        if let Err(err) = socket.connect(
            self.iface.context(),
            IpEndpoint::new(to_smoltcp(remote.ip()), remote.port()),
            IpListenEndpoint { addr: Some(to_smoltcp(local.ip())), port: local.port() },
        ) {
            self.drop_socket(handle);
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{remote}: {err}")));
        }
        let id = self.next_id;
        self.next_id += 1;
        let stream = self.adopt(id, handle, local, remote);
        self.emit(StackEvent::Connected { id, local, remote });
        self.stats.connected.fetch_add(1, Ordering::Relaxed);
        Ok(stream)
    }

    /// Keep the state of the connection `id` on `handle`, and give its handle
    /// to the app.
    pub(super) fn adopt(&mut self, id: u32, handle: SocketHandle, local: SocketAddr, remote: SocketAddr) -> TcpStream {
        let state = self.sockets.get::<tcp::Socket>(handle).state();
        let (to_app_tx, to_app_rx) = mpsc::channel(64);
        let (from_app_tx, from_app_rx) = mpsc::channel(64);
        let (ctl_tx, ctl_rx) = mpsc::channel(16);
        let write_waker = Arc::new(AtomicWaker::new());
        let read_stalled = Arc::new(AtomicBool::new(false));
        let cause = Cause::default();
        self.conns.insert(
            id,
            Conn {
                handle,
                local,
                remote,
                to_app: to_app_tx,
                from_app: from_app_rx,
                write_waker: write_waker.clone(),
                read_stalled: read_stalled.clone(),
                ctl: ctl_rx,
                closing: false,
                orphan: None,
                reset: None,
                state,
                recv_eof_sent: false,
                announced: false,
                pending: Vec::new(),
                cause: cause.clone(),
            },
        );
        TcpStream {
            id,
            local,
            remote,
            read_rx: to_app_rx,
            write_tx: from_app_tx,
            write_waker,
            read_stalled,
            ctl_tx,
            kick: self.kick.clone(),
            read_buf: Vec::new(),
            read_pos: 0,
            eof: false,
            nodelay: false,
            cause,
        }
    }
}

pub(super) fn ephemeral_port(id: u32, seed: u64) -> u16 {
    // 49152..=65535 is the dynamic range; the id and the seed mix, so that a
    // restart does not reuse the same ports.
    let mixed = (id as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ seed;
    49152 + (mixed % (65535 - 49152 + 1)) as u16
}
