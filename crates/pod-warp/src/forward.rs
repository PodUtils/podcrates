//! Local forwards into WARP (PodUtils/podnet issue 143): podnet's `warp
//! forward -L LISTEN=HOST:PORT`. Each connection to LISTEN goes through the
//! session to HOST:PORT; a name is resolved inside the tunnel
//! ([`crate::proxy::Names`]). The caller binds LISTEN and gives the
//! listener, or each accepted stream.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Event, Failure, Report, Stage, Target};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

use crate::proxy::{CONNECT_TIMEOUT, Names, Tunnel};

fn usage(text: String) -> Failure {
    Failure::new(Stage::Warp, Class::Usage, text)
}

/// Where a forward or a proxy listens: a TCP address, or a Unix socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listen {
    Tcp(SocketAddr),
    Unix(PathBuf),
}

impl Listen {
    /// `unix:PATH` (`unix:///tmp/x` is `/tmp/x`), `tcp://ADDR:PORT`,
    /// `tcp:ADDR:PORT` or `ADDR:PORT`. A bad form is `Usage`.
    pub fn parse(value: &str) -> Result<Listen, Failure> {
        if let Some(path) = value.strip_prefix("unix://").or_else(|| value.strip_prefix("unix:")) {
            if path.is_empty() {
                return Err(usage("unix: listen address has no path".into()));
            }
            let path = match path.starts_with('/') {
                true => format!("/{}", path.trim_start_matches('/')),
                false => path.to_string(),
            };
            return Ok(Listen::Unix(PathBuf::from(path)));
        }
        let rest = value.strip_prefix("tcp://").or_else(|| value.strip_prefix("tcp:")).unwrap_or(value);
        rest.parse()
            .map(Listen::Tcp)
            .map_err(|_| usage(format!("listen address {value:?} is not unix:/path or tcp://host:port")))
    }
}

impl std::fmt::Display for Listen {
    /// `unix:PATH` or `tcp://ADDR:PORT`, the form that [`Listen::parse`]
    /// reads.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Listen::Unix(path) => write!(f, "unix:{}", path.display()),
            Listen::Tcp(addr) => write!(f, "tcp://{addr}"),
        }
    }
}

/// One rule of `-L`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub listen: Listen,
    pub target: Target,
}

impl Rule {
    /// `LISTEN=HOST:PORT`. LISTEN is `PORT` (on 127.0.0.1), `ADDR:PORT`,
    /// `tcp://ADDR:PORT` or `unix:/PATH`. HOST:PORT can start with
    /// `tunnel:` or `tcp://`; an IPv6 address is in brackets. A loopback
    /// target is refused: it is the host itself, which WARP does not reach.
    pub fn parse(spec: &str) -> Result<Rule, Failure> {
        let (listen, target) =
            spec.rsplit_once('=').ok_or_else(|| usage(format!("{spec:?}: give LISTEN=HOST:PORT")))?;
        let listen = match listen.parse::<u16>() {
            Ok(0) => return Err(usage(format!("{spec:?}: the listen port must be 1 to 65535"))),
            Ok(port) => Listen::Tcp(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)),
            Err(_) => Listen::parse(listen).map_err(|e| usage(format!("{spec:?}: {}", e.chain())))?,
        };
        let target = target.strip_prefix("tunnel:").or_else(|| target.strip_prefix("tcp://")).unwrap_or(target);
        let (host, port) = target.rsplit_once(':').ok_or_else(|| usage(format!("{spec:?}: give HOST:PORT after =")))?;
        let port: u16 = port
            .parse()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| usage(format!("{spec:?}: the port must be 1 to 65535")))?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        if host.is_empty() || host.contains(['/', ' ', '[', ']']) {
            return Err(usage(format!("{spec:?}: give HOST:PORT after =")));
        }
        let target = match host.parse::<IpAddr>() {
            Ok(ip) => Target::Addr(SocketAddr::new(ip, port)),
            Err(_) => Target::Name { host: host.to_ascii_lowercase(), port },
        };
        let loopback = match &target {
            Target::Addr(addr) => addr.ip().is_loopback() || addr.ip().is_unspecified(),
            Target::Name { host, .. } => host == "localhost" || host.ends_with(".localhost"),
        };
        if loopback {
            return Err(usage(format!("{spec:?}: the target is a loopback address; WARP does not reach the host")));
        }
        Ok(Rule { listen, target })
    }
}

