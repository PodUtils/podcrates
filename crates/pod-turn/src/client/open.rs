//! The transport to a server: the caller's UDP socket, or the caller's dial
//! and the TLS handshake of the caller's context, with the ALPN `stun.turn`
//! when the settings ask for it; and the one redirect of an Allocate (300
//! Try Alternate, RFC 8489, 10, and RFC 8656, 7.4) to a new transport.

use std::net::SocketAddr;
use std::time::Duration;

use pod_core::{BoxedIo, Target};
use tokio::time::Instant;

use super::wire::Conn;
use super::{Client, Error, Transport};
use crate::stun::ALLOCATE;

/// The ALPN of TURN over TLS (RFC 7443).
pub const ALPN: &[u8] = b"stun.turn";

/// An open transport, and the ALPN protocol that the server chose.
pub struct Link {
    pub conn: Conn,
    pub alpn: Option<Vec<u8>>,
}

/// Open the transport to `server`. Over TLS the certificate is checked
/// against `domain` when a redirect named one, else against the name of the
/// settings.
pub async fn connect(
    server: &Target,
    transport: &Transport,
    domain: Option<&str>,
    deadline: Instant,
    limit: Duration,
) -> Result<Link, Error> {
    match transport {
        Transport::Udp(source) => {
            let (socket, addr) = source.open(server, deadline).await.map_err(Error::Connect)?;
            Ok(Link { conn: Conn::udp(socket, addr), alpn: None })
        }
        Transport::Tcp { dialer } => {
            let connected = dialer.dial(server, deadline).await.map_err(Error::Connect)?;
            Ok(Link { conn: Conn::stream(connected.io), alpn: None })
        }
        Transport::Tls { dialer, context, trust, server_name, alpn } => {
            let connected = dialer.dial(server, deadline).await.map_err(Error::Connect)?;
            let name = domain.unwrap_or(server_name);
            let offered: &[&[u8]] = if *alpn { &[ALPN] } else { &[] };
            let tls =
                context.connect_over_alpn(connected.io, name, trust, offered, limit).await.map_err(Error::Connect)?;
            let chosen = tls.get_ref().1.alpn_protocol().map(<[u8]>::to_vec);
            Ok(Link { conn: Conn::stream(Box::new(tls) as BoxedIo), alpn: chosen })
        }
    }
}

impl Client {
    /// Follow the 300 of an Allocate to `to`: a new transport through the
    /// caller's way out, the same user and password, and a fresh challenge.
    /// The realm of the settings, else the realm that the first server gave,
    /// is the one that the new server must give. A redirect to the same
    /// server is not followed.
    pub(super) async fn redirect(
        &mut self,
        to: SocketAddr,
        domain: Option<String>,
        transport: &Transport,
    ) -> Result<(), Error> {
        let target = Target::Addr(to);
        if self.server == target || self.conn.server() == Some(to) {
            return Err(Error::Alternate { method: ALLOCATE, to, domain });
        }
        if self.realm.is_none() {
            self.realm = self.auth.as_ref().map(|auth| auth.realm.clone());
        }
        let deadline = self.clock.now() + self.limits.connect_timeout;
        let link = connect(&target, transport, domain.as_deref(), deadline, self.limits.connect_timeout)
            .await
            .map_err(|error| Error::Redirected { to, error: Box::new(error) })?;
        tracing::debug!(from = %self.server, %to, "TURN: the Allocate follows a redirect");
        self.conn.shutdown().await;
        self.conn = link.conn;
        self.alpn = link.alpn;
        self.server = target;
        self.tls_name = domain;
        self.auth = None;
        Ok(())
    }
}
