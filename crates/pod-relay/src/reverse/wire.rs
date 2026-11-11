//! How a runner reaches the relay: TLS for each real use, or plain `ws://` to
//! a stand-in relay on the loopback, for the tests of an embedder (feature
//! `plain-ws`, podssh T-068). A binary does not enable the feature. An
//! operator's leg of a carrier (podssh T-595) is a pipe to the relay role's
//! side in its own process, through the role's [`Door`].

use std::time::Duration;

use pod_core::BoxFuture;
use pod_ws::client::{ConnectError, WsClientConfig, connect};
use pod_ws::session::RelaySession;
use tokio::io::DuplexStream;

/// The relay role's side of a carrier's node in this process: the role of
/// `pod-relay-serve` gives one. It opens a socket at `path` as the role's
/// own client, within `limit`, with no connection and no token: the stream
/// is the door.
pub trait Door: Send + Sync + 'static {
    /// A socket of the role at `path`.
    fn open<'a>(
        &'a self,
        path: &'a str,
        limit: Duration,
    ) -> BoxFuture<'a, Result<RelaySession<DuplexStream>, ConnectError>>;
    /// What the door is, for a debug line.
    fn label(&self) -> String;
}

/// A carrier's [`Door`]: `Copy`, as each wire is, so the role lives as long
/// as the process, which runs one carrier.
#[derive(Clone, Copy)]
pub struct Carrier(&'static dyn Door);

impl Carrier {
    /// The carrier of `door`, which lives as long as the process.
    pub fn of(door: impl Door) -> Carrier {
        Carrier(Box::leak(Box::new(door)))
    }

    /// The door.
    pub fn door(self) -> &'static dyn Door {
        self.0
    }
}

impl PartialEq for Carrier {
    fn eq(&self, other: &Carrier) -> bool {
        std::ptr::addr_eq(self.0, other.0)
    }
}

impl Eq for Carrier {}

impl std::fmt::Debug for Carrier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Carrier({})", self.0.label())
    }
}

/// The wire of a socket to the relay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Wire {
    /// TLS to the relay, through the proxy when one is set.
    #[default]
    Tls,
    /// Plain `ws://` to a stand-in relay on the loopback, for tests. A host
    /// that is not the loopback is refused before any connection, so a token
    /// never leaves the host in clear (`pod_ws::plain`).
    #[cfg(feature = "plain-ws")]
    PlainLoopback,
    /// The relay role's side of a carrier's node, in this process: no
    /// connection, no token.
    Carrier(Carrier),
}

/// A socket to the relay, on either wire.
pub(crate) enum Socket {
    Tls(RelaySession),
    #[cfg(feature = "plain-ws")]
    Plain(RelaySession<tokio::net::TcpStream>),
    Carrier(RelaySession<DuplexStream>),
}

/// Connect on `wire` with `token`, as `config` says.
pub(crate) async fn open(wire: Wire, config: &WsClientConfig, token: &str) -> Result<Socket, ConnectError> {
    match wire {
        Wire::Tls => connect(config, token).await.map(Socket::Tls),
        #[cfg(feature = "plain-ws")]
        Wire::PlainLoopback => {
            let at = &config.endpoint;
            pod_ws::plain::connect_loopback(&at.host, at.port, &at.path, token, config.timeout, config.idle_timeout)
                .await
                .map(Socket::Plain)
        }
        // The stream is the door: the role takes no token.
        Wire::Carrier(carrier) => carrier.door().open(&config.endpoint.path, config.timeout).await.map(Socket::Carrier),
    }
}

/// What the facade's forward stream uses of a socket.
#[cfg(feature = "blocking")]
impl Socket {
    pub(crate) async fn read_frame(&self) -> Result<pod_ws::Frame, pod_ws::SessionError> {
        match self {
            Socket::Tls(s) => s.read_frame().await,
            #[cfg(feature = "plain-ws")]
            Socket::Plain(s) => s.read_frame().await,
            Socket::Carrier(s) => s.read_frame().await,
        }
    }

    pub(crate) async fn send_binary(&self, payload: &[u8]) -> Result<(), pod_ws::SessionError> {
        match self {
            Socket::Tls(s) => s.send_binary(payload).await,
            #[cfg(feature = "plain-ws")]
            Socket::Plain(s) => s.send_binary(payload).await,
            Socket::Carrier(s) => s.send_binary(payload).await,
        }
    }

    pub(crate) async fn send_close(&self, code: u16, reason: &str) -> Result<(), pod_ws::SessionError> {
        match self {
            Socket::Tls(s) => s.send_close(code, reason).await,
            #[cfg(feature = "plain-ws")]
            Socket::Plain(s) => s.send_close(code, reason).await,
            Socket::Carrier(s) => s.send_close(code, reason).await,
        }
    }

    pub(crate) fn close_sent(&self) -> bool {
        match self {
            Socket::Tls(s) => s.close_sent(),
            #[cfg(feature = "plain-ws")]
            Socket::Plain(s) => s.close_sent(),
            Socket::Carrier(s) => s.close_sent(),
        }
    }
}
