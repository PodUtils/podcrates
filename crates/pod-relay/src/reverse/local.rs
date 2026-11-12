//! A node's local service as podnet names it: `host:port`, `tcp://host:port`
//! or `unix:/path`, and the handler that dials it for each session (podnet's
//! plain mode). A TCP target goes through the caller's `pod_proxy::Dialer`,
//! which resolves a name for each session and keeps a loopback target off
//! any proxy; a Unix socket is on the node's host.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pod_core::BoxedIo;
use pod_proxy::Dialer;

use super::framing::SessionId;
use super::node::{Handler, Opening};

/// The local service of a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A host (a name, an IPv4 address, or an IPv6 address with no
    /// brackets) and a port.
    Tcp { host: String, port: u16 },
    /// A Unix socket on the node's host.
    Unix(PathBuf),
}

impl Target {
    /// `host:port`, `[IPv6]:port`, `tcp://host:port`, or `unix:/path`
    /// (also `unix:///path`).
    pub fn parse(text: &str) -> Result<Target, String> {
        if let Some(path) = text.strip_prefix("unix:") {
            let path = path.trim_start_matches("//");
            if path.is_empty() {
                return Err("the target unix: has no path".into());
            }
            return Ok(Target::Unix(PathBuf::from(path)));
        }
        let bad = || format!("{text:?} is not host:port or unix:/path");
        let rest = text.strip_prefix("tcp://").unwrap_or(text);
        let (host, port) = rest.rsplit_once(':').ok_or_else(bad)?;
        let port = port.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(bad)?;
        let host = match host.strip_prefix('[') {
            Some(v6) => v6.strip_suffix(']').ok_or_else(bad)?,
            // An IPv6 address and a port, with no brackets, cannot be told apart.
            None if host.contains(':') => return Err(format!("write the IPv6 address of {text:?} in brackets")),
            None => host,
        };
        pod_core::names::check_direct_target(host).map_err(|why| format!("{text:?}: {why}"))?;
        Ok(Target::Tcp { host: host.to_string(), port })
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Tcp { host, port } => write!(f, "{}", pod_proxy::host::authority(host, *port)),
            Target::Unix(path) => write!(f, "unix:{}", path.display()),
        }
    }
}

/// Each session is one connection to `target`; a refusal names the target.
#[derive(Clone)]
pub struct LocalHandler {
    pub target: Target,
    /// Bound on each connect, under the node's limit for opening a session.
    pub timeout: Duration,
    /// The dial of a TCP target.
    pub dialer: Arc<Dialer>,
}

impl Handler for LocalHandler {
    type Stream = BoxedIo;

    fn open(&self, _id: SessionId) -> Opening<BoxedIo> {
        let (target, timeout, dialer) = (self.target.clone(), self.timeout, self.dialer.clone());
        Box::pin(async move { connect(&target, &dialer, timeout).await.map_err(|why| format!("{target}: {why}")) })
    }
}

/// One connection to `target`, within `timeout`.
pub async fn connect(target: &Target, dialer: &Dialer, timeout: Duration) -> Result<BoxedIo, String> {
    match target {
        Target::Tcp { host, port } => {
            let stream = dialer.tcp(host, *port, timeout).await.map_err(|e| e.to_string())?;
            Ok(Box::new(stream))
        }
        #[cfg(unix)]
        Target::Unix(path) => {
            let connect = tokio::net::UnixStream::connect(path);
            let stream = tokio::time::timeout(timeout, connect)
                .await
                .map_err(|_| format!("no answer within {} s", timeout.as_secs()))?
                .map_err(|e| e.to_string())?;
            Ok(Box::new(stream))
        }
        #[cfg(not(unix))]
        Target::Unix(_) => Err("this system has no Unix sockets".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// podnet's targets: each form, and each text that is no target.
    #[test]
    fn targets_and_names() {
        let tcp = |host: &str, port| Target::Tcp { host: host.into(), port };
        assert_eq!(Target::parse("127.0.0.1:22"), Ok(tcp("127.0.0.1", 22)));
        assert_eq!(Target::parse("tcp://localhost:5432"), Ok(tcp("localhost", 5432)));
        assert_eq!(Target::parse("[::1]:8022"), Ok(tcp("::1", 8022)));
        assert_eq!(Target::parse("unix:///run/x.sock"), Ok(Target::Unix(PathBuf::from("/run/x.sock"))));
        assert_eq!(Target::parse("unix:/run/x.sock"), Ok(Target::Unix(PathBuf::from("/run/x.sock"))));
        for bad in ["22", "host:", "host:x", "unix:", ":22", "host:0", "::1:22", "[::1:22", "a b:22", "a/b:22"] {
            assert!(Target::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(tcp("::1", 8022).to_string(), "[::1]:8022");
        assert_eq!(Target::Unix(PathBuf::from("/run/x.sock")).to_string(), "unix:/run/x.sock");
    }
}
