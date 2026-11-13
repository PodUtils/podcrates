//! The datagrams of one peer over a socket of many: a dial takes the
//! datagrams of its peer's address from its own socket; a session of a
//! listener takes the datagrams that the listener's task gives it.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::{Mutex, mpsc};

use crate::datagram::{DatagramFuture, Datagrams};

/// The largest datagram that a socket reads.
pub(super) const MAX_DATAGRAM: usize = 65535;

/// The datagrams of a dial: its own socket, toward one peer. A datagram of
/// another address is dropped.
pub(super) struct Toward {
    pub(super) socket: Arc<dyn pod_core::Datagrams>,
    pub(super) peer: SocketAddr,
    pub(super) mtu: usize,
}

impl Datagrams for Toward {
    fn mtu(&self) -> usize {
        self.mtu
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        Box::pin(pod_core::datagrams::send_to(&*self.socket, datagram, self.peer))
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        Box::pin(async move {
            let mut buf = vec![0u8; MAX_DATAGRAM];
            loop {
                let (n, from) = pod_core::datagrams::recv_from(&*self.socket, &mut buf).await?;
                if from == self.peer {
                    buf.truncate(n);
                    return Ok(buf);
                }
            }
        })
    }
}

/// The datagrams of one session of a listener: it sends from the
/// listener's socket, and takes what the task of the listener gives it.
/// When the listener is gone, the peer is silence.
pub(super) struct Session {
    pub(super) socket: Arc<dyn pod_core::Datagrams>,
    pub(super) peer: SocketAddr,
    pub(super) mtu: usize,
    pub(super) given: Mutex<mpsc::Receiver<Vec<u8>>>,
}

impl Datagrams for Session {
    fn mtu(&self) -> usize {
        self.mtu
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        Box::pin(pod_core::datagrams::send_to(&*self.socket, datagram, self.peer))
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        Box::pin(async move {
            match self.given.lock().await.recv().await {
                Some(datagram) => Ok(datagram),
                None => std::future::pending().await,
            }
        })
    }
}
