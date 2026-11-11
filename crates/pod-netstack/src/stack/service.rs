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

    pub(super) fn accept_inbound(&mut self) {
        let mut adopted: Vec<(usize, SocketHandle, Local, SocketAddr, SocketAddr)> = Vec::new();
        for (index, listener) in self.listeners.iter().enumerate() {
            for handle in &listener.handles {
                let socket = self.sockets.get::<tcp::Socket>(*handle);
                if socket.may_recv()
                    && let (Some(local), Some(remote)) = (socket.local_endpoint(), socket.remote_endpoint())
                {
                    // The local end is the destination of the SYN: on a
                    // listener on any address, the address that the peer
                    // dialed.
                    let (local, remote) = (endpoint_to_socket(local), endpoint_to_socket(remote));
                    adopted.push((index, *handle, listener.local, local, remote));
                }
            }
        }
        for (index, handle, on, local, remote) in adopted {
            if let Some(listener) = self.listeners.get_mut(index) {
                listener.handles.retain(|candidate| *candidate != handle);
            }
            let id = self.next_id;
            self.next_id += 1;
            let stream = self.adopt(id, handle, local, remote);
            match self.spawn_listener_socket(on) {
                Ok(replacement) => {
                    if let Some(listener) = self.listeners.get_mut(index) {
                        listener.handles.push(replacement);
                    }
                }
                Err(err) => tracing::warn!(error = %err, "could not fill the listener pool again"),
            }
            let queued = self.listeners.get_mut(index).map(|listener| listener.accept_tx.try_send(stream).is_ok());
            if queued != Some(true) {
                tracing::warn!("the accept queue is full: the stack drops an inbound connection");
                // Free the socket too. The connection is established: a
                // socket left in the set keeps its buffers, and the peer
                // keeps a half-open connection that nothing serves.
                if let Some(dropped) = self.conns.remove(&id) {
                    self.drop_socket(dropped.handle);
                }
                self.stats.dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            self.emit(StackEvent::Accepted { id, local, remote });
            self.stats.accepted.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn service_conns(&mut self) {
        let ids: Vec<u32> = self.conns.keys().copied().collect();
        let mut reap: Vec<(u32, Option<String>)> = Vec::new();
        for id in ids {
            let Some(conn) = self.conns.get_mut(&id) else {
                continue;
            };
            if service_ctl(conn, &mut self.sockets) {
                reap.push((id, Some("connection aborted".into())));
                continue;
            }
            let wrote = move_to_socket(conn, &mut self.sockets);
            move_to_app(conn, &mut self.sockets);
            if close_and_check(conn, &mut self.sockets) {
                reap.push((id, None));
            }
            if wrote {
                // The app had data on a full channel; it can write again.
                conn.write_waker.wake();
            }
        }
        for (id, reason) in reap {
            if let Some(conn) = self.conns.remove(&id) {
                let event = match reason {
                    Some(reason) => AppEvent::Reset(reason),
                    None => AppEvent::Eof,
                };
                let _ = conn.to_app.try_send(event);
                // Free the smoltcp socket: with no `remove`, each closed
                // connection keeps its two buffers for the life of the
                // process.
                self.drop_socket(conn.handle);
                self.emit(StackEvent::Closed { id });
                self.stats.closed.fetch_add(1, Ordering::Relaxed);
            }
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

/// Run the control commands of `conn`. True when the app aborted the
/// connection: the caller reaps it with a reset.
fn service_ctl(conn: &mut Conn, sockets: &mut SocketSet<'static>) -> bool {
    loop {
        match conn.ctl.try_recv() {
            // A close marks the connection for its end. The socket closes
            // after the queues are empty (see `close_and_check`): a handle
            // dropped while a reply is on its way must not cut the exchange.
            //
            // This is the only way that the reactor sees a dropped handle:
            // `Drop` of `TcpStream` sends it, and `to_app.is_closed()` looks
            // the other way and stays open after the app goes.
            Ok(ConnCmd::Close) => conn.closing = true,
            Ok(ConnCmd::Abort) => {
                sockets.get_mut::<tcp::Socket>(conn.handle).abort();
                return true;
            }
            Ok(ConnCmd::SetNodelay(enabled)) => {
                sockets.get_mut::<tcp::Socket>(conn.handle).set_nagle_enabled(!enabled);
            }
            Err(_) => return false,
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
fn move_to_app(conn: &mut Conn, sockets: &mut SocketSet<'static>) {
    let socket = sockets.get_mut::<tcp::Socket>(conn.handle);
    while socket.may_recv() {
        // Take a place in the channel first: bytes taken from the socket with
        // no place are lost, and the stream has a hole.
        let permit = match conn.to_app.try_reserve() {
            Ok(permit) => permit,
            Err(mpsc::error::TrySendError::Full(())) => {
                conn.read_stalled.store(true, Ordering::Release);
                break;
            }
            // A gone reader closes below.
            Err(mpsc::error::TrySendError::Closed(())) => break,
        };
        let mut buffer = [0u8; 4096];
        match socket.recv_slice(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => permit.send(AppEvent::Data(buffer[..n].to_vec())),
        }
    }
    // A reader that has gone closes the socket.
    if conn.to_app.is_closed() {
        socket.close();
        conn.closing = true;
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
    // A close request closes the socket only when nothing is owed in either
    // direction, so that a handle dropped in an exchange does not lose the
    // reply on its way. `TcpStream::drop` cannot make this decision: it does
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
    // A socket in TIME-WAIT is finished for this stack: it holds no data,
    // and smoltcp does not move it again, as the peer (the last to close)
    // sends the segment that ends a TIME-WAIT. A socket kept there leaves a
    // corpse for each short connection, and a load of many grows with no
    // limit. So `TimeWait` and `Closing` count as closed here.
    !open || matches!(state, tcp::State::Closed | tcp::State::TimeWait | tcp::State::Closing)
}
