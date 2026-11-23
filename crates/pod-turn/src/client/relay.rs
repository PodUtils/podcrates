//! The TCP relay of RFC 6062: a Connect on the control connection, the
//! CONNECTION-ID of its success, a second TCP connection bound with
//! ConnectionBind, and the peer's bytes on it. A ConnectionAttempt
//! indication takes the same road. The control connection stays open: the
//! stream's keeper refreshes the allocation on its timer (RFC 6062, 4.6;
//! PodUtils/podnet#485, TURN-2).
//!
//! The data connection is the bare TCP stream of RFC 6062, 4.5: the bytes
//! that the peer sends are the bytes that the caller reads, and the caller's
//! writes go to the peer unchanged. `stun` stays the only codec: the relay
//! reuses its messages.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use pod_core::BoxedIo;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::task::JoinHandle;

use crate::stun::{
    self, CONNECT, CONNECTION_ATTEMPT, CONNECTION_BIND, CONNECTION_ID, Class, XOR_PEER_ADDRESS, class_of, message_type,
    method_of,
};

use super::exchange::{self, Value};
use super::keep::Keeping;
use super::wire::{Conn, Frame};
use super::{Client, Error, Relay, open};

/// The most bytes that one [`RelayedStream::read_frame`] returns.
const CHUNK: usize = 64 * 1024;

/// What [`crate::Client::status`] and [`RelayedStream::status`] report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// The lifetime that the server gave at the last Allocate or Refresh.
    pub lifetime: Duration,
    /// The caller's rate limit of the relay, in bytes per second. TURN
    /// carries no such attribute; `None` when the caller names none.
    pub rate_limit: Option<u64>,
}

/// A relayed TCP connection through a TURN allocation (RFC 6062).
///
/// It is a bare byte stream in both directions, as RFC 6062, 4.5 specifies.
/// [`RelayedStream::read_frame`] gives one read's worth of bytes and
/// [`RelayedStream::write_frame`] writes them unchanged. The allocation is
/// refreshed while the stream lives, and ends when the stream is dropped.
pub struct RelayedStream {
    io: BoxedIo,
    /// Bytes read from `io` and not yet given to the caller.
    buffered: Vec<u8>,
    peer: SocketAddr,
    status: Arc<Mutex<Status>>,
    /// Aborted when the stream is dropped: the allocation ends with it.
    _keeper: Keeper,
}

/// The allocation's keeper: the control connection, its refresh timer, and
/// the task that owns both.
struct Keeper {
    task: JoinHandle<()>,
}

impl Drop for Keeper {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Client {
    /// Open a TCP connection to `peer` through a TCP allocation
    /// (RFC 6062, 4.3): a Connect with XOR-PEER-ADDRESS, its CONNECTION-ID,
    /// a second connection to the same server, and a ConnectionBind there.
    /// The returned stream refreshes the allocation on its timer. The
    /// client is consumed: one stream owns its allocation.
    pub async fn connect(mut self, peer: SocketAddr) -> Result<RelayedStream, Error> {
        self.live()?;
        if self.relay != Relay::Tcp {
            return Err(Error::NoTcpAllocation);
        }
        let attributes = [(XOR_PEER_ADDRESS, Value::Peer(peer))];
        let reply = match self.transact(CONNECT, &attributes).await {
            Ok(reply) => reply,
            Err(Error::Server { code, reason, .. }) => return Err(Error::ConnectRefused { code, reason }),
            Err(error) => return Err(error),
        };
        let read = stun::decode(&reply).map_err(Error::Decode)?;
        let id = connection_id(&read)?;
        let data = self.open_data().await?;
        let data = self.bind_data(data, id).await?;
        self.relay(data, peer)
    }

    /// Accept a TCP connection that a peer opened to the relayed address
    /// (RFC 6062, 4.4): wait for a ConnectionAttempt indication, then open
    /// the second connection and bind its CONNECTION-ID. A permission for
    /// the peer's address must exist, else the server opens nothing.
    pub async fn accept(mut self) -> Result<RelayedStream, Error> {
        self.live()?;
        if self.relay != Relay::Tcp {
            return Err(Error::NoTcpAllocation);
        }
        let wait = self.limits.connection_attempt_timeout;
        let deadline = self.clock.now() + wait;
        let (id, peer) = loop {
            let frame = tokio::select! {
                frame = self.conn.recv() => frame?,
                () = self.clock.sleep_until(deadline) => {
                    return Err(Error::ConnectionAttemptTimeout { waited: wait });
                }
            };
            let found = match &frame {
                Frame::Stun(bytes) => connection_attempt(bytes)?,
                Frame::Channel { .. } => None,
            };
            match found {
                Some((id, peer)) => break (id, peer),
                None => self.incoming(frame),
            }
        };
        let data = self.open_data().await?;
        let data = self.bind_data(data, id).await?;
        self.relay(data, peer)
    }

    /// The second connection to the server: the same transport and the same
    /// server as the control connection (RFC 6062, 4.3).
    async fn open_data(&self) -> Result<Conn, Error> {
        let deadline = self.clock.now() + self.limits.connect_timeout;
        let link = open::connect(
            &self.server,
            &self.transport,
            self.tls_name.as_deref(),
            deadline,
            self.limits.connect_timeout,
        )
        .await?;
        if link.conn.server().is_some() {
            return Err(Error::Usage("RFC 6062 needs a byte stream, not UDP".into()));
        }
        Ok(link.conn)
    }

