//! The way out: a target, a dialer that connects to it, and a resolver that
//! gives the addresses of a name. A crate that opens a connection takes an
//! `Arc<dyn Dial>` from its caller; the binary makes the chain (direct, a
//! proxy, WARP, the relay, a road of `pod-egress`).

use std::fmt;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::Instant;

use crate::datagrams::Datagrams;
use crate::failure::{Class, Failure, Stage};

/// A boxed future, so that the traits can be used as `dyn`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A stream that a dial gives.
pub trait Io: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin + ?Sized> Io for T {}

/// A boxed stream.
pub type BoxedIo = Box<dyn Io>;

/// The far end of a flow: a name that a resolver (or the far end) resolves,
/// or an address.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Name { host: String, port: u16 },
    Addr(SocketAddr),
}

impl Target {
    /// A name and a port. An IP literal becomes an address.
    pub fn new(host: &str, port: u16) -> Target {
        let bare = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
        match bare.parse::<IpAddr>() {
            Ok(ip) => Target::Addr(SocketAddr::new(ip, port)),
            Err(_) => Target::Name { host: host.to_string(), port },
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            Target::Name { port, .. } => *port,
            Target::Addr(a) => a.port(),
        }
    }

    /// The host as a proxy or a TLS name takes it: the name, or the address
    /// with no brackets.
    pub fn host(&self) -> String {
        match self {
            Target::Name { host, .. } => host.clone(),
            Target::Addr(a) => a.ip().to_string(),
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Name { host, port } => write!(f, "{host}:{port}"),
            Target::Addr(a) => write!(f, "{a}"),
        }
    }
}

impl FromStr for Target {
    type Err = Failure;

    /// `HOST:PORT`, or `[V6]:PORT`.
    fn from_str(s: &str) -> Result<Target, Failure> {
        let bad = || Failure::new(Stage::Connect, Class::Usage, format!("{s:?} is not HOST:PORT"));
        let (host, port) = s.rsplit_once(':').ok_or_else(bad)?;
        let port: u16 = port.parse().map_err(|_| bad())?;
        if host.is_empty() || (host.contains(':') && !host.starts_with('[')) {
            return Err(bad());
        }
        Ok(Target::new(host, port))
    }
}

/// A connected stream, with its addresses when the dialer knows them.
pub struct Connected {
    pub io: BoxedIo,
    /// The address of the far end of the TCP connection (a proxy's, for a
    /// dial through a proxy).
    pub peer: Option<SocketAddr>,
    pub local: Option<SocketAddr>,
}

impl Connected {
    pub fn new(io: BoxedIo) -> Connected {
        Connected { io, peer: None, local: None }
    }
}

impl fmt::Debug for Connected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connected").field("peer", &self.peer).field("local", &self.local).finish_non_exhaustive()
    }
}

/// A way out for TCP, and for UDP when the way carries it.
pub trait Dial: Send + Sync {
    /// The name of the way for a log line (`direct`, `socks5h://proxy:1080`).
    fn label(&self) -> String;

    /// The proxy that a dial of `target` goes through, as a line names it.
    /// None: no proxy, or a way that does not say. A line that blames a
    /// proxy names one only where this gives one.
    fn proxy_label(&self, target: &Target) -> Option<String> {
        let _ = target;
        None
    }

    /// Connect to `target` before `deadline`.
    fn dial<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>>;

