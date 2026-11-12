//! A local SOCKS5 and HTTP proxy whose connections go through WARP
//! (podnet's `warp proxy`): SOCKS5 CONNECT, `UDP ASSOCIATE` and the UDP
//! association in the stream (0x83), HTTP `CONNECT`, and requests for
//! `http://` URLs. Programs that read `ALL_PROXY` or `HTTPS_PROXY` use it.
//!
//! Names are resolved through the tunnel by the resolvers of
//! [`crate::resolver`] (DNS on UDP to 1.1.1.1, or the caller's list on UDP
//! or over TLS), so no question goes to the resolver of the host. The
//! rules of [`crate::rules`] send a destination through the tunnel,
//! directly from the host (the caller's direct dial), or refuse it.
//!
//! The proxy takes the session of the caller as a [`Tunnel`]: one session
//! ([`Fixed`], a [`crate::WarpNet`]), or the session of now, replaced after
//! a reconnect. Each module takes the session as a [`Dial`], so a test can
//! give any dial. The caller binds the listeners and gives them, or each
//! accepted stream ([`Proxy::client`]); the crate prints nothing: a line for
//! the user is an [`Event`] of the caller's [`Report`], and the health line
//! is the caller's ([`Proxy::health`] each [`HEALTH_EVERY`]).

use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_core::{BoxFuture, BoxedIo, Dial, Event, Failure, Report, Target};
use pod_proxy::Credentials;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::task::JoinHandle;

mod client;
mod dns_bound;
pub mod names;
mod route;
mod udp;

pub use client::{Opening, Udp, serve_client};
pub use dns_bound::{DnsGate, MAX_DNS_TASKS};
pub use names::{Names, Pins, order};
pub use udp::relay_bind_ip;

use crate::rules::Rules;

/// The longest time for a TCP connect through the tunnel, or from the host.
pub const CONNECT_TIMEOUT: Duration = crate::resolver::CONNECT_TIMEOUT;
/// The longest time for the request of a client (podnet's
/// `REQUEST_LIMIT`).
pub const REQUEST_LIMIT: Duration = Duration::from_secs(30);
/// The interval of the health line of the caller (PodUtils/podnet issue
/// 116).
pub const HEALTH_EVERY: Duration = Duration::from_secs(600);

/// The session that the clients of a proxy take: the session of the time
/// that each client comes.
pub trait Tunnel: Send + Sync {
    fn session(&self) -> BoxFuture<'_, Result<Arc<dyn Dial>, Failure>>;
}

/// One session for the life of the proxy.
#[derive(Clone)]
pub struct Fixed(pub Arc<dyn Dial>);

impl Tunnel for Fixed {
    fn session(&self) -> BoxFuture<'_, Result<Arc<dyn Dial>, Failure>> {
        Box::pin(async move { Ok(self.0.clone()) })
    }
}

impl Tunnel for crate::net::WarpNet {
    fn session(&self) -> BoxFuture<'_, Result<Arc<dyn Dial>, Failure>> {
        Box::pin(async move { Ok(Arc::new(self.clone()) as Arc<dyn Dial>) })
    }
}

/// The counters of the clients of a proxy (PodUtils/podnet issue 117).
#[derive(Debug, Default)]
pub struct Stats {
    pub active: AtomicU64,
    pub total: AtomicU64,
    pub refused: AtomicU64,
    /// The DNS datagrams that the DNS forwarder dropped at its bound.
    pub dns_dropped: AtomicU64,
    /// When the last refusal at the limit was told, so that one line each
    /// 10 s at most goes to the caller.
    told: Mutex<Option<std::time::Instant>>,
}

/// The numbers of the health line of a proxy: the clients, the decisions of
/// the rules and the drops of the DNS forwarder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Health {
    pub clients: u64,
    pub since_start: u64,
    pub refused: u64,
    pub dns_dropped: u64,
    pub tunnel: u64,
    pub direct: u64,
    pub rejected: u64,
    pub reloads: u64,
}

/// A local proxy into WARP.
pub struct Proxy {
    /// The session of the clients.
    pub tunnel: Arc<dyn Tunnel>,
    /// The way of the rule `direct`: the host's own dial, with no proxy of
    /// the caller (it can be this proxy).
    pub direct: Arc<dyn Dial>,
    /// The rules and their counters, which the caller can replace.
    pub rules: Arc<Rules>,
    /// The names: the resolvers inside the tunnel and the caller's pins.
    pub names: Names,
    /// The credentials that each client must give (PodUtils/podnet issue
    /// 146).
    pub credentials: Option<Credentials>,
    /// The most clients at the same time; 0 is no limit (issue 117).
    pub max_connections: u64,
    pub stats: Arc<Stats>,
    /// The decisions `warp.dispatch` and `warp.rule`, and the events.
    pub report: Report,
    /// The time limit of each connect ([`CONNECT_TIMEOUT`]).
    pub connect_timeout: Duration,
    /// The time limit of the request of a client ([`REQUEST_LIMIT`]).
    pub request_limit: Duration,
}

