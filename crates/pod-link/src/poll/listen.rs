//! The accept side of the poll carriers. One task accepts the TCP
//! connections and runs the TLS handshake of each in a task of its own, at
//! most `max_handshakes` at once; the task then serves the requests of its
//! connection. An open gives a new session to [`PollListener::accept_key`];
//! a failed handshake is an error of one accept. A drop of the listener
//! ends each session that it holds.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pod_identity::PublicKey;
use pod_tls::peer;
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;

use super::session::Table;
use super::wire::{Body, Conn};
use super::{ALPN, BATCH, Poll, WINDOW, answer};
use crate::carrier::{Accepted, BoxIo, CarrierError, CarrierErrorKind, Listener, Pending};
use crate::pin;

/// A session, or a failed handshake.
pub(super) type Shaken = Result<(BoxIo, PublicKey), CarrierError>;

/// The results that wait for an accept. A result that finds the queue full
/// is dropped (a handshake) or answered 429 (an open).
const QUEUE: usize = 64;

/// The silence after which a connection with no request closes.
const KEEP_ALIVE: Duration = Duration::from_secs(30);

/// The pause after a failed accept of a TCP connection.
const ACCEPT_PAUSE: Duration = Duration::from_millis(50);

/// The counts of a listener, for a check of its work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Sessions opened.
    pub sessions: u64,
    /// Opens answered 429.
    pub busy: u64,
    /// Requests read.
    pub requests: u64,
    /// Bytes up that came again after a lost answer, and were dropped.
    pub duplicate: u64,
    /// Bytes down sent again after a cut.
    pub resent: u64,
    /// Long polls, simplex polls, pushes, and pushes answered 406.
    pub polls: u64,
    pub shorts: u64,
    pub pushes: u64,
    pub no_push: u64,
    /// The bytes down that the sessions keep now for their dialers, within
    /// `Poll::max_bytes`.
    pub held: u64,
}

#[derive(Default)]
pub(super) struct Counters {
    pub sessions: AtomicU64,
    pub busy: AtomicU64,
    pub requests: AtomicU64,
    pub duplicate: AtomicU64,
    pub resent: AtomicU64,
    pub polls: AtomicU64,
    pub shorts: AtomicU64,
    pub pushes: AtomicU64,
    pub no_push: AtomicU64,
}

impl Counters {
    pub fn add(counter: &AtomicU64, n: u64) {
        counter.fetch_add(n, Ordering::Relaxed);
    }

    fn snapshot(&self, held: usize) -> Stats {
        let get = |c: &AtomicU64| c.load(Ordering::Relaxed);
        Stats {
            sessions: get(&self.sessions),
            busy: get(&self.busy),
            requests: get(&self.requests),
            duplicate: get(&self.duplicate),
            resent: get(&self.resent),
            polls: get(&self.polls),
            shorts: get(&self.shorts),
            pushes: get(&self.pushes),
            no_push: get(&self.no_push),
            held: held as u64,
        }
    }
}

/// What the tasks of one listener share.
pub(super) struct Serving {
    pub name: &'static str,
    pub table: Table,
    pub stats: Counters,
    pub out: mpsc::Sender<Shaken>,
    pub idle: Duration,
    pub answer_timeout: Duration,
    pub push: bool,
}

/// A listener of the poll carriers. Its drop ends the task of its accepts,
/// each connection and each session.
pub struct PollListener {
    local: SocketAddr,
    name: &'static str,
    results: mpsc::Receiver<Shaken>,
    serving: Arc<Serving>,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for PollListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PollListener").field("local", &self.local).finish_non_exhaustive()
    }
}

pub(super) async fn bind(p: &Poll, addr: SocketAddr) -> Result<PollListener, CarrierError> {
    let name = p.form.name();
    let config = peer::server_config(p.provider.clone(), p.identity.clone(), p.clients.clone(), &[ALPN])
        .map_err(|e| CarrierError::new(name, CarrierErrorKind::Unavailable, e.to_string()))?;
    let listener =
        tokio::net::TcpListener::bind(addr).await.map_err(|e| CarrierError::io(name, format!("bind {addr}"), e))?;
    let local = listener.local_addr().map_err(|e| CarrierError::io(name, format!("the address of {addr}"), e))?;
    let (out, results) = mpsc::channel(QUEUE);
    let serving = Arc::new(Serving {
        name,
        table: Table::new(p.max_sessions, p.max_bytes, WINDOW, BATCH),
        stats: Counters::default(),
        out,
        idle: p.idle,
        answer_timeout: p.answer_timeout,
        push: p.push,
    });
    let limit = Arc::new(Semaphore::new(p.max_handshakes));
    let task = tokio::spawn(serve(listener, TlsAcceptor::from(config), limit, p.handshake_timeout, serving.clone()));
    Ok(PollListener { local, name, results, serving, task })
}

