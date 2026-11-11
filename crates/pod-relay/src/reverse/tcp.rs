//! A handler that offers a TCP service: each session of the relay is one
//! connection to TARGET.

use std::sync::Arc;
use std::time::Duration;

use pod_proxy::Dialer;
use tokio::net::TcpStream;

use super::framing::SessionId;
use super::node::{Handler, Opening};
use super::sshd;

/// Each session dials `host:port`, with the caller's `pod_proxy::Dialer`,
/// which keeps a loopback target off any proxy.
#[derive(Clone)]
pub struct TcpHandler {
    pub host: String,
    pub port: u16,
    /// Bound on each dial, under the node's limit for opening a session.
    pub timeout: Duration,
    /// A roost (T-166): TARGET is a standard sshd, whose version line comes
    /// first on each connection, or the session is refused.
    pub sshd: bool,
    /// The dial of each connection.
    pub dialer: Arc<Dialer>,
}

impl Handler for TcpHandler {
    type Stream = TcpStream;

    fn open(&self, _id: SessionId) -> Opening<TcpStream> {
        let (host, port, timeout, sshd) = (self.host.clone(), self.port, self.timeout, self.sshd);
        let dialer = self.dialer.clone();
        Box::pin(async move { dial_target(&dialer, &host, port, timeout, sshd).await })
    }
}

/// One connection to `host:port` through `dialer`; with `sshd`, a roost's,
/// which must then send an SSH version line first (podssh T-166). The line
/// stays in the stream for the session's SSH client.
pub async fn dial_target(
    dialer: &Dialer,
    host: &str,
    port: u16,
    timeout: Duration,
    sshd: bool,
) -> Result<TcpStream, String> {
    let stream = dialer.tcp(host, port, timeout).await.map_err(|e| e.to_string())?;
    if sshd {
        let at = pod_proxy::host::authority(host, port);
        sshd::check(&stream, sshd::BANNER_LIMIT).await.map_err(|why| format!("{at} is not an SSH server: it {why}"))?;
    }
    Ok(stream)
}