impl Proxy {
    /// A proxy into `tunnel`, with `direct` for the rule `direct`: the
    /// rules route each destination through the tunnel, the resolver of
    /// the tunnel, no credentials and no limit.
    pub fn new(tunnel: Arc<dyn Tunnel>, direct: Arc<dyn Dial>) -> Proxy {
        Proxy {
            tunnel,
            direct,
            rules: Arc::new(Rules::default()),
            names: Names::default(),
            credentials: None,
            max_connections: 0,
            stats: Arc::new(Stats::default()),
            report: Report::default(),
            connect_timeout: CONNECT_TIMEOUT,
            request_limit: REQUEST_LIMIT,
        }
    }

    /// The numbers of the health line now.
    pub fn health(&self) -> Health {
        let (stats, counters) = (&self.stats, &self.rules.counters);
        use std::sync::atomic::Ordering::Relaxed;
        Health {
            clients: stats.active.load(Relaxed),
            since_start: stats.total.load(Relaxed),
            refused: stats.refused.load(Relaxed),
            dns_dropped: stats.dns_dropped.load(Relaxed),
            tunnel: counters.tunnel.load(Relaxed),
            direct: counters.direct.load(Relaxed),
            rejected: counters.reject.load(Relaxed),
            reloads: counters.reloads.load(Relaxed),
        }
    }

    /// Take a place for a new client: `None` at the limit.
    pub fn admit(&self) -> Option<Admitted> {
        let active = self.stats.active.fetch_add(1, SeqCst) + 1;
        if self.max_connections > 0 && active > self.max_connections {
            self.stats.active.fetch_sub(1, SeqCst);
            self.stats.refused.fetch_add(1, SeqCst);
            return None;
        }
        self.stats.total.fetch_add(1, SeqCst);
        Some(Admitted(self.stats.clone()))
    }

    /// Serve the clients of `listener` until it fails or the task is
    /// aborted. A listener that other hosts reach with no credentials gets
    /// the event `warp.proxy.open` (PodUtils/podnet issue 146).
    pub fn run_tcp(self: &Arc<Self>, listener: tokio::net::TcpListener) -> JoinHandle<()> {
        if let Ok(addr) = listener.local_addr()
            && !addr.ip().is_loopback()
            && self.credentials.is_none()
        {
            let text = format!(
                "the WARP proxy listens on {addr}, which other hosts can reach, and takes each client: give credentials"
            );
            self.report.event(Event::warn("warp.proxy.open", text));
        }
        let proxy = self.clone();
        tokio::spawn(async move {
            while let Ok((stream, peer)) = listener.accept().await {
                let local = stream.local_addr().ok().map(|a| a.ip());
                proxy.client(stream, local.map(|l| (l, peer.ip())));
            }
        })
    }

    /// Serve the clients of a Unix socket until it fails or the task is
    /// aborted.
    #[cfg(unix)]
    pub fn run_unix(self: &Arc<Self>, listener: tokio::net::UnixListener) -> JoinHandle<()> {
        let proxy = self.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                proxy.client(stream, None);
            }
        })
    }

    /// Serve one client on its own task; `control` is the local and the
    /// peer address of a TCP client (PodUtils/podnet#278: the UDP relay
    /// binds the local address and takes the datagrams of the peer
    /// address only). False when the client is closed at once at
    /// [`Proxy::max_connections`] (issue 117).
    pub fn client<S>(self: &Arc<Self>, stream: S, control: Option<(IpAddr, IpAddr)>) -> bool
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let Some(admitted) = self.admit() else {
            self.tell_refusal();
            return false;
        };
        let proxy = self.clone();
        tokio::spawn(async move {
            let _admitted = admitted;
            // The session of now: a new one after a reconnect.
            let net = match proxy.tunnel.session().await {
                Ok(net) => net,
                Err(failure) => {
                    tracing::warn!(error = %failure.chain(), "WARP proxy client refused: no session");
                    return;
                }
            };
            let (routed, by) = (net.clone(), proxy.clone());
            let dial = move |target: Target| async move { by.route(routed, target).await };
            let opening = Opening {
                credentials: proxy.credentials.as_ref(),
                limit: proxy.request_limit,
                report: &proxy.report,
                control,
            };
            let udp = Some(Udp { net, names: &proxy.names });
            if let Err(failure) = serve_client(stream, dial, udp, &opening).await {
                tracing::debug!(error = %failure.chain(), "WARP proxy client failed");
            }
        });
        true
    }

    /// One event `warp.proxy.limit` each 10 s at most.
    fn tell_refusal(&self) {
        let mut told = self.stats.told.lock().unwrap_or_else(|e| e.into_inner());
        if told.is_none_or(|t| t.elapsed() > Duration::from_secs(10)) {
            *told = Some(std::time::Instant::now());
            let text = format!("a client of the WARP proxy is refused: {} clients is the limit", self.max_connections);
            self.report.event(Event::warn("warp.proxy.limit", text));
        }
    }

    /// Connect to `target` by the rules (PodUtils/podnet issues 132 and
    /// 136): through `net`, directly from the host, or not at all (a
    /// [`crate::rules::Refused`] failure, class `Policy`).
    pub async fn route(&self, net: Arc<dyn Dial>, target: Target) -> Result<BoxedIo, Failure> {
        route::route(self, net, target).await
    }
}

/// The place of a client; the drop frees it.
#[derive(Debug)]
pub struct Admitted(Arc<Stats>);

impl Drop for Admitted {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, SeqCst);
    }
}

#[cfg(test)]
mod tests;