impl PollListener {
    /// The bound address: the port that the system chose for port 0.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// The counts of the listener.
    pub fn stats(&self) -> Stats {
        self.serving.stats.snapshot(self.serving.table.held())
    }

    /// The next session of a client whose key is pinned, and that key. A
    /// client that failed its handshake is an error of one accept: the
    /// listener stays open.
    pub async fn accept_key(&mut self) -> Result<(BoxIo, PublicKey), CarrierError> {
        match self.results.recv().await {
            Some(result) => result,
            None => Err(CarrierError::new(
                self.name,
                CarrierErrorKind::Io(io::ErrorKind::BrokenPipe),
                "the task of the listener ended",
            )),
        }
    }
}

impl Listener for PollListener {
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

impl Drop for PollListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Accept the TCP connections and serve each, with at most `limit`
/// handshakes at once; end the sessions with no request. The drop of this
/// task drops each connection and the table of the sessions.
async fn serve(
    listener: tokio::net::TcpListener,
    acceptor: TlsAcceptor,
    limit: Arc<Semaphore>,
    within: Duration,
    s: Arc<Serving>,
) {
    let mut tasks = JoinSet::new();
    let reaper = s.clone();
    tasks.spawn(async move {
        let mut tick = tokio::time::interval((reaper.idle / 4).min(Duration::from_secs(1)));
        loop {
            tick.tick().await;
            reaper.table.expire(reaper.idle);
        }
    });
    loop {
        let Ok(permit) = limit.clone().acquire_owned().await else { return };
        while tasks.try_join_next().is_some() {}
        match listener.accept().await {
            Ok((tcp, from)) => {
                tasks.spawn(connection(acceptor.clone(), tcp, from, within, permit, s.clone()));
            }
            Err(e) => {
                drop(permit);
                let _ = s.out.try_send(Err(CarrierError::io(s.name, "accept", e)));
                tokio::time::sleep(ACCEPT_PAUSE).await;
            }
        }
    }
}

/// The handshake of one connection within `within`, then its requests
/// until a silence of [`KEEP_ALIVE`] or a close.
async fn connection(
    acceptor: TlsAcceptor,
    tcp: TcpStream,
    from: SocketAddr,
    within: Duration,
    permit: OwnedSemaphorePermit,
    s: Arc<Serving>,
) {
    // The frames of a link are small; Nagle would hold a PING behind them.
    let _ = tcp.set_nodelay(true);
    let name = s.name;
    let shake = async {
        let tls =
            acceptor.accept(tcp).await.map_err(|e| pin::tls_error(name, format!("the TLS handshake of {from}"), e))?;
        let conn = tls.get_ref().1;
        pin::check_alpn(name, conn.alpn_protocol(), ALPN, &from)?;
        let key = pin::proved(name, conn.peer_certificates(), &from)?;
        Ok::<_, CarrierError>((tls, key))
    };
    let shaken = timeout(within, shake).await.unwrap_or_else(|_| {
        Err(CarrierError::new(
            name,
            CarrierErrorKind::Io(io::ErrorKind::TimedOut),
            format!("the handshake of {from} took more than {within:?}"),
        ))
    });
    drop(permit);
    let (tls, key) = match shaken {
        Ok(shaken) => shaken,
        Err(e) => {
            if let Err(e) = s.out.try_send(Err(e)) {
                tracing::debug!(error = %e, "a failed handshake of a poll listener found the queue full");
            }
            return;
        }
    };
    let mut conn = Conn::new(tls);
    loop {
        let head = match timeout(KEEP_ALIVE, conn.head()).await {
            Ok(Ok(Some(head))) => head,
            Ok(Err(e)) if e.kind() == io::ErrorKind::InvalidData => {
                let _ = conn
                    .send(super::wire::answer(431, &[("Connection", "close".into())], Body::Length(0)).as_bytes())
                    .await;
                return;
            }
            _ => return,
        };
        Counters::add(&s.stats.requests, 1);
        if !answer::handle(&mut conn, &head, &key, &s).await {
            return;
        }
    }
}
