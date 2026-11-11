use super::*;

// The core of the reactor: the interface, the socket set and the state of
// each connection.

pub(super) struct Conn {
    pub(super) handle: SocketHandle,
    pub(super) to_app: mpsc::Sender<AppEvent>,
    pub(super) from_app: mpsc::Receiver<Vec<u8>>,
    pub(super) write_waker: Arc<AtomicWaker>,
    /// Set when the channel to the app was full: the next read of the app
    /// kicks the reactor, so that the data in the socket moves at once.
    pub(super) read_stalled: Arc<AtomicBool>,
    pub(super) ctl: mpsc::Receiver<ConnCmd>,
    pub(super) closing: bool,
    pub(super) recv_eof_sent: bool,
    /// [`AppEvent::Connected`] was sent.
    pub(super) announced: bool,
    /// Bytes from the app that the socket did not take yet: `send_slice` can
    /// take part of a chunk when the send buffer is full.
    pub(super) pending: Vec<u8>,
}

pub(super) struct UdpConn {
    pub(super) handle: SocketHandle,
    pub(super) local: SocketAddr,
    pub(super) to_app: mpsc::Sender<(Vec<u8>, SocketAddr)>,
    pub(super) from_app: mpsc::Receiver<(Vec<u8>, SocketAddr)>,
}

/// The UDP service of the consumer at the gateway.
pub(super) struct GatewayUdp {
    pub(super) handle: SocketHandle,
    pub(super) service: Arc<dyn GatewayService>,
}

pub(super) struct Listener {
    pub(super) addr: SocketAddr,
    pub(super) handles: Vec<SocketHandle>,
    pub(super) accept_tx: mpsc::Sender<TcpStream>,
}

pub(super) enum Command {
    Listen { addr: SocketAddr, reply: oneshot::Sender<io::Result<TcpListener>> },
    Connect { remote: SocketAddr, local_port: u16, reply: oneshot::Sender<io::Result<TcpStream>> },
    BindUdp { addr: SocketAddr, reply: oneshot::Sender<io::Result<UdpSocket>> },
    Shutdown,
}

pub(super) struct Core {
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
}

pub(super) fn new_tcp_socket(config: &StackConfig) -> tcp::Socket<'static> {
    let size = config.tcp_buffer;
    let mut socket = tcp::Socket::new(tcp::SocketBuffer::new(vec![0u8; size]), tcp::SocketBuffer::new(vec![0u8; size]));
    // No congestion controller, as in smoltcp before 0.12: the link holds
    // the stack when its queue is full (PodUtils/podnet#134).
    socket.set_congestion_control(tcp::CongestionControl::None);
    socket
}

pub(super) fn new_udp_socket(config: &StackConfig) -> udp::Socket<'static> {
    let buffer =
        || udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; config.udp_packets], vec![0u8; config.udp_payload]);
    udp::Socket::new(buffer(), buffer())
}

