//! The accept loop of a WebSocket server over TLS (feature `serve`): the
//! listeners and the TLS server configuration are the caller's; the loop
//! accepts, runs TLS, reads and checks the upgrade (and its subprotocol,
//! where [`Serve::protocol`] names one), answers 101, waits for the first
//! frame, and gives each session to the caller with its peer, the
//! key that the peer proved and a [`PreAuth`] gate.
//!
//! A connection is before `ready` from its accept until the caller calls
//! [`PreAuth::ready`]. Until then it holds a slot of the caps ([`Caps`]),
//! and each stage has a deadline ([`Deadlines`]): a flood of connections
//! that never authenticate holds a bounded count of slots and bytes, each
//! for a bounded time.
//!
//! * One source holds at most `per_prefix` slots (its /32 for IPv4, its
//!   /64 for IPv6; the loopback too); an accept over it is closed.
//! * When `max_unauth` slots or `preauth_bytes` bytes are full, an accept
//!   evicts the connection of the earliest stage (TLS, head, first frame,
//!   ready), the oldest in it.
//! * A deadline that passes, an eviction, or a [`PreAuth`] dropped with no
//!   `ready` cuts the connection.
//! * Each accepted socket gets `TCP_NODELAY`, and `TCP_NOTSENT_LOWAT` where
//!   the system has it (Linux, Android).
//!
//! The loop runs until its future is dropped; the drop ends each
//! connection that did not reach the caller, and the deadline of each one
//! that did.

mod conn;
mod gate;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_identity::PublicKey;
use pod_proxy::http::request::Request;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

pub use gate::PreAuth;

use crate::frame;
use crate::limits::Limits;
use crate::session::RelaySession;

/// The TLS stream of a session that the loop accepted.
pub type ServedStream = tokio_rustls::server::TlsStream<TcpStream>;

/// The deadline of each stage before `ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadlines {
    /// The TLS handshake, from the accept.
    pub tls: Duration,
    /// The head of the upgrade, from the end of the TLS handshake.
    pub head: Duration,
    /// The first frame, from the upgrade (the 101).
    pub first: Duration,
    /// [`PreAuth::ready`], from the accept: the bound of each stage.
    pub ready: Duration,
}

impl Default for Deadlines {
    /// 5 s for TLS, 3 s for the head, 2 s for the first frame, 10 s to
    /// `ready`.
    fn default() -> Deadlines {
        Deadlines {
            tls: Duration::from_secs(5),
            head: Duration::from_secs(3),
            first: Duration::from_secs(2),
            ready: Duration::from_secs(10),
        }
    }
}

/// The caps of the connections before `ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// The connections before `ready`, of all sources.
    pub max_unauth: usize,
    /// The connections before `ready` of one source: its /32 for IPv4,
    /// its /64 for IPv6.
    pub per_prefix: usize,
    /// The bytes of the connections before `ready`: each holds
    /// [`Serve::reserve`].
    pub preauth_bytes: usize,
    /// The bytes of the head of the upgrade.
    pub head_bytes: usize,
    /// The lines of the head of the upgrade, with its request line.
    pub head_lines: usize,
}

impl Default for Caps {
    /// 1,024 connections, 64 of one source, 64 MiB; a head of 4 KiB and
    /// 32 lines.
    fn default() -> Caps {
        Caps { max_unauth: 1024, per_prefix: 64, preauth_bytes: 64 << 20, head_bytes: 4096, head_lines: 32 }
    }
}

/// The settings of the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Serve {
    pub deadlines: Deadlines,
    pub caps: Caps,
    /// `TCP_NOTSENT_LOWAT` of each accepted socket, where the system has
    /// it.
    pub lowat: Option<u32>,
    /// The frame and the message that each session takes in.
    pub limits: Limits,
    /// The bound of each read of a session after the first frame; none
    /// waits for ever.
    pub idle: Option<Duration>,
    /// The one subprotocol of the server, a constant of the caller: a
    /// request that does not offer it gets 426, and the 101 names it. None
    /// chooses no subprotocol and takes each request.
    pub protocol: Option<&'static str>,
}

impl Default for Serve {
    /// The default deadlines and caps, a `lowat` of 16 KiB, and limits of
    /// a frame of 16 KiB and a message of 64 KiB, so that 1,024 sessions
    /// before `ready` fill the 64 MiB.
    fn default() -> Serve {
        Serve {
            deadlines: Deadlines::default(),
            caps: Caps::default(),
            lowat: Some(16 << 10),
            limits: Limits { message: 64 << 10, frame: 16 << 10 },
            idle: None,
            protocol: None,
        }
    }
}

impl Serve {
    /// The bytes that one connection holds against `preauth_bytes` until
    /// `ready`: the largest buffer of the loop (the head, the first frame
    /// with its header) or of the session (a message), whichever is larger.
    pub fn reserve(&self) -> usize {
        let message = self.limits.message.max(frame::MAX_CONTROL_PAYLOAD);
        let frame = self.receive_cap().saturating_add(FRAME_HEADER);
        self.caps.head_bytes.max(frame).max(message)
    }

    /// The largest frame that a session takes in, as
    /// [`RelaySession::with_limits`] clamps it.
    fn receive_cap(&self) -> usize {
        let message = self.limits.message.max(frame::MAX_CONTROL_PAYLOAD);
        self.limits.frame.clamp(frame::MAX_CONTROL_PAYLOAD, message)
    }

