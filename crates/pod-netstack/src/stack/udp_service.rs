use super::*;

// The UDP of the reactor: the head of each inbound datagram, the backlog of
// a socket whose app reads slowly, the sends of the apps, the gateway
// service, and the size of a datagram that the stack can carry.

/// The largest IPv4 UDP payload: 65535 bytes of a packet, less 20 of IPv4
/// and 8 of UDP.
const MAX_IPV4_DATAGRAM: usize = 65_507;

/// The largest UDP payload that a stack sends to each family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Limits {
    pub(super) v4: usize,
    pub(super) v6: usize,
}

impl Limits {
    pub(super) fn of(config: &StackConfig) -> Self {
        // The MTU of the link device (`LinkDevice::new`).
        Self::compute(config.mtu.max(576), config.udp_payload, crate::IPV4_FRAGMENTATION)
    }

    /// The headers: 20 bytes of IPv4 or 40 of IPv6, and 8 of UDP. smoltcp
    /// drops an IPv6 packet over the MTU, and an IPv4 packet over the MTU
    /// with no fragmentation; its fragmenter holds one whole IPv4 packet.
    pub(super) fn compute(mtu: usize, udp_payload: usize, fragmentation: bool) -> Self {
        let v4 = if fragmentation {
            MAX_IPV4_DATAGRAM.min(smoltcp::config::FRAGMENTATION_BUFFER_SIZE.saturating_sub(28))
        } else {
            mtu.saturating_sub(28)
        };
        Self { v4: v4.min(udp_payload), v6: mtu.saturating_sub(48).min(udp_payload) }
    }

    pub(super) fn to(self, ip: IpAddr) -> usize {
        if ip.is_ipv4() { self.v4 } else { self.v6 }
    }

    /// Refuse a datagram of `len` bytes to `to` that the stack cannot carry,
    /// and count it.
    pub(super) fn check(self, len: usize, to: SocketAddr, stats: &NetStats) -> io::Result<()> {
        let limit = self.to(to.ip());
        if len <= limit {
            return Ok(());
        }
        NetStats::count(&stats.udp_oversize);
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("a datagram of {len} bytes to {to} is over the {limit} bytes that the stack can carry"),
        ))
    }
}

/// What the reactor reads in the head of an inbound packet before smoltcp
/// takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Head {
    Syn(Syn),
    /// A whole UDP datagram: its destination and the bytes of its payload.
    Udp {
        to: SocketAddr,
        len: usize,
    },
    /// A fragment of an IPv4 or an IPv6 packet. `udp`: on the first fragment
    /// of a UDP datagram, its destination and its length.
    Fragment {
        v4: bool,
        udp: Option<(SocketAddr, usize)>,
    },
    Other,
}

impl Head {
    pub(super) fn parse(packet: &[u8]) -> Head {
        if let Some(syn) = Syn::parse(packet) {
            return Head::Syn(syn);
        }
        Self::datagram(packet).unwrap_or(Head::Other)
    }

