//! The real path of the ICMP carrier over a raw socket (unix): the dialer's
//! carrier of whole ICMP messages toward one peer, and the listener that
//! reads the socket, tells one session from another by the session in the
//! echo data, and starts a session at a new session id. Both wrap their
//! carrier in [`super::echo::EchoFrames`], so the same framing runs here and
//! in the memory tests (call C276).

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_identity::PublicKey;
use tokio::sync::{Mutex as AsyncMutex, Semaphore, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

use super::echo::EchoFrames;
use super::socket::RawIcmp;
use super::{Icmp, NAME, err, wire};
use crate::arq::ArqSettings;
use crate::carrier::{Accepted, BoxIo, CarrierError, CarrierErrorKind, Listener, Pending};
use crate::datagram::{DatagramFuture, Datagrams};
use crate::dgram_tls;

/// The results of handshakes that wait for an accept.
const QUEUE: usize = 8;

/// The whole ICMP messages that wait for each session; more are lost.
const DEPTH: usize = 256;

/// The interval of the removal of the sessions that ended.
const PRUNE: Duration = Duration::from_secs(1);

/// The pause after a failed read of the socket, so the task does not spin.
const READ_PAUSE: Duration = Duration::from_millis(50);

/// The result of one handshake.
type Shaken = Result<(BoxIo, PublicKey), CarrierError>;

/// The dialer's carrier of whole ICMP messages toward one peer, over a raw
/// socket. [`EchoFrames`] wraps it.
pub(super) struct ClientIcmp {
    raw: Arc<RawIcmp>,
    peer: SocketAddr,
}

impl ClientIcmp {
    pub(super) fn new(raw: Arc<RawIcmp>, peer: SocketAddr) -> ClientIcmp {
        ClientIcmp { raw, peer }
    }
}

impl Datagrams for ClientIcmp {
    fn mtu(&self) -> usize {
        self.raw.mtu()
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        Box::pin(self.raw.send_to(datagram, self.peer))
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        Box::pin(async move {
            loop {
                let (message, from) = self.raw.recv_message().await?;
                if from.ip() == self.peer.ip() {
                    return Ok(message);
                }
            }
        })
    }
}

impl fmt::Debug for ClientIcmp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientIcmp").field("peer", &self.peer).finish_non_exhaustive()
    }
}

/// The listener's carrier of whole ICMP messages for one session: it sends
/// to the peer over the shared socket, and takes what the listener's task
/// gives it. [`EchoFrames`] wraps it.
struct SessionIcmp {
    raw: Arc<RawIcmp>,
    peer: SocketAddr,
    given: AsyncMutex<mpsc::Receiver<Vec<u8>>>,
    mtu: usize,
}

impl Datagrams for SessionIcmp {
    fn mtu(&self) -> usize {
        self.mtu
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        Box::pin(self.raw.send_to(datagram, self.peer))
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        Box::pin(async move {
            match self.given.lock().await.recv().await {
                Some(message) => Ok(message),
                None => std::future::pending().await,
            }
        })
    }
}

impl fmt::Debug for SessionIcmp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionIcmp").field("peer", &self.peer).finish_non_exhaustive()
    }
}

/// A listener of the ICMP carrier. Its drop ends each handshake under way and
/// starts no new session; the task gives the messages of the sessions that
/// it gave until each of them ends.
pub struct IcmpListener {
    local: Ipv4Addr,
    results: mpsc::Receiver<Shaken>,
    _stop: oneshot::Sender<()>,
}

impl fmt::Debug for IcmpListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IcmpListener").field("local", &self.local).finish_non_exhaustive()
    }
}

#[derive(Clone)]
struct Shake {
    acceptor: TlsAcceptor,
    alpn: &'static [u8],
    arq: ArqSettings,
    within: Duration,
    mtu: usize,
}