    /// Each deadline more than zero, each cap at least one, room for one
    /// connection in `preauth_bytes`, and a subprotocol name that
    /// [`crate::server::is_protocol_name`] takes.
    pub fn check(&self) -> Result<(), String> {
        if let Some(protocol) = self.protocol
            && !crate::server::is_protocol_name(protocol)
        {
            return Err(format!("the subprotocol {protocol:?} is not visible ASCII with no comma"));
        }
        let d = &self.deadlines;
        if [d.tls, d.head, d.first, d.ready].contains(&Duration::ZERO) {
            return Err("each deadline of the loop is more than zero".into());
        }
        let c = &self.caps;
        if c.max_unauth == 0 || c.per_prefix == 0 || c.head_lines == 0 {
            return Err("max_unauth, per_prefix and head_lines are 1 or more".into());
        }
        if c.head_bytes < MIN_HEAD {
            return Err(format!("head_bytes is {MIN_HEAD} or more"));
        }
        if c.preauth_bytes < self.reserve() {
            return Err(format!("preauth_bytes holds no connection of {} bytes", self.reserve()));
        }
        Ok(())
    }
}

/// The smallest cap of a head: a request line and a Host.
const MIN_HEAD: usize = 64;

/// The longest header of a frame of a client: 2 bytes, 8 of length, 4 of
/// mask (RFC 6455 section 5.2).
const FRAME_HEADER: usize = 14;

/// The options that `getsockopt` read on an accepted socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub nodelay: bool,
    /// None where the system has no `TCP_NOTSENT_LOWAT`, or the read failed.
    pub notsent_lowat: Option<u32>,
}

/// A session that the loop accepted: its first frame came.
pub struct Accepted {
    /// The session, as a server's: its first frame waits in it.
    pub session: RelaySession<ServedStream>,
    /// The address of the peer.
    pub peer: SocketAddr,
    /// The Ed25519 key that the peer proved in TLS, when its certificate
    /// holds one (`pod_tls::peer::peer_key`).
    pub key: Option<PublicKey>,
    /// The head of the upgrade: its target and its headers.
    pub request: Request,
    /// The options of the socket.
    pub options: Options,
    /// The slot of the session until the caller admits its peer.
    pub gate: PreAuth,
}

impl std::fmt::Debug for Accepted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accepted")
            .field("peer", &self.peer)
            .field("key", &self.key)
            .field("target", &self.request.target)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// The accepts that wait for a slot.
const INCOMING: usize = 64;

/// The pause after a failed accept (no file descriptor left), so that the
/// task does not spin.
const ACCEPT_PAUSE: Duration = Duration::from_millis(50);

/// Accept on each of `listeners` with the TLS of `tls`, and give each
/// session to `on` after its first frame. The error is a usage error of
/// `cfg` or of no listener; else the loop runs until its future is dropped.
pub async fn serve<F>(
    listeners: Vec<TcpListener>,
    tls: Arc<rustls::ServerConfig>,
    cfg: Serve,
    on: F,
) -> Result<(), Failure>
where
    F: FnMut(Accepted) + Send,
{
    serve_reporting(listeners, tls, cfg, on, |_, _| {}).await
}

/// [`serve`], and give each TLS handshake that failed to `failed`: the
/// address of the peer and the error of TLS. The error holds the
/// `rustls::Error` of the handshake, so the caller can tell an alert of the
/// peer (a client that refused the certificate of this server) from noise.
/// The loop closes the connection as [`serve`] does.
pub async fn serve_reporting<F, G>(
    listeners: Vec<TcpListener>,
    tls: Arc<rustls::ServerConfig>,
    cfg: Serve,
    mut on: F,
    mut failed: G,
) -> Result<(), Failure>
where
    F: FnMut(Accepted) + Send,
    G: FnMut(SocketAddr, std::io::Error) + Send,
{
    cfg.check().map_err(|why| Failure::new(Stage::WebSocket, Class::Usage, why))?;
    if listeners.is_empty() {
        return Err(Failure::new(Stage::WebSocket, Class::Usage, "the loop has no listener"));
    }
    let cfg = Arc::new(cfg);
    let gate = Arc::new(gate::Gate::new(cfg.caps, cfg.reserve()));
    let acceptor = TlsAcceptor::from(tls);
    let (incoming_tx, mut incoming) = mpsc::channel(INCOMING);
    let (out, mut sessions) = mpsc::unbounded_channel();
    let (tls_failed, mut tls_failures) = mpsc::unbounded_channel();
    let mut tasks = JoinSet::new();
    for listener in listeners {
        tasks.spawn(accept_each(listener, incoming_tx.clone()));
    }
    drop(incoming_tx);
    loop {
        tokio::select! {
            Some(accepted) = sessions.recv() => on(accepted),
            Some((from, e)) = tls_failures.recv() => failed(from, e),
            Some((tcp, from)) = incoming.recv() => {
                while tasks.try_join_next().is_some() {}
                let start = tokio::time::Instant::now();
                match gate.admit(gate::Prefix::of(&from)) {
                    Ok(slot) => {
                        let conn = conn::Conn {
                            gate: gate.clone(),
                            slot,
                            tcp,
                            from,
                            acceptor: acceptor.clone(),
                            cfg: cfg.clone(),
                            out: out.clone(),
                            tls_failed: tls_failed.clone(),
                            start,
                        };
                        tasks.spawn(conn::run(conn));
                    }
                    Err(why) => tracing::debug!(%from, ?why, "an accept over the caps is closed"),
                }
            }
            else => return Ok(()),
        }
    }
}

/// Accept on `listener` and give each connection to the loop.
async fn accept_each(listener: TcpListener, to: mpsc::Sender<(TcpStream, SocketAddr)>) {
    loop {
        match listener.accept().await {
            Ok(accepted) => {
                if to.send(accepted).await.is_err() {
                    return;
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "an accept failed");
                tokio::time::sleep(ACCEPT_PAUSE).await;
            }
        }
    }
}
