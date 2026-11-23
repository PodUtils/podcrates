//! The server half of the NTP carrier's listener: a UDP socket on port 123
//! (or the port of the caller) that answers each request of a client with
//! the bytes that the listener end wrote, and gives the bytes of the request
//! to that end. Each source address is one peer.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::carrier::{BoxIo, CarrierError, CarrierErrorKind, Listener, Pending};

use super::NAME;
use super::session::MAX_PAYLOAD;
use super::wire::{self, Timestamp, Types};

/// The accept side of the NTP carrier: one peer for each source address
/// that sends a request after the bind.
pub struct NtpListener {
    /// The address of the socket.
    pub local_addr: SocketAddr,
    accept: mpsc::Receiver<tokio::io::DuplexStream>,
}

impl NtpListener {
    pub(super) fn new(local_addr: SocketAddr, accept: mpsc::Receiver<tokio::io::DuplexStream>) -> NtpListener {
        NtpListener { local_addr, accept }
    }
}

impl Listener for NtpListener {
    fn address(&self) -> String {
        self.local_addr.to_string()
    }

    fn accept(&mut self) -> Pending<'_, BoxIo> {
        Box::pin(async move {
            match self.accept.recv().await {
                Some(io) => Ok(Box::new(io) as BoxIo),
                None => Err(CarrierError::new(NAME, CarrierErrorKind::Unavailable, "the listener is closed")),
            }
        })
    }
}

/// Answer each request of each peer until the socket fails.
pub(super) async fn run(socket: Arc<UdpSocket>, accept: mpsc::Sender<tokio::io::DuplexStream>, ring: usize) {
    let mut peers: HashMap<SocketAddr, tokio::io::DuplexStream> = HashMap::new();
    let mut types = Types::new(seed());
    let mut buffer = vec![0u8; 2048];
    loop {
        let (bytes, from) = match socket.recv_from(&mut buffer).await {
            Ok(pair) => pair,
            Err(_) => return,
        };
        if let std::collections::hash_map::Entry::Vacant(slot) = peers.entry(from) {
            let (near, far) = tokio::io::duplex(ring);
            if accept.send(near).await.is_err() {
                return;
            }
            slot.insert(far);
        }
        let Some(end) = peers.get_mut(&from) else { continue };
        let Ok(request) = wire::parse(&buffer[..bytes]) else { continue };
        if request.mode != wire::MODE_CLIENT || request.version != wire::VERSION {
            continue;
        }
        // The bytes up go to the listener end. A full pipe takes no reply,
        // so the client gives the same bytes again at its next poll.
        let payload = request.payload();
        let mut rest = payload.as_slice();
        let mut full = false;
        while !rest.is_empty() {
            match now_write(end, rest) {
                Ok(0) => {
                    full = true;
                    break;
                }
                Ok(taken) => rest = &rest[taken..],
                Err(_) => {
                    full = true;
                    break;
                }
            }
        }
        if full {
            continue;
        }
        let mut value = vec![0u8; MAX_PAYLOAD.min(1024)];
        let read = now_read(end, &mut value).unwrap_or(0);
        let down = value[..read].to_vec();
        let at = Timestamp::now();
        let kind = types.take();
        let answered = wire::reply(&request, at, at, at, 2, kind, down);
        if let Ok(bytes) = answered.encode() {
            let _ = socket.send_to(&bytes, from).await;
        }
    }
}

/// One write that the peer takes now, without a wait: `WouldBlock` when its
/// pipe is full.
fn now_write<W: AsyncWrite + Unpin>(io: &mut W, bytes: &[u8]) -> io::Result<usize> {
    let mut context = Context::from_waker(Waker::noop());
    match Pin::new(io).poll_write(&mut context, bytes) {
        Poll::Ready(result) => result,
        Poll::Pending => Err(io::ErrorKind::WouldBlock.into()),
    }
}

/// One read that the peer gives now, without a wait: an empty value when its
/// pipe is empty.
fn now_read<R: AsyncRead + Unpin>(io: &mut R, out: &mut [u8]) -> io::Result<usize> {
    let mut context = Context::from_waker(Waker::noop());
    let mut buffer = ReadBuf::new(out);
    match Pin::new(io).poll_read(&mut context, &mut buffer) {
        Poll::Ready(Ok(())) => Ok(buffer.filled().len()),
        Poll::Ready(Err(e)) => Err(e),
        Poll::Pending => Err(io::ErrorKind::WouldBlock.into()),
    }
}

/// A seed for the types of the server.
fn seed() -> u16 {
    use pod_core::entropy::{Entropy, OsEntropy};
    let mut bytes = [0u8; 2];
    OsEntropy.fill(&mut bytes).map(|()| u16::from_be_bytes(bytes)).unwrap_or(0x5a5a)
}