pub(super) fn bind(icmp: &Icmp, ip: Ipv4Addr) -> Result<IcmpListener, CarrierError> {
    if icmp.max_handshakes == 0 {
        return Err(err(CarrierErrorKind::Usage, "max_handshakes is 0: give 1 or more"));
    }
    let mtu = icmp.icmp.payload + wire::HEADER + wire::SESSION;
    let raw = RawIcmp::open(mtu).map_err(icmp_open)?;
    raw.bind(ip).map_err(|e| CarrierError::io(NAME, format!("bind {ip}"), e))?;
    let local = raw.local_ip().unwrap_or(ip);
    let shake = Shake {
        acceptor: dgram_tls::acceptor(NAME, &icmp.provider, &icmp.identity, &icmp.clients, icmp.alpn)?,
        alpn: icmp.alpn,
        arq: icmp.arq,
        within: icmp.handshake_timeout,
        mtu,
    };
    let (out, results) = mpsc::channel(QUEUE);
    let (stop, stopped) = oneshot::channel();
    let limit = Arc::new(Semaphore::new(icmp.max_handshakes));
    tokio::spawn(serve(Arc::new(raw), shake, limit, out, stopped));
    Ok(IcmpListener { local, results, _stop: stop })
}

/// A failure to open a raw socket, with the reason and the privilege.
pub(super) fn icmp_open(e: io::Error) -> CarrierError {
    let kind = if e.kind() == io::ErrorKind::PermissionDenied {
        CarrierErrorKind::Unavailable
    } else {
        CarrierErrorKind::Io(e.kind())
    };
    CarrierError::with_source(NAME, kind, "a raw ICMP socket needs CAP_NET_RAW or uid 0", e)
}

impl IcmpListener {
    pub fn local_ip(&self) -> Ipv4Addr {
        self.local
    }

    /// The next client whose key is pinned, and that key.
    pub async fn accept_key(&mut self) -> Result<(BoxIo, PublicKey), CarrierError> {
        match self.results.recv().await {
            Some(result) => result,
            None => Err(err(CarrierErrorKind::Io(io::ErrorKind::BrokenPipe), "the task of the listener ended")),
        }
    }
}

impl Listener for IcmpListener {
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

/// Read the socket, give each echo request to its session, and start a
/// session at a new session id.
async fn serve(
    raw: Arc<RawIcmp>,
    shake: Shake,
    limit: Arc<Semaphore>,
    out: mpsc::Sender<Shaken>,
    mut stop: oneshot::Receiver<()>,
) {
    let mut sessions: HashMap<(IpAddr, u32), mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut shakes = JoinSet::new();
    let mut stopped = false;
    let mut prune = tokio::time::interval(PRUNE);
    prune.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            got = raw.recv_message() => {
                let (message, from) = match got {
                    Ok(got) => got,
                    Err(e) => {
                        tracing::debug!(error = %e, "a read of the socket of an icmp listener failed");
                        tokio::time::sleep(READ_PAUSE).await;
                        continue;
                    }
                };
                let Some(echo) = wire::decode(&message) else { continue };
                // Only the dialer's echo requests start or feed a session; a
                // reply (the listener's own, or the kernel's to a request) is
                // dropped by the mark.
                if echo.kind != wire::REQUEST || echo.from != wire::CLIENT {
                    continue;
                }
                let key = (from.ip(), echo.session);
                if let Some(given) = sessions.get(&key) {
                    match given.try_send(message) {
                        Err(mpsc::error::TrySendError::Closed(_)) => drop(sessions.remove(&key)),
                        _ => continue,
                    }
                    continue;
                }
                if stopped {
                    continue;
                }
                let Ok(permit) = limit.clone().try_acquire_owned() else { continue };
                let (given, taken) = mpsc::channel(DEPTH);
                let _ = given.try_send(message);
                sessions.insert(key, given);
                let session = SessionIcmp { raw: raw.clone(), peer: from, given: AsyncMutex::new(taken), mtu: shake.mtu };
                let frames = EchoFrames::server(session, echo.session, echo.session as u16);
                let (shake, out) = (shake.clone(), out.clone());
                shakes.spawn(async move {
                    let _permit = permit;
                    let result =
                        dgram_tls::accept(NAME, frames, shake.acceptor, shake.alpn, shake.arq, &from, shake.within).await;
                    let _ = out.send(result).await;
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