    /// Connect to `target` for a request of plain `http://`: a way that
    /// chooses its proxy by the scheme (`http_proxy`) overrides it. The
    /// default is [`Dial::dial`].
    fn dial_http<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        self.dial(target, deadline)
    }

    /// Connect to the first of `addrs` that answers, for a caller that
    /// resolved `host` itself. The default tries them in their order.
    fn dial_addrs<'a>(
        &'a self,
        host: &'a str,
        addrs: &'a [SocketAddr],
        deadline: Instant,
    ) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let mut last = None;
            for addr in addrs {
                match self.dial(&Target::Addr(*addr), deadline).await {
                    Ok(c) => return Ok(c),
                    Err(e) => last = Some(e),
                }
            }
            Err(last.unwrap_or_else(|| {
                Failure::new(Stage::Dns, Class::Unavailable, format!("{host} has no address to connect to"))
            }))
        })
    }

    /// True when the far end of the way resolves a name (socks5h, a relay):
    /// the caller then sends the name, not an address.
    fn names_inside(&self) -> bool {
        false
    }

    /// Datagrams through this way. The default: the way carries no UDP.
    fn datagrams<'a>(&'a self) -> BoxFuture<'a, Result<Arc<dyn Datagrams>, Failure>> {
        let label = self.label();
        Box::pin(
            async move { Err(Failure::new(Stage::Connect, Class::Unavailable, format!("{label} carries no UDP"))) },
        )
    }
}

/// One source of a resolver (the system, a DoH server, a pin), as a rank in
/// its order and a name for a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Source {
    pub rank: u8,
    pub name: &'static str,
}

/// The addresses of a name, the source that gave them, and whether a source
/// after it may give more when none of them connects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub addrs: Vec<SocketAddr>,
    pub source: Source,
    /// True when the resolver asks a source after this one when none of
    /// these addresses connects (a cache that can be stale, DNS over HTTPS
    /// before the old addresses). The race of addresses then gives them
    /// [`SOURCE_WAIT`] to connect, and asks again with `after: Some(source)`.
    pub more: bool,
}

/// How long the addresses of a source with another after it have to
/// connect before the race of addresses asks the next source: a stale
/// address does not hold the dial. A resolver that waits for a slow source
/// before an old one uses the same time.
pub const SOURCE_WAIT: Duration = Duration::from_secs(4);

/// The addresses of a name. `pod-dns` implements it; `pod-proxy` dials
/// through it, so neither crate depends on the other.
pub trait Resolve: Send + Sync {
    fn label(&self) -> String;

    /// The addresses of `host`, from the first source after `after` (from
    /// the first source with `None`) that has some, before `deadline`. A
    /// race of addresses asks again with the source of the last answer when
    /// none of its addresses connected and the answer said [`Found::more`].
    fn resolve_from<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        deadline: Instant,
        after: Option<Source>,
    ) -> BoxFuture<'a, Result<Found, Failure>>;

    /// The addresses of `host` from the first source that has some.
    fn resolve<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        deadline: Instant,
    ) -> BoxFuture<'a, Result<Vec<SocketAddr>, Failure>> {
        Box::pin(async move { self.resolve_from(host, port, deadline, None).await.map(|f| f.addrs) })
    }

    /// The addresses that the caller pinned for `host`, with no lookup.
    fn pinned(&self, _host: &str) -> Vec<IpAddr> {
        Vec::new()
    }

    /// A connection to `peer` worked: a resolver can keep the source first.
    fn note_dialed(&self, _host: &str, _peer: SocketAddr, _source: Source) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_parses_a_name_an_address_and_a_v6_address() {
        let name: Target = "relay.example:443".parse().unwrap();
        assert_eq!(name, Target::Name { host: "relay.example".into(), port: 443 });
        assert_eq!(name.to_string(), "relay.example:443");
        let v4: Target = "10.0.0.1:80".parse().unwrap();
        assert_eq!(v4, Target::Addr("10.0.0.1:80".parse().unwrap()));
        let v6: Target = "[2001:db8::1]:8443".parse().unwrap();
        assert_eq!(v6.host(), "2001:db8::1");
        assert_eq!(v6.to_string(), "[2001:db8::1]:8443");
        assert_eq!(v6.port(), 8443);
    }

    #[test]
    fn a_target_refuses_what_is_not_host_and_port() {
        for bad in ["relay.example", ":443", "relay.example:http", "2001:db8::1:443", "host:70000"] {
            let e = bad.parse::<Target>().expect_err(bad);
            assert_eq!((e.stage, e.class), (Stage::Connect, Class::Usage), "{bad}");
        }
    }
}