    /// Bind `conn` with `id` (RFC 6062, 4.3 and 4.4): a ConnectionBind
    /// signed with the allocation's key. A stale nonce (438) goes again
    /// with the server's fresh one.
    async fn bind_data(&mut self, mut conn: Conn, id: u32) -> Result<Conn, Error> {
        let mut stale = 0u32;
        loop {
            let txid = self.new_id()?;
            let value = id.to_be_bytes().to_vec();
            let attributes = [(CONNECTION_ID, Value::Bytes(value))];
            let request = exchange::request_with(
                self.auth.as_ref(),
                self.credentials.username(),
                self.software.as_deref(),
                CONNECTION_BIND,
                &txid,
                &attributes,
            );
            let reply =
                exchange::exchange_on(&mut conn, &request, &txid, self.clock.as_ref(), &self.limits, CONNECTION_BIND)
                    .await?;
            let read = stun::decode(&reply).map_err(Error::Decode)?;
            let kind = read.kind;
            if kind == message_type(CONNECTION_BIND, Class::Success) {
                exchange::check_with(self.auth.as_ref(), &reply, kind)?;
                return Ok(conn);
            }
            if kind != message_type(CONNECTION_BIND, Class::Error) {
                let why = "an answer of another type to a ConnectionBind request".to_string();
                return Err(Error::Protocol { kind, why });
            }
            let (code, reason) = match read.error() {
                Some(Ok(error)) => error,
                Some(Err(why)) => return Err(Error::Protocol { kind, why }),
                None => return Err(Error::Protocol { kind, why: "an error answer with no ERROR-CODE".into() }),
            };
            if code == 438 && stale < self.limits.stale_nonces {
                let Some(nonce) = read.attribute(stun::NONCE) else {
                    return Err(Error::BindRefused { code, reason });
                };
                if let Some(auth) = self.auth.as_mut() {
                    auth.nonce = nonce.to_vec();
                }
                stale += 1;
                continue;
            }
            return Err(Error::BindRefused { code, reason });
        }
    }

    /// The stream of a bound data connection, with the keeper of the
    /// control connection and the allocation.
    fn relay(mut self, data: Conn, peer: SocketAddr) -> Result<RelayedStream, Error> {
        let auth = self.auth.take().ok_or(Error::NoAllocation)?;
        let (io, buffered) = data.into_stream()?;
        let status = Arc::new(Mutex::new(Status { lifetime: self.lifetime, rate_limit: self.rate_limit }));
        let task = Keeping {
            control: self.conn,
            auth,
            username: self.credentials.username().to_string(),
            software: self.software.clone(),
            clock: self.clock.clone(),
            limits: self.limits.clone(),
            lifetime: self.lifetime,
            granted: self.granted,
            entropy: self.entropy,
            status: status.clone(),
        }
        .spawn();
        let keeper = Keeper { task };
        Ok(RelayedStream { io, buffered, peer, status, _keeper: keeper })
    }
}

impl std::fmt::Debug for RelayedStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayedStream")
            .field("peer", &self.peer)
            .field("status", &self.status())
            .field("buffered", &self.buffered.len())
            .finish_non_exhaustive()
    }
}

impl RelayedStream {
    /// The peer of the relayed connection.
    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// The allocation lifetime and the relay's rate limit.
    pub fn status(&self) -> Status {
        *self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The next bytes of the relayed stream: one read, at most
    /// `CHUNK` bytes. An end between reads is [`Error::RelayedClosed`].
    pub async fn read_frame(&mut self) -> Result<Vec<u8>, Error> {
        if !self.buffered.is_empty() {
            return Ok(std::mem::take(&mut self.buffered));
        }
        let mut chunk = vec![0u8; CHUNK];
        let got = self.io.read(&mut chunk).await.map_err(Error::Io)?;
        if got == 0 {
            return Err(Error::RelayedClosed);
        }
        chunk.truncate(got);
        Ok(chunk)
    }

    /// Write bytes to the relayed stream unchanged.
    pub async fn write_frame(&mut self, data: &[u8]) -> Result<(), Error> {
        self.io.write_all(data).await.map_err(Error::Io)
    }

    /// Flush the relayed stream. RFC 6062 has no wire keepalive on the data
    /// connection; the allocation's refresh is the keepalive.
    pub async fn write_keepalive(&mut self) -> Result<(), Error> {
        self.io.flush().await.map_err(Error::Io)
    }
}

impl AsyncRead for RelayedStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if !self.buffered.is_empty() {
            let n = self.buffered.len().min(buf.remaining());
            buf.put_slice(&self.buffered[..n]);
            self.buffered.drain(..n);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl AsyncWrite for RelayedStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

/// The CONNECTION-ID and peer of a ConnectionAttempt indication.
fn connection_attempt(bytes: &[u8]) -> Result<Option<(u32, SocketAddr)>, Error> {
    let read = stun::decode(bytes).map_err(Error::Decode)?;
    if class_of(read.kind) != Class::Indication || method_of(read.kind) != CONNECTION_ATTEMPT {
        return Ok(None);
    }
    let id = connection_id(&read)?;
    let value = read
        .attribute(XOR_PEER_ADDRESS)
        .ok_or_else(|| Error::BadConnectionId { why: "a ConnectionAttempt with no XOR-PEER-ADDRESS".into() })?;
    let peer = stun::read_address(value, Some(&read.id))
        .map_err(|why| Error::BadConnectionId { why: format!("XOR-PEER-ADDRESS: {why}") })?;
    Ok(Some((id, peer)))
}

fn connection_id(read: &stun::Message<'_>) -> Result<u32, Error> {
    let value =
        read.attribute(CONNECTION_ID).ok_or_else(|| Error::BadConnectionId { why: "no CONNECTION-ID".into() })?;
    let bytes = <[u8; 4]>::try_from(value)
        .map_err(|_| Error::BadConnectionId { why: format!("a CONNECTION-ID of {} bytes", value.len()) })?;
    Ok(u32::from_be_bytes(bytes))
}
