use super::*;

// The listeners of the reactor: a listener keeps one free listen socket,
// adds one for each SYN that finds none, and frees each of its sockets at
// its close.

/// The streams that wait in the queue of a listener for `accept`. A full
/// queue drops nothing: an established connection waits in its listener.
const ACCEPT_QUEUE: usize = 64;

pub(super) struct Listener {
    pub(super) local: Local,
    /// The sockets that the reactor did not adopt: the free listen socket,
    /// the half-open ones, and the established ones that wait for room in
    /// the queue.
    pub(super) handles: Vec<SocketHandle>,
    pub(super) accept_tx: mpsc::Sender<TcpStream>,
    /// Set when the queue was full: the next `accept` kicks the reactor.
    pub(super) stalled: Arc<AtomicBool>,
}

/// The head of an inbound TCP SYN (no ACK, no RST): its source and its
/// destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Syn {
    pub(super) from: SocketAddr,
    pub(super) to: SocketAddr,
}

impl Syn {
    /// Read an IPv4 or IPv6 packet. A packet with IPv6 extension headers is
    /// not read: smoltcp gives it to the free listen socket.
    pub(super) fn parse(packet: &[u8]) -> Option<Syn> {
        let (from, to, tcp): (IpAddr, IpAddr, &[u8]) = match packet.first()? >> 4 {
            4 => {
                let header = usize::from(packet[0] & 0x0f) * 4;
                // Only a first fragment holds the TCP header.
                let offset = u16::from_be_bytes([*packet.get(6)?, *packet.get(7)?]) & 0x1fff;
                if *packet.get(9)? != 6 || header < 20 || offset != 0 {
                    return None;
                }
                let from: [u8; 4] = packet.get(12..16)?.try_into().ok()?;
                let to: [u8; 4] = packet.get(16..20)?.try_into().ok()?;
                (from.into(), to.into(), packet.get(header..)?)
            }
            6 => {
                if *packet.get(6)? != 6 {
                    return None;
                }
                let from: [u8; 16] = packet.get(8..24)?.try_into().ok()?;
                let to: [u8; 16] = packet.get(24..40)?.try_into().ok()?;
                (from.into(), to.into(), packet.get(40..)?)
            }
            _ => return None,
        };
        let flags = *tcp.get(13)?;
        // SYN set; ACK and RST clear.
        if flags & 0x02 == 0 || flags & 0x14 != 0 {
            return None;
        }
        let port = |at: usize| Some(u16::from_be_bytes([*tcp.get(at)?, *tcp.get(at + 1)?]));
        Some(Syn { from: SocketAddr::new(from, port(0)?), to: SocketAddr::new(to, port(2)?) })
    }
}

impl Local {
    /// True when a packet to `to` can reach this local end.
    pub(super) fn takes(self, to: SocketAddr) -> bool {
        self.port == to.port() && self.ip.is_none_or(|ip| ip == to.ip())
    }
}

/// The ends of a connection whose handshake ended, and that the peer did
/// not reset. The local end is the destination of the SYN: on a listener on
/// any address, the address that the peer dialed.
fn synchronized(socket: &tcp::Socket) -> Option<(SocketAddr, SocketAddr)> {
    let state = socket.state();
    let open =
        matches!(state, tcp::State::Established | tcp::State::CloseWait | tcp::State::FinWait1 | tcp::State::FinWait2);
    let (local, remote) = (socket.local_endpoint()?, socket.remote_endpoint()?);
    open.then(|| (endpoint_to_socket(local), endpoint_to_socket(remote)))
}

impl Core {
    pub(super) fn listen(&mut self, local: Local) -> io::Result<TcpListener> {
        self.bindable(local)?;
        self.prune_listeners();
        let addr = local.addr();
        if self.listeners.iter().any(|listener| listener.local.overlaps(local)) {
            return Err(refuse(io::ErrorKind::AddrInUse, format!("{addr}: a listener holds port {}", local.port)));
        }
        if let Some(ip) = local.ip {
            self.own(ip, addr)?;
        }
        let handle = match self.spawn_listener_socket(local) {
            Ok(handle) => handle,
            Err(err) => {
                self.release_addresses();
                return Err(err);
            }
        };
        let (accept_tx, rx) = mpsc::channel(ACCEPT_QUEUE);
        let stalled = Arc::new(AtomicBool::new(false));
        self.listeners.push(Listener { local, handles: vec![handle], accept_tx, stalled: stalled.clone() });
        self.emit(StackEvent::ListenerBound { addr });
        Ok(TcpListener { local: addr, rx, stalled, kick: self.kick.clone() })
    }

