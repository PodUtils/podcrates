use super::*;

// The handles of the app: each talks to the reactor by channels, and kicks
// it after each command.

#[derive(Debug)]
pub(super) enum AppEvent {
    /// The handshake ended (sent once, before or after the first data).
    Connected,
    Data(Vec<u8>),
    Eof,
    Reset(String),
}

#[derive(Debug)]
pub(super) enum ConnCmd {
    Close,
    Abort,
    SetNodelay(bool),
}

/// A TCP connection of the stack, with [`AsyncRead`] and [`AsyncWrite`].
pub struct TcpStream {
    pub(super) id: u32,
    pub(super) local: SocketAddr,
    pub(super) remote: SocketAddr,
    pub(super) read_rx: mpsc::Receiver<AppEvent>,
    pub(super) write_tx: mpsc::Sender<Vec<u8>>,
    pub(super) write_waker: Arc<AtomicWaker>,
    pub(super) read_stalled: Arc<AtomicBool>,
    pub(super) ctl_tx: mpsc::Sender<ConnCmd>,
    pub(super) kick: Arc<Notify>,
    pub(super) read_buf: Vec<u8>,
    pub(super) read_pos: usize,
    pub(super) eof: bool,
    pub(super) nodelay: bool,
}

impl std::fmt::Debug for TcpStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpStream")
            .field("id", &self.id)
            .field("local", &self.local)
            .field("remote", &self.remote)
            .field("eof", &self.eof)
            .finish()
    }
}

impl TcpStream {
    /// Wait for the handshake of a connection that this stack opened. A
    /// refusal (a reset, or the end before the handshake) is an error of
    /// kind `ConnectionRefused` or `ConnectionReset`. Data that comes first
    /// stays for the next read.
    pub async fn established(&mut self) -> io::Result<()> {
        if self.eof {
            return Err(io::Error::from(io::ErrorKind::ConnectionRefused));
        }
        if self.read_pos < self.read_buf.len() {
            return Ok(());
        }
        let event = self.read_rx.recv().await;
        self.took_event();
        match event {
            Some(AppEvent::Connected) => Ok(()),
            Some(AppEvent::Data(data)) => {
                self.read_buf = data;
                self.read_pos = 0;
                Ok(())
            }
            Some(AppEvent::Eof) | None => {
                self.eof = true;
                Err(io::Error::new(io::ErrorKind::ConnectionRefused, "the connection was refused"))
            }
            Some(AppEvent::Reset(message)) => Err(io::Error::new(io::ErrorKind::ConnectionReset, message)),
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.remote
    }

    /// Set `TCP_NODELAY`: the reactor turns the Nagle algorithm of the
    /// smoltcp socket off (true) or on (false).
    pub fn set_nodelay(&mut self, enabled: bool) {
        self.nodelay = enabled;
        let _ = self.ctl_tx.try_send(ConnCmd::SetNodelay(enabled));
        self.kick.notify_one();
    }

    pub fn nodelay(&self) -> bool {
        self.nodelay
    }

    /// Close the write side (a FIN after the queued data).
    pub fn shutdown(&self) {
        let _ = self.ctl_tx.try_send(ConnCmd::Close);
        self.kick.notify_one();
    }

    /// Abort the connection (a RST).
    pub fn abort(&self) {
        let _ = self.ctl_tx.try_send(ConnCmd::Abort);
        self.kick.notify_one();
    }

    /// The app took an event from the channel: when the reactor found the
    /// channel full, kick it, so that the data kept in the socket moves now
    /// and not at the next tick.
    fn took_event(&self) {
        if self.read_stalled.swap(false, Ordering::AcqRel) {
            self.kick.notify_one();
        }
    }

    /// Copy the kept bytes into `buf`.
    fn put_kept(&mut self, buf: &mut ReadBuf<'_>) {
        let n = buf.remaining().min(self.read_buf.len() - self.read_pos);
        buf.put_slice(&self.read_buf[self.read_pos..self.read_pos + n]);
        self.read_pos += n;
        if self.read_pos >= self.read_buf.len() {
            self.read_buf.clear();
            self.read_pos = 0;
        }
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if self.read_pos < self.read_buf.len() {
            self.put_kept(buf);
            return Poll::Ready(Ok(()));
        }
        if self.eof {
            return Poll::Ready(Ok(()));
        }
        loop {
            let event = self.read_rx.poll_recv(cx);
            if event.is_ready() {
                self.took_event();
            }
            return match event {
                Poll::Ready(Some(AppEvent::Connected)) => continue,
                Poll::Ready(Some(AppEvent::Data(data))) => {
                    self.read_buf = data;
                    self.read_pos = 0;
                    self.put_kept(buf);
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Some(AppEvent::Eof)) | Poll::Ready(None) => {
                    self.eof = true;
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Some(AppEvent::Reset(message))) => {
                    Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, message)))
                }
                Poll::Pending => Poll::Pending,
            };
        }
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        match self.write_tx.try_send(buf.to_vec()) {
            Ok(()) => {
                // The reactor can wait for the link; tell it that the app has
                // data to move now, not at the next tick.
                self.kick.notify_one();
                Poll::Ready(Ok(buf.len()))
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                // The reactor empties the channel each time it serves the
                // connection, and wakes this waker: the back pressure of a
                // write.
                self.write_waker.register(cx.waker());
                Poll::Pending
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "the TCP connection is closed")))
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let _ = self.ctl_tx.try_send(ConnCmd::Close);
        self.kick.notify_one();
        Poll::Ready(Ok(()))
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        // A dropped stream frees its socket, but it must not close the
        // connection while data is on its way.
        //
        // With no `Drop`, the drop of the handle drops `ctl_tx`, and the
        // reactor does not learn of it: `to_app.is_closed()` looks the other
        // way and stays open. The socket then stays in CloseWait until the
        // peer closes, which never comes for two handles that one task drops,
        // and each such connection keeps its smoltcp socket for the life of
        // the process.
        //
        // No graceful close at once: a program that writes, reads to EOF and
        // drops has its own `Drop` before the reply of the peer comes, and a
        // close at once cuts the exchange (two tests read an empty answer).
        //
        // No abort at once: an abort discards the queued data, and a program
        // that wrote and dropped with no read loses the reply.
        //
        // So the handle asks, and the reactor, which knows whether bytes are
        // owed in either direction, closes and removes the socket.
        let _ = self.ctl_tx.try_send(ConnCmd::Close);
        self.kick.notify_one();
    }
}

