//! The accept side of the udp carrier. One task reads the socket and gives
//! the datagrams of each address and conversation to its session. The
//! first segment of a new conversation starts a session and its handshake
//! in a task of its own, at most `max_handshakes` at once: a peer that
//! stops holds only its own task, until its time limit. The result of each
//! handshake waits in a queue for [`UdpListener::accept_key`].

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_identity::PublicKey;
use pod_tls::peer;
use tokio::io::AsyncWriteExt as _;
use tokio::sync::{Mutex, Semaphore, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

use super::ends::{MAX_DATAGRAM, Session};
use super::settings::{KCP_HEADER, KcpSettings};
use super::stream::KcpStream;
use super::{ACCEPTED, NAME, Udp, err};
use crate::carrier::{Accepted, BoxIo, CarrierError, CarrierErrorKind, Listener, Pending};
use crate::pin;

/// The result of one handshake.
type Shaken = Result<(BoxIo, PublicKey), CarrierError>;

/// The results of handshakes that wait for an accept. A handshake that
/// ended waits with its permit, so the streams that wait are at most
/// `max_handshakes`.
const QUEUE: usize = 8;

/// The datagrams that wait for each session; more are lost.
const DEPTH: usize = 256;

/// The interval of the removal of the sessions that ended.
const PRUNE: Duration = Duration::from_secs(1);

/// The pause after a failed read of the socket, so that the task does not
/// spin.
const READ_PAUSE: Duration = Duration::from_millis(50);

/// The command of a KCP segment with data.
const PUSH: u8 = 81;

/// A listener of the udp carrier. Its drop ends each handshake under way
/// and starts no new session; the task gives the datagrams of the sessions
/// that it gave until each of them ends.
pub struct UdpListener {
    local: SocketAddr,
    results: mpsc::Receiver<Shaken>,
    /// Its drop tells the task to start no session.
    _stop: oneshot::Sender<()>,
}

impl fmt::Debug for UdpListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UdpListener").field("local", &self.local).finish_non_exhaustive()
    }
}

/// What each session of a listener needs.
#[derive(Clone)]
struct Shake {
    acceptor: TlsAcceptor,
    kcp: KcpSettings,
    alpn: &'static [u8],
    within: Duration,
}

/// The TLS of a listener of `udp`.
pub(super) fn acceptor(udp: &Udp) -> Result<TlsAcceptor, CarrierError> {
    let config = peer::server_config(udp.provider.clone(), udp.identity.clone(), udp.clients.clone(), &[udp.alpn])
        .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
    Ok(TlsAcceptor::from(config))
}

pub(super) fn bind(udp: &Udp, socket: Arc<dyn pod_core::Datagrams>) -> Result<UdpListener, CarrierError> {
    if udp.max_handshakes == 0 {
        return Err(err(CarrierErrorKind::Usage, "max_handshakes is 0: give 1 or more"));
    }
    let shake = Shake { acceptor: acceptor(udp)?, kcp: udp.kcp, alpn: udp.alpn, within: udp.handshake_timeout };
    let local = socket.local_addr().map_err(|e| CarrierError::io(NAME, "the address of the socket", e))?;
    let (out, results) = mpsc::channel(QUEUE);
    let (stop, stopped) = oneshot::channel();
    let limit = Arc::new(Semaphore::new(udp.max_handshakes));
    // The task is not aborted: it ends by itself when no session is left.
    tokio::spawn(serve(socket, shake, limit, out, stopped));
    Ok(UdpListener { local, results, _stop: stop })
}

impl UdpListener {
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

impl Listener for UdpListener {
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

/// The first segment of a conversation: data with the sequence 0.
fn first_segment(datagram: &[u8]) -> bool {
    datagram.len() >= KCP_HEADER && datagram[4] == PUSH && datagram[12..16] == [0, 0, 0, 0]
}

/// Read the socket, give each datagram to its session, and start a
/// session at the first segment of a new conversation.
async fn serve(
    socket: Arc<dyn pod_core::Datagrams>,
    shake: Shake,
    limit: Arc<Semaphore>,
    out: mpsc::Sender<Shaken>,
    mut stop: oneshot::Receiver<()>,
) {
    let mut sessions: HashMap<(SocketAddr, u32), mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut shakes = JoinSet::new();
    let mut stopped = false;
    let mut buf = vec![0u8; MAX_DATAGRAM];
    let mut prune = tokio::time::interval(PRUNE);
    prune.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            got = pod_core::datagrams::recv_from(&*socket, &mut buf) => {
                let (n, from) = match got {
                    Ok(got) => got,
                    Err(e) => {
                        tracing::debug!(error = %e, "a read of the socket of a udp listener failed");
                        tokio::time::sleep(READ_PAUSE).await;
                        continue;
                    }
                };
                let datagram = &buf[..n];
                if n < KCP_HEADER {
                    continue;
                }
                let conv = u32::from_le_bytes([datagram[0], datagram[1], datagram[2], datagram[3]]);
                if let Some(given) = sessions.get(&(from, conv)) {
                    match given.try_send(datagram.to_vec()) {
                        Err(mpsc::error::TrySendError::Closed(_)) => drop(sessions.remove(&(from, conv))),
                        // A full queue loses the datagram, as UDP does.
                        _ => continue,
                    }
                }
                if stopped || !first_segment(datagram) {
                    continue;
                }
                // At the limit the first segment is lost: the dialer sends
                // it again.
                let Ok(permit) = limit.clone().try_acquire_owned() else { continue };
                let (given, taken) = mpsc::channel(DEPTH);
                let _ = given.try_send(datagram.to_vec());
                sessions.insert((from, conv), given);
                let session = Session { socket: socket.clone(), peer: from, mtu: shake.kcp.mtu, given: Mutex::new(taken) };
                let (shake, out) = (shake.clone(), out.clone());
                shakes.spawn(async move {
                    let result = match KcpStream::new(session, Some(conv), shake.kcp) {
                        Ok(stream) => handshake(shake.acceptor, stream, shake.alpn, &from, shake.within).await,
                        Err(e) => Err(err(CarrierErrorKind::Usage, format!("KCP: {e}"))),
                    };
                    let _ = out.send(result).await;
                    drop(permit);
                });
            }
            _ = &mut stop, if !stopped => {
                stopped = true;
                shakes.abort_all();
            }
            _ = prune.tick() => {
                sessions.retain(|_, given| !given.is_closed());
                while shakes.try_join_next().is_some() {}
                if stopped && sessions.is_empty() {
                    return;
                }
            }
        }
    }
}

/// The TLS handshake of one client over KCP, its ALPN, its key, and the
/// byte of acceptance, within `within`.
pub(super) async fn handshake(
    acceptor: TlsAcceptor,
    stream: KcpStream,
    alpn: &[u8],
    from: &(dyn fmt::Display + Sync),
    within: Duration,
) -> Shaken {
    let run = async {
        let mut tls = acceptor
            .accept(stream)
            .await
            .map_err(|e| pin::tls_error(NAME, format!("the TLS handshake of {from}"), e))?;
        let conn = tls.get_ref().1;
        pin::check_alpn(NAME, conn.alpn_protocol(), alpn, from)?;
        let key = pin::proved(NAME, conn.peer_certificates(), from)?;
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
            format!("the handshake of {from} took more than {within:?}"),
        ))
    })
}