    /// Own `ip` for a listener on an address that the stack does not own
    /// yet, so that a SYN to it gets an answer, not the time limit of the
    /// client (PodUtils/podnet#41). The list of the interface, not
    /// `has_ip_addr`: with any IP, that is true for each address, and the
    /// device would send a packet to `ip` out on the link.
    fn own(&mut self, ip: IpAddr, addr: SocketAddr) -> io::Result<()> {
        let smol = to_smoltcp(ip);
        if !self.iface.ip_addrs().iter().any(|cidr| cidr.address() == smol) && !smol.is_unspecified() {
            let prefix = self.config.subnet.prefix_for(ip);
            let mut full = false;
            self.iface.update_ip_addrs(|addrs| {
                full = addrs.push(IpCidr::new(smol, prefix)).is_err();
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
            self.device.add_local(ip);
            self.owned.push(ip);
        }
        Ok(())
    }

    /// Drop each address that `own` added and that no listener and no
    /// connection uses: the stack owns few addresses, and smoltcp resets a
    /// socket whose local address the interface does not have.
    pub(super) fn release_addresses(&mut self) {
        let mut unused = Vec::new();
        for &ip in &self.owned {
            let held = self.listeners.iter().any(|listener| listener.local.ip == Some(ip))
                || self.conns.values().any(|conn| conn.local.ip() == ip || conn.remote.ip() == ip);
            if !held {
                unused.push(ip);
            }
        }
        if unused.is_empty() {
            return;
        }
        self.owned.retain(|ip| !unused.contains(ip));
        self.iface.update_ip_addrs(|addrs| addrs.retain(|cidr| !unused.contains(&IpAddr::from(cidr.address()))));
        for ip in unused {
            self.device.remove_local(ip);
        }
    }

    pub(super) fn spawn_listener_socket(&mut self, local: Local) -> io::Result<SocketHandle> {
        let handle = self.sockets.add(self.tcp_socket());
        self.socket_count.add();
        let socket = self.sockets.get_mut::<tcp::Socket>(handle);
        if let Err(err) = socket.listen(local.endpoint()) {
            self.drop_socket(handle);
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("{}: {err}", local.addr())));
        }
        Ok(handle)
    }

    /// Remove each listener whose `TcpListener` has gone, with its listen
    /// sockets and its half-open ones (the ACK of the peer then gets a RST).
    /// Its established connections close as dropped streams do: each ends
    /// with its FIN.
    pub(super) fn prune_listeners(&mut self) {
        if !self.listeners.iter().any(|listener| listener.accept_tx.is_closed()) {
            return;
        }
        let (closed, open) =
            std::mem::take(&mut self.listeners).into_iter().partition(|listener| listener.accept_tx.is_closed());
        self.listeners = open;
        for listener in closed {
            for handle in listener.handles {
                match synchronized(self.sockets.get::<tcp::Socket>(handle)) {
                    Some((local, remote)) => {
                        let id = self.next_id;
                        self.next_id += 1;
                        // The drop of the stream asks for its close.
                        drop(self.adopt(id, handle, local, remote));
                        self.emit(StackEvent::Accepted { id, local, remote });
                        self.stats.accepted.fetch_add(1, Ordering::Relaxed);
                    }
                    None => self.drop_socket(handle),
                }
            }
        }
        self.release_addresses();
    }

    /// The connections of the stack, open and half-open.
    pub(super) fn connections(&self) -> usize {
        let half_open = self.listeners.iter().flat_map(|listener| &listener.handles);
        self.conns.len() + half_open.filter(|handle| !self.sockets.get::<tcp::Socket>(**handle).is_listening()).count()
    }