/// A forward: each connection goes through the session of now to the
/// target of its rule.
pub struct Forward {
    pub rule: Rule,
    pub tunnel: Arc<dyn Tunnel>,
    pub names: Names,
    /// The time limit of each connect ([`CONNECT_TIMEOUT`]).
    pub connect_timeout: Duration,
    pub report: Report,
    limit: Arc<Semaphore>,
}

impl Forward {
    /// A forward of `rule` into `tunnel`, with at most `max` connections at
    /// the same time (0: no limit); a connection over it is closed at once.
    pub fn new(rule: Rule, tunnel: Arc<dyn Tunnel>, names: Names, max: usize) -> Forward {
        let places = if max == 0 { Semaphore::MAX_PERMITS } else { max };
        let limit = Arc::new(Semaphore::new(places));
        Forward { rule, tunnel, names, connect_timeout: CONNECT_TIMEOUT, report: Report::default(), limit }
    }

    /// Serve the connections of `listener`. A listener that other hosts
    /// reach gets the event `warp.forward.open`.
    pub fn run_tcp(self: &Arc<Self>, listener: tokio::net::TcpListener) -> JoinHandle<()> {
        if let Ok(addr) = listener.local_addr()
            && !addr.ip().is_loopback()
        {
            let text = format!("the forward to {} listens on {addr}, which other hosts can reach", self.rule.target);
            self.report.event(Event::warn("warp.forward.open", text));
        }
        let forward = self.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                forward.connection(stream);
            }
        })
    }

    /// Serve the connections of a Unix socket.
    #[cfg(unix)]
    pub fn run_unix(self: &Arc<Self>, listener: tokio::net::UnixListener) -> JoinHandle<()> {
        let forward = self.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                forward.connection(stream);
            }
        })
    }

    /// Serve one connection on its own task. False when it is closed at
    /// once at the limit.
    pub fn connection<S>(self: &Arc<Self>, mut stream: S) -> bool
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let Ok(permit) = self.limit.clone().try_acquire_owned() else {
            tracing::warn!(target = %self.rule.target, "WARP forward: a connection is refused at the limit");
            return false;
        };
        let forward = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let target = &forward.rule.target;
            let net = match forward.tunnel.session().await {
                Ok(net) => net,
                Err(failure) => {
                    tracing::warn!(error = %failure.chain(), "WARP forward: no session");
                    return;
                }
            };
            let mut remote = match forward.names.connect(net.as_ref(), target, forward.connect_timeout).await {
                Ok(connected) => connected.io,
                Err(failure) => {
                    tracing::warn!(%target, error = %failure.chain(), "WARP forward: no connection");
                    return;
                }
            };
            if let Err(e) = tokio::io::copy_bidirectional(&mut stream, &mut remote).await {
                tracing::debug!(%target, error = %e, "WARP forward: the connection ended");
            }
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_of_a_local_forward() {
        let rule = Rule::parse("5432=tunnel:db.internal:5432").unwrap();
        assert_eq!(rule.listen, Listen::Tcp("127.0.0.1:5432".parse().unwrap()));
        assert_eq!(rule.target, Target::Name { host: "db.internal".into(), port: 5432 });
        assert_eq!(rule.target.to_string(), "db.internal:5432");
        let rule = Rule::parse("tcp://0.0.0.0:8080=[2606:4700::1111]:80").unwrap();
        assert_eq!(rule.listen, Listen::Tcp("0.0.0.0:8080".parse().unwrap()));
        assert_eq!(rule.target.to_string(), "[2606:4700::1111]:80");
        let rule = Rule::parse("unix:/run/x.sock=1.1.1.1:443").unwrap();
        assert_eq!(rule.listen, Listen::Unix("/run/x.sock".into()));
        assert_eq!(rule.target, Target::Addr("1.1.1.1:443".parse().unwrap()));
        assert_eq!(Rule::parse("127.0.0.1:9000=Example.com:443").unwrap().target.to_string(), "example.com:443");
        for bad in [
            "5432",
            "0=a:1",
            "5432=db",
            "5432=db:0",
            "5432=:22",
            "5432=127.0.0.1:22",
            "5432=tunnel:localhost:22",
            "5432=[::1]:22",
            "nothing=a:1",
        ] {
            assert_eq!(Rule::parse(bad).unwrap_err().class, Class::Usage, "{bad}");
        }
        assert_eq!(Listen::parse("unix:///tmp/x").unwrap(), Listen::Unix("/tmp/x".into()));
        assert_eq!(Listen::parse("tcp:127.0.0.1:1080").unwrap().to_string(), "tcp://127.0.0.1:1080");
    }
}