/// A UDP socket of the stack.
pub struct UdpSocket {
    pub(super) id: u32,
    pub(super) local: SocketAddr,
    pub(super) recv_rx: mpsc::Receiver<Inbound>,
    pub(super) send_tx: mpsc::Sender<Outbound>,
    pub(super) kick: Arc<Notify>,
}

impl std::fmt::Debug for UdpSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdpSocket").field("id", &self.id).field("local", &self.local).finish()
    }
}

pub(super) fn udp_closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "the UDP socket is closed")
}

/// Refuse a datagram whose source and destination are of two families:
/// smoltcp makes no such packet, and its reactor would panic. `source` is
/// the address of the socket, or the source of the datagram; the
/// unspecified address of a socket on any address takes both families.
pub(super) fn one_family(source: IpAddr, remote: SocketAddr) -> io::Result<()> {
    if source.is_unspecified() || source.is_ipv4() == remote.is_ipv4() {
        return Ok(());
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{remote} is not of the family of {source}")))
}

impl UdpSocket {
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The bound address; `[::]:port` for a socket on any address.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Queue one datagram to `remote`; wait while the queue to the reactor
    /// is full. A `remote` of the other family than the bound address is
    /// `InvalidInput`.
    pub async fn send_to(&self, buf: &[u8], remote: impl Into<SocketAddr>) -> io::Result<usize> {
        self.queue(buf, remote.into(), None).await
    }

    /// Queue one datagram to `remote` from `source`, an address that the
    /// stack need not own (a router answers for the destination that the
    /// program sent to). The source port is the port of the socket. A
    /// `source` of the other family than `remote` is `InvalidInput`.
    pub async fn send_to_from(&self, buf: &[u8], remote: impl Into<SocketAddr>, source: IpAddr) -> io::Result<usize> {
        self.queue(buf, remote.into(), Some(source)).await
    }

    async fn queue(&self, buf: &[u8], to: SocketAddr, from: Option<IpAddr>) -> io::Result<usize> {
        one_family(from.unwrap_or(self.local.ip()), to)?;
        self.send_tx.send(Outbound { data: buf.to_vec(), to, from }).await.map_err(|_| udp_closed())?;
        self.kick.notify_one();
        Ok(buf.len())
    }

    /// Wait for the next datagram. A datagram longer than `buf` is cut.
    pub async fn recv_from(&mut self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let (n, from, _) = self.recv_from_to(buf).await?;
        Ok((n, from))
    }

    /// Wait for the next datagram, with its source and its destination: on a
    /// socket on any address, the address that the peer sent to. A datagram
    /// longer than `buf` is cut.
    pub async fn recv_from_to(&mut self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr, SocketAddr)> {
        match self.recv_rx.recv().await {
            Some(Inbound { data, from, to }) => {
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                Ok((n, from, to))
            }
            None => Err(udp_closed()),
        }
    }
}

/// The queue of accepted connections of one listener.
pub struct TcpListener {
    pub(super) local: SocketAddr,
    pub(super) rx: mpsc::Receiver<TcpStream>,
}

impl std::fmt::Debug for TcpListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpListener").field("local", &self.local).finish()
    }
}

impl TcpListener {
    /// The bound address; `[::]:port` for a listener on any address.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    pub async fn accept(&mut self) -> io::Result<TcpStream> {
        self.rx.recv().await.ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "the listener is closed"))
    }
}

/// The live and the highest socket counts of one stack.
///
/// `live` is the count of the sockets that the reactor holds now, `peak` the
/// most at one time. A `peak` that rises with each connection while `live`
/// goes back to its start shows a set that the reactor does not prune.
#[derive(Debug, Default)]
pub struct SocketCount {
    pub(super) live: AtomicU64,
    pub(super) peak: AtomicU64,
}

impl SocketCount {
    pub(super) fn add(&self) {
        let now = self.live.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak.fetch_max(now, Ordering::Relaxed);
    }

    pub(super) fn remove(&self) {
        // Saturating: a second remove must not wrap the count to u64::MAX.
        let _ = self.live.try_update(Ordering::Relaxed, Ordering::Relaxed, |live| live.checked_sub(1));
    }

    /// The sockets that the reactor holds now.
    pub fn live(&self) -> u64 {
        self.live.load(Ordering::Relaxed)
    }

    /// The most sockets at one time.
    pub fn peak(&self) -> u64 {
        self.peak.load(Ordering::Relaxed)
    }
}