fn refuse(kind: io::ErrorKind, message: String) -> io::Error {
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
        let gateway = config.gateway_service.clone().and_then(|service| {
            let handle = sockets.add(new_udp_socket(&config));
            let socket = sockets.get_mut::<udp::Socket>(handle);
            let listen = IpListenEndpoint { addr: Some(IpAddress::Ipv4(gateway_v4)), port: service.port() };
            socket.bind(listen).ok()?;
            Some(GatewayUdp { handle, service })
        });
        Self {
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
        }
    }

    pub(super) fn poll_iface(&mut self) {
        let now = self.clock.now();
        let _ = self.iface.poll(now, &mut self.device, &mut self.sockets);
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
            Command::Listen { addr, reply } => {
                let _ = reply.send(self.listen(addr));
            }
            Command::Connect { remote, local_port, reply } => {
                let _ = reply.send(self.connect(remote, local_port));
            }
            Command::BindUdp { addr, reply } => {
                let _ = reply.send(self.bind_udp(addr));
            }
            Command::Shutdown => {}
        }
    }

    /// The checks of a bind: in the subnet, admitted.
    fn bindable(&self, addr: SocketAddr) -> io::Result<()> {
        if !self.in_subnet(&addr) {
            return Err(refuse(io::ErrorKind::AddrNotAvailable, format!("{addr} is outside the subnet of the stack")));
        }
        if !self.config.admits(addr) {
            return Err(refuse(io::ErrorKind::PermissionDenied, format!("the policy refuses a bind of {addr}")));
        }
        Ok(())
    }

    pub(super) fn listen(&mut self, addr: SocketAddr) -> io::Result<TcpListener> {
        self.bindable(addr)?;
        if self.listeners.iter().any(|listener| listener.addr == addr) {
            return Err(refuse(io::ErrorKind::AddrInUse, format!("{addr} is already bound")));
        }
        // A listener on an address that the stack does not own yet: own it,
        // so that a SYN to it gets an answer, not the time limit of the
        // client (PodUtils/podnet#41).
        let ip = to_smoltcp(addr.ip());
        if !self.iface.has_ip_addr(ip) && !ip.is_unspecified() {
            let prefix = self.config.subnet.prefix_for(addr.ip());
            let mut full = false;
            self.iface.update_ip_addrs(|addrs| {
                full = addrs.push(IpCidr::new(ip, prefix)).is_err();
            });
            if full {
                return Err(refuse(
                    io::ErrorKind::AddrNotAvailable,
                    format!(
                        "{addr}: the stack owns at most {} addresses (its own, the gateways and the listeners)",
                        crate::MAX_ADDRESSES
                    ),
                ));
            }
            self.device.add_local(addr.ip());
        }
        let pool = self.config.accept_pool.max(1);
        let (accept_tx, accept_rx) = mpsc::channel(pool * 4);
        let mut handles = Vec::with_capacity(pool);
        for _ in 0..pool {
            match self.spawn_listener_socket(addr) {
                Ok(handle) => handles.push(handle),
                Err(err) => {
                    for handle in handles {
                        self.drop_socket(handle);
                    }
                    return Err(err);
                }
            }
        }
        self.listeners.push(Listener { addr, handles, accept_tx });
        self.emit(StackEvent::ListenerBound { addr });
        Ok(TcpListener { local: addr, rx: accept_rx })
    }

    pub(super) fn spawn_listener_socket(&mut self, addr: SocketAddr) -> io::Result<SocketHandle> {
        let handle = self.sockets.add(self.tcp_socket());
        self.socket_count.add();
        let socket = self.sockets.get_mut::<tcp::Socket>(handle);
        if let Err(err) = socket.listen(IpListenEndpoint { addr: Some(to_smoltcp(addr.ip())), port: addr.port() }) {
            self.drop_socket(handle);
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("{addr}: {err}")));
        }
        Ok(handle)
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
        if self.conns.len() >= self.config.max_connections {
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
        let (to_app_tx, to_app_rx) = mpsc::channel(64);
        let (from_app_tx, from_app_rx) = mpsc::channel(64);
        let (ctl_tx, ctl_rx) = mpsc::channel(16);
        let write_waker = Arc::new(AtomicWaker::new());
        let read_stalled = Arc::new(AtomicBool::new(false));
        self.conns.insert(
            id,
            Conn {
                handle,
                to_app: to_app_tx,
                from_app: from_app_rx,
                write_waker: write_waker.clone(),
                read_stalled: read_stalled.clone(),
                ctl: ctl_rx,
                closing: false,
                recv_eof_sent: false,
                announced: false,
                pending: Vec::new(),
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
        }
    }

    pub(super) fn bind_udp(&mut self, addr: SocketAddr) -> io::Result<UdpSocket> {
        self.bindable(addr)?;
        if self.udp.values().any(|conn| conn.local == addr) {
            return Err(refuse(io::ErrorKind::AddrInUse, format!("{addr} is already bound")));
        }
        let handle = self.sockets.add(self.udp_socket());
        self.socket_count.add();
        let socket = self.sockets.get_mut::<udp::Socket>(handle);
        if let Err(err) = socket.bind(IpListenEndpoint { addr: Some(to_smoltcp(addr.ip())), port: addr.port() }) {
            self.drop_socket(handle);
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("{addr}: {err}")));
        }
        let id = self.next_id;
        self.next_id += 1;
        let (to_app_tx, to_app_rx) = mpsc::channel(64);
        let (send_tx, send_rx) = mpsc::channel(64);
        self.udp.insert(id, UdpConn { handle, local: addr, to_app: to_app_tx, from_app: send_rx });
        Ok(UdpSocket { id, local: addr, recv_rx: to_app_rx, send_tx, kick: self.kick.clone() })
    }
}

pub(super) fn ephemeral_port(id: u32, seed: u64) -> u16 {
    // 49152..=65535 is the dynamic range; the id and the seed mix, so that a
    // restart does not reuse the same ports.
    let mixed = (id as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ seed;
    49152 + (mixed % (65535 - 49152 + 1)) as u16
}
