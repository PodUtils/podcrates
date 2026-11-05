//! The TCP connection to a target: directly, with the race of its addresses
//! and Multipath TCP on request, or through an HTTP CONNECT proxy or a
//! SOCKS5 proxy that the settings choose. A [`Dialer`] is made once from
//! its [`Settings`] and the caller's resolver, and implements
//! [`pod_core::Dial`].
//!
//! The proxy path sends the target's name, never an address, and needs no
//! DNS: the proxy resolves the name. The direct path asks the caller's
//! resolver ([`pod_core::Resolve`]) for the addresses.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{BoxFuture, BoxedIo, Connected, Dial, Failure, Resolve, Target};
use tokio::net::TcpStream;
use tokio::time::Instant;

use crate::connect::{Ask, connect_via};
use crate::env::Environment;
use crate::error::DialError;
use crate::host::{authority, check_name};
use crate::mptcp::Multipath;
use crate::proxy::{Proxy, ProxyKind};
use crate::race::{Leg, connect_race, race};
use crate::write_limit::WriteLimit;

/// How long one address may take to connect before the next is tried as
/// well (RFC 8305's attempt delay): an address that never answers, as an
/// IPv6 address with no route can, does not hold the whole dial.
pub const ATTEMPT_DELAY: Duration = Duration::from_millis(250);

/// How to reach a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyChoice {
    /// The proxy that the caller read from its environment
    /// ([`Environment::read`]), for each target that the loopback, the pins
    /// of the resolver and `NO_PROXY` do not exclude.
    FromEnvironment(Environment),
    /// Connect directly.
    Direct,
    /// Use this proxy.
    Via(Proxy),
}

/// The settings of a [`Dialer`], which the consumer fills once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The name of the program in the words of a refusal (`podssh`).
    pub app: String,
    /// The `User-Agent` of a CONNECT request (`podssh/VERSION`); none is
    /// sent with `None`.
    pub user_agent: Option<String>,
    /// How to reach a target.
    pub proxy: ProxyChoice,
    /// Ask for Multipath TCP on a connection with no proxy ([`crate::mptcp`]):
    /// Linux alone makes one, and elsewhere the connection is plain TCP.
    pub multipath: bool,
    /// The bytes that the kernel may hold unsent on each connection
    /// (`TCP_NOTSENT_LOWAT`, [`crate::lowat`]), so that a keystroke does not
    /// wait behind megabytes; `None` keeps the system's buffer.
    pub lowat: Option<u32>,
    /// The limit of a write that makes no progress on a stream of
    /// [`Dial::dial`] ([`WriteLimit`]); `None`: no limit.
    pub write_limit: Option<Duration>,
    /// The attempt delay of the race of addresses.
    pub attempt_delay: Duration,
}

impl Settings {
    /// The settings of `app` with `proxy`: no user agent, no Multipath TCP,
    /// [`crate::lowat::LOWAT`] unsent bytes, no write limit, and
    /// [`ATTEMPT_DELAY`].
    pub fn new(app: impl Into<String>, proxy: ProxyChoice) -> Settings {
        Settings {
            app: app.into(),
            user_agent: None,
            proxy,
            multipath: false,
            lowat: Some(crate::lowat::LOWAT),
            write_limit: None,
            attempt_delay: ATTEMPT_DELAY,
        }
    }
}

/// The dial of a consumer: its [`Settings`] and its resolver.
pub struct Dialer {
    settings: Settings,
    resolve: Arc<dyn Resolve>,
}

impl fmt::Debug for Dialer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dialer").field("settings", &self.settings).field("resolve", &self.resolve.label()).finish()
    }
}