    /// Before smoltcp takes the next inbound packet, `syn`: a SYN to the
    /// port of a listener finds a free listen socket of that listener, or
    /// waits at `max_connections`. False when the stack dropped the packet.
    pub(super) fn admit_syn(&mut self, syn: Syn) -> bool {
        let Some(index) = self.listeners.iter().position(|listener| listener.local.takes(syn.to)) else {
            return true;
        };
        let (from, to) = (Some(IpEndpoint::from(syn.from)), Some(IpEndpoint::from(syn.to)));
        let mut free = false;
        let mut held = false;
        for handle in &self.listeners[index].handles {
            let socket = self.sockets.get::<tcp::Socket>(*handle);
            free |= socket.is_listening();
            held |= socket.remote_endpoint() == from && socket.local_endpoint() == to;
        }
        // A SYN sent again (its SYN-ACK was lost or late) whose 4-tuple a
        // socket holds. smoltcp gives a segment to the first socket of its
        // set that takes it, and a listen socket in an earlier slot takes
        // each SYN of its port: two sockets would hold one 4-tuple (T-223).
        // The socket that holds it ignores a SYN with no ACK, and sends its
        // SYN-ACK again at its own time, so the stack drops the SYN.
        held |= self.conns.values().any(|conn| {
            // A SYN to a socket in SYN-SENT is a simultaneous open: it goes.
            conn.local == syn.to
                && conn.remote == syn.from
                && self.sockets.get::<tcp::Socket>(conn.handle).state() != tcp::State::SynSent
        });
        if held {
            let _ = self.inbound.pop();
            return false;
        }
        if free {
            return true;
        }
        if self.connections() >= self.config.max_connections {
            // The SYN waits: the peer sends it again, maybe after a close.
            let _ = self.inbound.pop();
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let local = self.listeners[index].local;
        match self.spawn_listener_socket(local) {
            Ok(handle) => self.listeners[index].handles.push(handle),
            Err(err) => tracing::warn!(error = %err, "could not add a listen socket"),
        }
        true
    }

    /// Give each established connection of each listener to its queue, and
    /// keep one free listen socket in each listener.
    pub(super) fn accept_inbound(&mut self) {
        self.prune_listeners();
        for index in 0..self.listeners.len() {
            self.adopt_established(index);
            self.keep_one_free(index);
        }
    }

    fn adopt_established(&mut self, index: usize) {
        let ready: Vec<(SocketHandle, SocketAddr, SocketAddr)> = self.listeners[index]
            .handles
            .iter()
            .filter_map(|handle| {
                let (local, remote) = synchronized(self.sockets.get::<tcp::Socket>(*handle))?;
                Some((*handle, local, remote))
            })
            .collect();
        for (handle, local, remote) in ready {
            let listener = &self.listeners[index];
            // A place in the queue first: with none, the connection waits in
            // the listener, and the next `accept` kicks the reactor.
            let permit = match listener.accept_tx.clone().try_reserve_owned() {
                Ok(permit) => permit,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    listener.stalled.store(true, Ordering::Release);
                    return;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => return,
            };
            self.listeners[index].handles.retain(|candidate| *candidate != handle);
            let id = self.next_id;
            self.next_id += 1;
            permit.send(self.adopt(id, handle, local, remote));
            self.emit(StackEvent::Accepted { id, local, remote });
            self.stats.accepted.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// One free listen socket: a SYN that `admit_syn` cannot read finds it.
    /// A socket that a RST closed leaves the listener.
    fn keep_one_free(&mut self, index: usize) {
        let mut free = 0;
        let mut gone = Vec::new();
        for handle in &self.listeners[index].handles {
            let socket = self.sockets.get::<tcp::Socket>(*handle);
            if socket.is_listening() {
                free += 1;
                if free > 1 {
                    gone.push(*handle);
                }
            } else if !socket.is_open() {
                gone.push(*handle);
            }
        }
        if !gone.is_empty() {
            self.listeners[index].handles.retain(|handle| !gone.contains(handle));
            for handle in gone {
                self.drop_socket(handle);
            }
        }
        if free == 0 && self.connections() < self.config.max_connections {
            let local = self.listeners[index].local;
            match self.spawn_listener_socket(local) {
                Ok(handle) => self.listeners[index].handles.push(handle),
                Err(err) => tracing::warn!(error = %err, "could not fill the listener again"),
            }
        }
    }
}
