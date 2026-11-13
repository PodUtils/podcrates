//! Two ends of a datagram carrier in one process. It acts as UDP: a full
//! queue or a gone peer drops the datagram, and a gone peer is silence.

use std::io;

use tokio::sync::{Mutex, mpsc};

use super::{DatagramFuture, Datagrams};

/// The unit of the pair by default: 1200 bytes, podnet's MTU of UDP.
pub const DEFAULT_MTU: usize = 1200;

/// The datagrams that wait in each direction by default.
pub const DEFAULT_DEPTH: usize = 1024;

/// One end of a memory datagram pair.
pub struct MemoryDatagrams {
    mtu: usize,
    tx: mpsc::Sender<Vec<u8>>,
    rx: Mutex<mpsc::Receiver<Vec<u8>>>,
}

/// Two connected ends: datagrams of at most `mtu` bytes, and at most
/// `depth` of them waiting in each direction.
pub fn pair(mtu: usize, depth: usize) -> (MemoryDatagrams, MemoryDatagrams) {
    let (a_tx, a_rx) = mpsc::channel(depth.max(1));
    let (b_tx, b_rx) = mpsc::channel(depth.max(1));
    (MemoryDatagrams { mtu, tx: a_tx, rx: Mutex::new(b_rx) }, MemoryDatagrams { mtu, tx: b_tx, rx: Mutex::new(a_rx) })
}

impl Datagrams for MemoryDatagrams {
    fn mtu(&self) -> usize {
        self.mtu
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        Box::pin(async move {
            if datagram.len() > self.mtu {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("a datagram of {} bytes, over the unit of {}", datagram.len(), self.mtu),
                ));
            }
            // A full queue or a gone peer loses the datagram, as UDP does.
            let _ = self.tx.try_send(datagram.to_vec());
            Ok(())
        })
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        Box::pin(async move {
            let mut rx = self.rx.lock().await;
            match rx.recv().await {
                Some(datagram) => Ok(datagram),
                // The peer is gone: no datagram comes again.
                None => std::future::pending().await,
            }
        })
    }
}

impl std::fmt::Debug for MemoryDatagrams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryDatagrams").field("mtu", &self.mtu).finish_non_exhaustive()
    }
}