impl Dialer {
    /// A dialer with `settings`, which asks `resolve` for the addresses of
    /// a direct connection and for the pins of a name.
    pub fn new(settings: Settings, resolve: Arc<dyn Resolve>) -> Dialer {
        Dialer { settings, resolve }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn resolver(&self) -> &Arc<dyn Resolve> {
        &self.resolve
    }

    /// The proxy for `host` by the choice of the settings, or none for a
    /// direct connection; why when the proxy of the environment cannot be
    /// used. The environment's proxy is passed over for a name that the
    /// resolver pins to loopback addresses alone ([`Environment::proxy_for`]).
    pub fn proxy_for(&self, host: &str) -> Result<Option<Proxy>, String> {
        match &self.settings.proxy {
            ProxyChoice::Direct => Ok(None),
            ProxyChoice::Via(proxy) => Ok(Some(proxy.clone())),
            ProxyChoice::FromEnvironment(env) => env.proxy_for(host, &self.resolve.pinned(host)),
        }
    }

    /// Open a TCP connection to `host:port`, through the proxy that the
    /// settings select. The whole operation (resolution, connect, and the
    /// exchange with a proxy) is bounded by `timeout`.
    pub async fn tcp(&self, host: &str, port: u16, timeout: Duration) -> Result<TcpStream, DialError> {
        self.tcp_multipath(host, port, timeout).await.map(|(stream, _)| stream)
    }

    /// [`Dialer::tcp`], and how Multipath TCP went: only a connection with
    /// no proxy can carry it, only when the settings ask for it, and only
    /// Linux makes it. Else, and where the kernel refuses it, the stream is
    /// plain TCP; the second value says which, and why.
    pub async fn tcp_multipath(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<(TcpStream, Multipath), DialError> {
        self.open(host, port, Instant::now() + timeout, timeout).await
    }

    fn leg(&self, deadline: Instant, budget: Duration) -> Leg<'_> {
        Leg { resolve: &*self.resolve, attempt_delay: self.settings.attempt_delay, deadline, budget }
    }

    fn ask(&self) -> Option<Result<(), String>> {
        self.settings.multipath.then(crate::mptcp::probe)
    }

    async fn open(
        &self,
        host: &str,
        port: u16,
        deadline: Instant,
        budget: Duration,
    ) -> Result<(TcpStream, Multipath), DialError> {
        check_name(host)?;
        let proxy = self.proxy_for(host).map_err(DialError::BadProxy)?;
        let leg = self.leg(deadline, budget);
        // Through a proxy, MPTCP could reach the proxy only, never the target.
        let proxied =
            || Multipath::Plain("a proxy carries the connection, and Multipath TCP would reach it only".into());
        let ask = Ask { app: &self.settings.app, user_agent: self.settings.user_agent.as_deref() };
        let (stream, how) = match proxy {
            None => connect_race(leg, host, port, self.ask()).await?,
            Some(p) if matches!(p.kind, ProxyKind::Socks5 { .. }) => {
                (crate::socks5::connect(leg, &self.settings.app, &p, host, port).await?, proxied())
            }
            Some(p) => (connect_via(leg, ask, &p, host, port).await?, proxied()),
        };
        Ok((self.tuned(stream), how))
    }

    /// A keystroke and an echo must not wait for Nagle, nor behind the
    /// megabytes that the kernel would hold unsent: a probe, and a system
    /// that refuses the option keeps its buffer.
    fn tuned(&self, stream: TcpStream) -> TcpStream {
        let _ = stream.set_nodelay(true);
        if let Some(bytes) = self.settings.lowat
            && let Err(why) = crate::lowat::set(&stream, bytes)
        {
            tracing::debug!(%why, "TCP_NOTSENT_LOWAT is not set");
        }
        stream
    }

    /// The stream as [`Dial`] gives it: with the write limit of the
    /// settings.
    fn connected(&self, stream: TcpStream) -> Connected {
        let (peer, local) = (stream.peer_addr().ok(), stream.local_addr().ok());
        let io: BoxedIo = match self.settings.write_limit {
            Some(limit) => Box::new(WriteLimit::new(stream, limit)),
            None => Box::new(stream),
        };
        Connected { io, peer, local }
    }
}

impl Dial for Dialer {
    fn label(&self) -> String {
        match &self.settings.proxy {
            ProxyChoice::Direct => "direct".into(),
            ProxyChoice::Via(proxy) => proxy.label(),
            ProxyChoice::FromEnvironment(env) => match &env.proxy {
                Some(given) => match &given.value {
                    Ok(proxy) => format!("{} of {}", proxy.label(), given.origin),
                    Err(_) => format!("the proxy of {}, which cannot be used", given.origin),
                },
                None => "direct".into(),
            },
        }
    }

    /// The error of the dial is the cause of the failure, so that a caller
    /// reads it back ([`crate::Cut::of_failure`]).
    fn dial<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let budget = deadline.saturating_duration_since(Instant::now());
            let (stream, how) = self.open(&target.host(), target.port(), deadline, budget).await?;
            tracing::debug!(%target, %how, "connected");
            Ok(self.connected(stream))
        })
    }

    /// A direct connection races the addresses, as a name's; through a
    /// proxy, each address in turn.
    fn dial_addrs<'a>(
        &'a self,
        host: &'a str,
        addrs: &'a [SocketAddr],
        deadline: Instant,
    ) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            if addrs.is_empty() || !matches!(self.proxy_for(host), Ok(None)) {
                let mut last = None;
                for addr in addrs {
                    match self.dial(&Target::Addr(*addr), deadline).await {
                        Ok(c) => return Ok(c),
                        Err(e) => last = Some(e),
                    }
                }
                return Err(last.unwrap_or_else(|| {
                    Failure::new(
                        pod_core::Stage::Dns,
                        pod_core::Class::Unavailable,
                        format!("{host} has no address to connect to"),
                    )
                }));
            }
            let budget = deadline.saturating_duration_since(Instant::now());
            let target = authority(host, addrs[0].port());
            let (stream, how) =
                race(self.leg(deadline, budget), &target, addrs.to_vec(), deadline, &self.ask()).await?;
            tracing::debug!(%target, %how, "connected");
            Ok(self.connected(self.tuned(stream)))
        })
    }

    fn names_inside(&self) -> bool {
        match &self.settings.proxy {
            ProxyChoice::Direct => false,
            ProxyChoice::Via(_) => true,
            ProxyChoice::FromEnvironment(env) => env.proxy.as_ref().is_some_and(|given| given.value.is_ok()),
        }
    }
}