    /// A packet with IPv6 extension headers before its UDP or fragment
    /// header is not read: smoltcp takes it as before.
    fn datagram(packet: &[u8]) -> Option<Head> {
        let byte = |at: usize| packet.get(at).copied();
        let word = |at: usize| Some(u16::from_be_bytes([byte(at)?, byte(at + 1)?]));
        match byte(0)? >> 4 {
            4 => {
                let header = usize::from(byte(0)? & 0x0f) * 4;
                let to: [u8; 4] = packet.get(16..20)?.try_into().ok()?;
                if header < 20 || byte(9)? != 17 {
                    // A fragment of another protocol: smoltcp cannot use it
                    // with no reassembly either.
                    let fragment = word(6)? & 0x3fff != 0;
                    return fragment.then_some(Head::Fragment { v4: true, udp: None });
                }
                let flags = word(6)?;
                let (more, offset) = (flags & 0x2000 != 0, flags & 0x1fff);
                let udp = if offset == 0 { udp_head(packet.get(header..)?, IpAddr::from(to)) } else { None };
                if more || offset != 0 {
                    return Some(Head::Fragment { v4: true, udp });
                }
                udp.map(|(to, len)| Head::Udp { to, len })
            }
            6 => {
                let to: [u8; 16] = packet.get(24..40)?.try_into().ok()?;
                match byte(6)? {
                    17 => udp_head(packet.get(40..)?, IpAddr::from(to)).map(|(to, len)| Head::Udp { to, len }),
                    44 => {
                        let first = word(42)? >> 3 == 0 && byte(40)? == 17;
                        let udp = if first { udp_head(packet.get(48..)?, IpAddr::from(to)) } else { None };
                        Some(Head::Fragment { v4: false, udp })
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// The destination and the payload length of a UDP header.
fn udp_head(udp: &[u8], ip: IpAddr) -> Option<(SocketAddr, usize)> {
    let port = u16::from_be_bytes([*udp.get(2)?, *udp.get(3)?]);
    let len = usize::from(u16::from_be_bytes([*udp.get(4)?, *udp.get(5)?]));
    Some((SocketAddr::new(ip, port), len.checked_sub(8)?))
}

impl Core {
    /// Before smoltcp takes the next inbound packet. False when the stack
    /// dropped it.
    pub(super) fn admit_head(&mut self, head: Head) -> bool {
        match head {
            Head::Syn(syn) => self.listeners.is_empty() || self.admit_syn(syn),
            // The receive buffer of smoltcp cannot hold it: it would drop the
            // datagram with no count.
            Head::Udp { to, len } if len > self.config.udp_payload && self.takes_datagram(to) => {
                NetStats::count(&self.stats.udp_oversize);
                let _ = self.inbound.pop();
                false
            }
            Head::Fragment { v4, udp } => {
                let reassembles = v4 && crate::IPV4_FRAGMENTATION;
                let lost = udp.is_some_and(|(to, len)| {
                    (!reassembles || len > self.config.udp_payload) && self.takes_datagram(to)
                });
                if lost {
                    NetStats::count(&self.stats.udp_oversize);
                }
                // With no reassembly, smoltcp reads an IPv4 fragment as a
                // whole packet: drop it here. smoltcp answers an IPv6
                // fragment itself.
                if v4 && !reassembles {
                    let _ = self.inbound.pop();
                    return false;
                }
                true
            }
            _ => true,
        }
    }

    /// After smoltcp took a packet: move a datagram that it gave to a socket
    /// out of smoltcp at once. The last fragment of a datagram can complete
    /// a datagram of each socket.
    pub(super) fn after_ingress(&mut self, head: Head) {
        match head {
            Head::Udp { to, .. } => self.take_datagrams(Some(to)),
            Head::Fragment { v4: true, .. } if crate::IPV4_FRAGMENTATION => self.take_datagrams(None),
            _ => {}
        }
    }

    /// The local end of the gateway service.
    fn gateway_local(&self) -> Option<Local> {
        let gateway = self.gateway.as_ref()?;
        Some(Local { ip: Some(IpAddr::V4(self.config.subnet.gateway_v4())), port: gateway.service.port() })
    }

    /// True when a socket of the stack takes a datagram to `to`.
    fn takes_datagram(&self, to: SocketAddr) -> bool {
        self.gateway_local().is_some_and(|local| local.takes(to)) || self.udp.values().any(|c| c.local.takes(to))
    }

    /// Move the datagrams that smoltcp holds for the socket of `to` (None:
    /// for each socket) to its app or its backlog. smoltcp gives a datagram
    /// to the first socket of its set that takes it: the gateway service,
    /// then the one socket whose local end takes it.
    fn take_datagrams(&mut self, to: Option<SocketAddr>) {
        let gateway = self.gateway_local();
        if to.is_none_or(|to| gateway.is_some_and(|local| local.takes(to))) {
            self.service_gateway();
            if to.is_some() {
                return;
            }
        }
        for conn in self.udp.values_mut() {
            if to.is_none_or(|to| conn.local.takes(to)) {
                receive(conn, &mut self.sockets, &self.config, &self.stats);
            }
        }
    }

    pub(super) fn service_udp(&mut self) {
        let now = self.clock.now();
        let linger = SmolDuration::from_millis(self.config.linger.as_millis().try_into().unwrap_or(u64::MAX));
        let ids: Vec<u32> = self.udp.keys().copied().collect();
        for id in ids {
            self.send_from_app(id);
            let Some(conn) = self.udp.get_mut(&id) else {
                continue;
            };
            receive(conn, &mut self.sockets, &self.config, &self.stats);
            if finished(conn, &self.sockets, now, linger)
                && let Some(conn) = self.udp.remove(&id)
            {
                self.drop_socket(conn.handle);
            }
        }
    }

    /// Move the datagrams of the app into smoltcp. A datagram that the send
    /// buffer does not take waits for the next pass.
    fn send_from_app(&mut self, id: u32) {
        let limits = Limits::of(&self.config);
        let Some(conn) = self.udp.get_mut(&id) else {
            return;
        };
        loop {
            let outbound = match conn.pending.take() {
                Some(outbound) => outbound,
                None => match conn.from_app.try_recv() {
                    Ok(outbound) => outbound,
                    Err(_) => return,
                },
            };
            let remote = outbound.to;
            // smoltcp panics on a packet whose source and destination are of
            // two families: the handles refuse it, and this is the last guard.
            let source = outbound.from.or(conn.local.ip);
            let mixed = source.is_some_and(|source| source.is_ipv4() != remote.is_ipv4());
            if mixed || !self.config.subnet.contains(remote.ip()) || !self.config.admits(remote) {
                NetStats::count(&self.stats.refused);
                continue;
            }
            if limits.check(outbound.data.len(), remote, &self.stats).is_err() {
                continue;
            }
            let mut meta = udp::UdpMetadata::from(IpEndpoint::new(to_smoltcp(remote.ip()), remote.port()));
            meta.local_address = outbound.from.map(to_smoltcp);
            let socket = self.sockets.get_mut::<udp::Socket>(conn.handle);
            match socket.send_slice(&outbound.data, meta) {
                Ok(()) => {
                    let shown = source.map_or(conn.local.addr(), |ip| SocketAddr::new(ip, conn.local.port));
                    let len = outbound.data.len();
                    let _ = self.events.send(StackEvent::Datagram { from: shown, to: remote, len });
                    NetStats::count(&self.stats.datagrams);
                }
                Err(udp::SendError::BufferFull) => {
                    conn.pending = Some(outbound);
                    return;
                }
                Err(udp::SendError::Unaddressable) => NetStats::count(&self.stats.refused),
            }
        }
    }

    /// Give each datagram to the port of the gateway service and send its
    /// answer back. A request with no answer gets nothing.
    pub(super) fn service_gateway(&mut self) {
        let Some(gateway) = self.gateway.as_ref() else {
            return;
        };
        let (handle, service) = (gateway.handle, gateway.service.clone());
        let limit = Limits::of(&self.config).v4;
        loop {
            let socket = self.sockets.get_mut::<udp::Socket>(handle);
            let Ok((request, metadata)) = socket.recv() else {
                break;
            };
            if request.len() > GATEWAY_DATAGRAM {
                NetStats::count(&self.stats.udp_rx_dropped);
                continue;
            }
            let Some(answer) = service.answer(endpoint_to_socket(metadata.endpoint), request) else {
                continue;
            };
            if answer.len() > limit {
                NetStats::count(&self.stats.udp_oversize);
                continue;
            }
            let socket = self.sockets.get_mut::<udp::Socket>(handle);
            if socket.send_slice(&answer, metadata.endpoint).is_err() {
                NetStats::count(&self.stats.udp_tx_dropped);
            }
        }
    }

    pub(super) fn bind_udp(&mut self, local: Local) -> io::Result<UdpSocket> {
        self.bindable(local)?;
        let addr = local.addr();
        // A socket that the app dropped holds its port until its queued
        // datagrams went: send them now.
        if self.udp.values().any(|conn| conn.local.overlaps(local) && conn.to_app.is_closed()) {
            self.service_udp();
            self.poll_iface();
            self.service_udp();
        }
        // The gateway service is the first socket of the set, so a socket on
        // any address of its port does not race it; one on its address
        // would never get a datagram.
        if self.udp.values().any(|conn| conn.local.overlaps(local)) || self.gateway_local() == Some(local) {
            return Err(refuse(io::ErrorKind::AddrInUse, format!("{addr}: a UDP socket holds port {}", local.port)));
        }
        let handle = self.sockets.add(self.udp_socket());
        self.socket_count.add();
        let socket = self.sockets.get_mut::<udp::Socket>(handle);
        if let Err(err) = socket.bind(local.endpoint()) {
            self.drop_socket(handle);
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("{addr}: {err}")));
        }
        let id = self.next_id;
        self.next_id += 1;
        let (to_app_tx, to_app_rx) = mpsc::channel(64);
        let (send_tx, send_rx) = mpsc::channel(64);
        let read_stalled = Arc::new(AtomicBool::new(false));
        self.udp.insert(
            id,
            UdpConn {
                handle,
                local,
                to_app: to_app_tx,
                from_app: send_rx,
                pending: None,
                backlog: VecDeque::new(),
                backlog_bytes: 0,
                read_stalled: read_stalled.clone(),
                gone: None,
            },
        );
        Ok(UdpSocket {
            id,
            local: addr,
            recv_rx: to_app_rx,
            send_tx,
            read_stalled,
            stats: self.stats.clone(),
            limits: Limits::of(&self.config),
            kick: DropKick(self.kick.clone()),
        })
    }
}

/// Move each datagram that smoltcp holds for `conn` into its backlog, and
/// the backlog into the channel to the app while it has room. A datagram
/// past the bound of the backlog is lost and counted.
fn receive(conn: &mut UdpConn, sockets: &mut SocketSet<'static>, config: &StackConfig, stats: &NetStats) {
    let socket = sockets.get_mut::<udp::Socket>(conn.handle);
    // No reader: the datagrams go, and the socket after them.
    let gone = conn.to_app.is_closed();
    while let Ok((data, metadata)) = socket.recv() {
        if gone {
            continue;
        }
        let full = |conn: &UdpConn| {
            conn.backlog.len() >= config.udp_packets || conn.backlog_bytes + data.len() > config.udp_payload
        };
        if full(conn) {
            flush(conn);
            if full(conn) {
                NetStats::count(&stats.udp_rx_dropped);
                continue;
            }
        }
        let from = endpoint_to_socket(metadata.endpoint);
        // smoltcp sets the destination of each datagram that it takes.
        let ip = metadata.local_address.map(IpAddr::from).or(conn.local.ip);
        let to = SocketAddr::new(ip.unwrap_or(conn.local.addr().ip()), conn.local.port);
        conn.backlog_bytes += data.len();
        conn.backlog.push_back(Inbound { data: data.to_vec(), from, to });
    }
    flush(conn);
}

/// Move the backlog into the channel to the app: a place first, then the
/// datagram, so that a full channel loses nothing.
fn flush(conn: &mut UdpConn) {
    while !conn.backlog.is_empty() {
        match conn.to_app.try_reserve() {
            Ok(permit) => {
                if let Some(datagram) = conn.backlog.pop_front() {
                    conn.backlog_bytes -= datagram.data.len();
                    permit.send(datagram);
                }
            }
            Err(mpsc::error::TrySendError::Full(())) => {
                conn.read_stalled.store(true, Ordering::Release);
                return;
            }
            Err(mpsc::error::TrySendError::Closed(())) => {
                conn.backlog.clear();
                conn.backlog_bytes = 0;
                return;
            }
        }
    }
}

/// True when the app dropped its socket and the stack can free it: each
/// datagram of the app went, or a link that takes nothing held them for
/// the linger.
fn finished(conn: &mut UdpConn, sockets: &SocketSet<'static>, now: SmolInstant, linger: SmolDuration) -> bool {
    if !conn.to_app.is_closed() {
        return false;
    }
    let since = *conn.gone.get_or_insert(now);
    let sent =
        conn.pending.is_none() && conn.from_app.is_empty() && sockets.get::<udp::Socket>(conn.handle).send_queue() == 0;
    sent || now >= since + linger
}
