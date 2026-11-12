use super::*;

impl Core {
    /// Move data between smoltcp and the app, take the inbound connections,
    /// and remove the closed sockets.
    pub(super) fn service(&mut self) {
        self.accept_inbound();
        self.service_conns();
        self.service_udp();
        self.service_gateway();
    }

    /// Give each datagram to the port of the gateway service and send its
    /// answer back. A request with no answer gets nothing.
    pub(super) fn service_gateway(&mut self) {
        let Some(gateway) = self.gateway.as_ref() else {
            return;
        };
        let handle = gateway.handle;
        let service = gateway.service.clone();
        loop {
            let socket = self.sockets.get_mut::<udp::Socket>(handle);
            if !socket.can_recv() {
                break;
            }
            let mut buffer = [0u8; GATEWAY_DATAGRAM];
            let (n, metadata) = match socket.recv_slice(&mut buffer) {
                Ok(received) => received,
                Err(_) => break,
            };
            let Some(response) = service.answer(endpoint_to_socket(metadata.endpoint), &buffer[..n]) else {
                continue;
            };
            let socket = self.sockets.get_mut::<udp::Socket>(handle);
            let _ = socket.send_slice(&response, metadata.endpoint);
        }
    }

    pub(super) fn service_conns(&mut self) {
        let now = self.clock.now();
        let linger = SmolDuration::from_millis(self.config.linger.as_millis().try_into().unwrap_or(u64::MAX));
        let ids: Vec<u32> = self.conns.keys().copied().collect();
        let mut reap: Vec<u32> = Vec::new();
        for id in ids {
            let Some(conn) = self.conns.get_mut(&id) else {
                continue;
            };
            service_ctl(conn, &mut self.sockets);
            let wrote = move_to_socket(conn, &mut self.sockets);
            move_to_app(conn, &mut self.sockets, now);
            if close_and_check(conn, &mut self.sockets) {
                reap.push(id);
            } else {
                linger_orphan(conn, &mut self.sockets, now, linger);
            }
            if wrote {
                // The app had data on a full channel; it can write again.
                conn.write_waker.wake();
            }
        }
        let mut reaped = false;
        for id in reap {
            let Some(conn) = self.conns.get(&id) else {
                continue;
            };
            let event = match &conn.reset {
                Some(reason) => AppEvent::Reset(reason.clone()),
                None => AppEvent::Eof,
            };
            // The end waits for a place after the data: with none, the app
            // would read a reset as a clean end. The next read kicks the
            // reactor.
            if !tell_app(conn, event) && !conn.to_app.is_closed() {
                continue;
            }
            if let Some(conn) = self.conns.remove(&id) {
                // Free the smoltcp socket: with no `remove`, each closed
                // connection keeps its two buffers for the life of the
                // process.
                self.drop_socket(conn.handle);
                self.emit(StackEvent::Closed { id });
                self.stats.closed.fetch_add(1, Ordering::Relaxed);
                reaped = true;
            }
        }
        if reaped && !self.owned.is_empty() {
            self.release_addresses();
        }
    }

