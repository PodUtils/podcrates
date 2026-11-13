//! The accept side of the tcp carrier. One task accepts the TCP
//! connections and runs the TLS handshake of each in a task of its own, at
//! most `max_handshakes` at once: a peer that sends nothing holds only its
//! own handshake, until its time limit. The result of each handshake waits
//! in a queue for [`TlsListener::accept_key`].

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_identity::PublicKey;
use pod_tls::peer;
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio_rustls::TlsAcceptor;

use super::{ACCEPTED, NAME, Tcp, check_alpn, err, proved, tls_error};
use crate::carrier::{Accepted, BoxIo, CarrierError, CarrierErrorKind, Listener, Pending};

/// The result of one handshake.
type Shaken = Result<(BoxIo, PublicKey), CarrierError>;

/// The results of handshakes that wait for an accept. A handshake that
/// ended waits with its permit, so the streams that wait are at most
/// `max_handshakes`.
const QUEUE: usize = 8;

/// The pause after a failed accept of a TCP connection (no file
/// descriptor left, for example), so that the task does not spin.
const ACCEPT_PAUSE: Duration = Duration::from_millis(50);

/// A listener of the tcp carrier. Its drop ends the task of its accepts
/// and each handshake under way.
pub struct TlsListener {
    local: SocketAddr,
    results: mpsc::Receiver<Shaken>,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for TlsListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsListener").field("local", &self.local).finish_non_exhaustive()
    }
}

pub(super) async fn bind(tcp: &Tcp, addr: SocketAddr) -> Result<TlsListener, CarrierError> {
    if tcp.max_handshakes == 0 {
        return Err(err(CarrierErrorKind::Usage, "max_handshakes is 0: give 1 or more"));
    }
    let config = peer::server_config(tcp.provider.clone(), tcp.identity.clone(), tcp.clients.clone(), &[tcp.alpn])
        .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
    let listener =
        tokio::net::TcpListener::bind(addr).await.map_err(|e| CarrierError::io(NAME, format!("bind {addr}"), e))?;
    let local = listener.local_addr().map_err(|e| CarrierError::io(NAME, format!("the address of {addr}"), e))?;
    let (out, results) = mpsc::channel(QUEUE);
    let limit = Arc::new(Semaphore::new(tcp.max_handshakes));
    let task = tokio::spawn(serve(listener, TlsAcceptor::from(config), limit, tcp.handshake_timeout, out));
    Ok(TlsListener { local, results, task })
}

impl TlsListener {
    /// The bound address: the port that the system chose for port 0.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// The next client whose key is pinned, and that key. A client that
    /// failed its handshake is an error of one accept: the listener stays
    /// open.
    pub async fn accept_key(&mut self) -> Result<(BoxIo, PublicKey), CarrierError> {
        match self.results.recv().await {
            Some(result) => result,
            None => Err(err(CarrierErrorKind::Io(io::ErrorKind::BrokenPipe), "the task of the listener ended")),
        }
    }
}

impl Listener for TlsListener {
    fn address(&self) -> String {
        self.local.to_string()
    }

    fn accept(&mut self) -> Pending<'_, BoxIo> {
        Box::pin(async move { self.accept_key().await.map(|(io, _)| io) })
    }

    fn accept_peer(&mut self) -> Pending<'_, Accepted> {
        Box::pin(async move { self.accept_key().await.map(|(io, key)| Accepted { io, peer: Some(key) }) })
    }
}

impl Drop for TlsListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Accept the TCP connections and shake hands with each, at most `limit`
/// at once. The drop of this task drops the handshakes under way.
async fn serve(
    listener: tokio::net::TcpListener,
    acceptor: TlsAcceptor,
    limit: Arc<Semaphore>,
    within: Duration,
    out: mpsc::Sender<Shaken>,
) {
    let mut shakes = JoinSet::new();
    loop {
        let Ok(permit) = limit.clone().acquire_owned().await else { return };
        while shakes.try_join_next().is_some() {}
        match listener.accept().await {
            Ok((tcp, from)) => {
                let (acceptor, out) = (acceptor.clone(), out.clone());
                shakes.spawn(async move {
                    let result = handshake(acceptor, tcp, from, within).await;
                    let _ = out.send(result).await;
                    drop(permit);
                });
            }
            Err(e) => {
                drop(permit);
                if out.send(Err(CarrierError::io(NAME, "accept", e))).await.is_err() {
                    return;
                }
                tokio::time::sleep(ACCEPT_PAUSE).await;
            }
        }
    }
}

/// The TLS handshake of one client, its ALPN, its key, and the byte of
/// acceptance, within `within`.
async fn handshake(acceptor: TlsAcceptor, tcp: TcpStream, from: SocketAddr, within: Duration) -> Shaken {
    // The frames of a link are small; Nagle would hold a PING behind them.
    let _ = tcp.set_nodelay(true);
    let run = async {
        let mut tls = acceptor.accept(tcp).await.map_err(|e| tls_error(format!("the TLS handshake of {from}"), e))?;
        let conn = tls.get_ref().1;
        // The configuration offers the one ALPN of the carrier.
        let want = acceptor.config().alpn_protocols.first().map_or(&[][..], Vec::as_slice);
        check_alpn(conn.alpn_protocol(), want, &from)?;
        let key = proved(conn.peer_certificates(), &from)?;
        let said = async {
            tls.write_all(&[ACCEPTED]).await?;
            tls.flush().await
        };
        said.await.map_err(|e| CarrierError::io(NAME, format!("the acceptance of {from}"), e))?;
        Ok((Box::new(tls) as BoxIo, key))
    };
    tokio::time::timeout(within, run).await.unwrap_or_else(|_| {
        Err(err(
            CarrierErrorKind::Io(io::ErrorKind::TimedOut),
            format!("the TLS handshake of {from} took more than {within:?}"),
        ))
    })
}