    pub(super) fn service_udp(&mut self) {
        let ids: Vec<u32> = self.udp.keys().copied().collect();
        for id in ids {
            let Some(conn) = self.udp.get_mut(&id) else {
                continue;
            };
            // App to stack.
            while let Ok(Outbound { data, to: remote, from }) = conn.from_app.try_recv() {
                // smoltcp panics on a packet whose source and destination
                // are of two families: the handles refuse it, and this is
                // the last guard.
                let source = from.or(conn.local.ip);
                let mixed = source.is_some_and(|source| source.is_ipv4() != remote.is_ipv4());
                if mixed || !self.config.subnet.contains(remote.ip()) || !self.config.admits(remote) {
                    self.stats.refused.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                let mut meta = udp::UdpMetadata::from(IpEndpoint::new(to_smoltcp(remote.ip()), remote.port()));
                meta.local_address = from.map(to_smoltcp);
                let socket = self.sockets.get_mut::<udp::Socket>(conn.handle);
                if socket.send_slice(&data, meta).is_err() {
                    break;
                }
                let shown = source.map_or(conn.local.addr(), |ip| SocketAddr::new(ip, conn.local.port));
                let _ = self.events.send(StackEvent::Datagram { from: shown, to: remote, len: data.len() });
                self.stats.datagrams.fetch_add(1, Ordering::Relaxed);
            }
            // Stack to app.
            loop {
                let socket = self.sockets.get_mut::<udp::Socket>(conn.handle);
                if !socket.can_recv() {
                    break;
                }
                let mut buffer = [0u8; 65535];
                match socket.recv_slice(&mut buffer) {
                    Ok((n, metadata)) => {
                        let from = endpoint_to_socket(metadata.endpoint);
                        // smoltcp sets the destination of each datagram that
                        // it takes.
                        let ip = metadata.local_address.map(IpAddr::from).or(conn.local.ip);
                        let to = SocketAddr::new(ip.unwrap_or(conn.local.addr().ip()), conn.local.port);
                        if conn.to_app.try_send(Inbound { data: buffer[..n].to_vec(), from, to }).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

/// Run the control commands of `conn`.
fn service_ctl(conn: &mut Conn, sockets: &mut SocketSet<'static>) {
    loop {
        match conn.ctl.try_recv() {
            // A close marks the connection for its end. The socket closes
            // after the queues are empty (see `close_and_check`): a handle
            // dropped while a reply is on its way must not cut the exchange.
            // A shutdown sends it; a drop sends it too, and the reactor also
            // sees the closed channel to the app (`move_to_app`).
            Ok(ConnCmd::Close) => conn.closing = true,
            Ok(ConnCmd::Abort) => {
                // The socket stays until smoltcp sent its RST (`close_and_check`).
                sockets.get_mut::<tcp::Socket>(conn.handle).abort();
                conn.reset = Some("connection aborted".into());
            }
            Ok(ConnCmd::SetNodelay(enabled)) => {
                sockets.get_mut::<tcp::Socket>(conn.handle).set_nagle_enabled(!enabled);
            }
            Err(_) => return,
        }
    }
}

/// Move the bytes of the app into the socket. True when the socket took
/// some.
fn move_to_socket(conn: &mut Conn, sockets: &mut SocketSet<'static>) -> bool {
    let mut wrote = false;
    let socket = sockets.get_mut::<tcp::Socket>(conn.handle);
    while socket.may_send() {
        if conn.pending.is_empty() {
            match conn.from_app.try_recv() {
                Ok(data) => conn.pending = data,
                Err(_) => break,
            }
        }
        match socket.send_slice(&conn.pending) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                conn.pending.drain(..n);
                wrote = true;
            }
        }
    }
    wrote
}

/// Send `event` to the app. False when the channel is full (the app kicks
/// the reactor at its next read) or closed.
fn tell_app(conn: &Conn, event: AppEvent) -> bool {
    match conn.to_app.try_send(event) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            conn.read_stalled.store(true, Ordering::Release);
            false
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    }
}

/// Move the bytes of the socket to the app, while the app reads.
fn move_to_app(conn: &mut Conn, sockets: &mut SocketSet<'static>, now: SmolInstant) {
    let socket = sockets.get_mut::<tcp::Socket>(conn.handle);
    // The app dropped its stream. The socket does not close at once: its
    // close would stop `move_to_socket`, and the bytes that wait in the
    // channel from the app would be lost. It closes after them
    // (`close_and_check`). The bytes that come in have no reader: discard
    // them, so that the window of the peer stays open.
    if conn.to_app.is_closed() {
        while socket.recv(|data| (data.len(), data.len())).is_ok_and(|n| n > 0) {}
        conn.closing = true;
        if conn.orphan.is_none() {
            let seen = (socket.state(), socket.send_queue(), conn.pending.len() + conn.from_app.len());
            conn.orphan = Some(Orphan { since: now, seen });
        }
        return;
    }
    while socket.may_recv() {
        // Take a place in the channel first: bytes taken from the socket with
        // no place are lost, and the stream has a hole.
        let permit = match conn.to_app.try_reserve() {
            Ok(permit) => permit,
            Err(mpsc::error::TrySendError::Full(())) => {
                conn.read_stalled.store(true, Ordering::Release);
                break;
            }
            // The reader went: the next pass discards the bytes.
            Err(mpsc::error::TrySendError::Closed(())) => break,
        };
        let mut buffer = [0u8; 4096];
        match socket.recv_slice(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => permit.send(AppEvent::Data(buffer[..n].to_vec())),
        }
    }
}

/// Abort a dropped stream that made no progress for `linger`: a dead peer,
/// or a peer that never sends its FIN, must not hold the socket for ever.
fn linger_orphan(conn: &mut Conn, sockets: &mut SocketSet<'static>, now: SmolInstant, linger: SmolDuration) {
    let Some(orphan) = conn.orphan.as_mut() else {
        return;
    };
    let socket = sockets.get_mut::<tcp::Socket>(conn.handle);
    let seen = (socket.state(), socket.send_queue(), conn.pending.len() + conn.from_app.len());
    if seen != orphan.seen {
        *orphan = Orphan { since: now, seen };
    } else if now >= orphan.since + linger && conn.reset.is_none() {
        socket.abort();
        conn.reset = Some("the peer made no progress for the linger of the stack".into());
    }
}

/// Tell the app about the handshake and the end of the receive side, close
/// the socket when the close of the app can go, and return true when the
/// connection is finished.
fn close_and_check(conn: &mut Conn, sockets: &mut SocketSet<'static>) -> bool {
    let socket = sockets.get_mut::<tcp::Socket>(conn.handle);
    let (state, send_queue, recv_queue, open) =
        (socket.state(), socket.send_queue(), socket.recv_queue(), socket.is_open());
    if !conn.announced
        && matches!(
            state,
            tcp::State::Established | tcp::State::CloseWait | tcp::State::FinWait1 | tcp::State::FinWait2
        )
    {
        conn.announced = tell_app(conn, AppEvent::Connected);
    }
    // The remote half-closed: tell the app once, and keep the write side
    // open until the app closes it. A full channel tells it at a later pass.
    if state == tcp::State::CloseWait && recv_queue == 0 && !conn.recv_eof_sent {
        conn.recv_eof_sent = tell_app(conn, AppEvent::Eof);
    }
    // A close request (a shutdown, a close or a drop) closes the socket only
    // when no byte waits in either direction: after `close`, smoltcp takes
    // no byte to send, so a byte in the channel from the app or in `pending`
    // would be lost. `TcpStream::drop` cannot make this decision: it does
    // not know what the reactor still holds.
    //
    // The flag stays from poll to poll: a close that comes while the send
    // queue holds the reply waits, and the check runs again at the next
    // pass. A flag of one poll forgets the request, no FIN goes, and a peer
    // that reads to the end waits forever.
    //
    // Before the handshake ends, a close waits too: smoltcp closes a socket
    // in SYN-SENT at once, which resets the connection and loses the data
    // that the app wrote first.
    let handshake_done = !matches!(state, tcp::State::SynSent | tcp::State::SynReceived | tcp::State::Listen);
    if conn.closing
        && handshake_done
        && send_queue == 0
        && recv_queue == 0
        && conn.pending.is_empty()
        && conn.from_app.is_empty()
    {
        socket.close();
    }
    // An aborted socket stays until smoltcp sent its RST: it forgets the
    // peer then.
    if state == tcp::State::Closed && socket.remote_endpoint().is_some() {
        return false;
    }
    // A RST of the peer closes a synchronized socket at once; a close passes
    // LAST-ACK, CLOSING or TIME-WAIT. The app reads a reset, not an end.
    let was_open = matches!(
        conn.state,
        tcp::State::Established | tcp::State::CloseWait | tcp::State::FinWait1 | tcp::State::FinWait2
    );
    if state == tcp::State::Closed && was_open && conn.reset.is_none() {
        conn.reset = Some("connection reset by the peer".into());
    }
    conn.state = socket.state();
    // A socket in TIME-WAIT is finished for this stack: it holds no data,
    // and smoltcp does not move it again, as the peer (the last to close)
    // sends the segment that ends a TIME-WAIT. A socket kept there leaves a
    // corpse for each short connection, and a load of many grows with no
    // limit. So `TimeWait` and `Closing` count as closed here.
    !open || matches!(state, tcp::State::Closed | tcp::State::TimeWait | tcp::State::Closing)
}
