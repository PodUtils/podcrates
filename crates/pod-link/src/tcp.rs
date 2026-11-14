//! The tcp carrier (feature `tcp`): TLS 1.3 over TCP, each side pinning
//! the Ed25519 key of the other (`pod_tls::peer`, the key of
//! `pod-identity`). A link that crosses a network proves its keys in this
//! handshake, not in a hello above it.
//!
//! * The dial goes through the way out of the caller (`pod_core::Dial`:
//!   direct, a proxy of `pod-proxy`, any chain), to `KEY@HOST:PORT`. KEY
//!   names the listener's key as `pod_identity::KeyName` reads it: the key
//!   in hex or base32, its fingerprint `SHA256:...`, or its display form.
//! * A listener binds `IP:PORT`, presents the identity of this end, and
//!   takes a client only when its key is in [`Tcp::clients`]; the accept
//!   gives the stream and the key that the client proved. It runs at most
//!   [`Tcp::max_handshakes`] handshakes at once, each for at most
//!   [`Tcp::handshake_timeout`], so a peer that sends nothing holds no other
//!   peer.
//! * TLS 1.3 alone, the ALPN [`Tcp::alpn`] ([`ALPN`] by default; podnet
//!   gives `podnet/1`), no resumption. After its handshake
//!   the listener writes the byte 1: the dial returns only when the
//!   listener took the key of this end, so a refused key is an error of the
//!   dial, not of the first read of the link.
//!
//! A key that is not pinned is a [`CarrierErrorKind::Refused`] error (77)
//! that names the key that the peer proved; [`refusal`] gives it typed. A peer that refuses the key of this
//! end gives [`CarrierErrorKind::KeyRefused`] (77).

mod listen;

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Dial, Failure, Target};
use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::ServerName;
use tokio::io::AsyncReadExt as _;
use tokio::time::{Instant, timeout, timeout_at};

pub use listen::TlsListener;
pub use pod_tls::peer::{Keys, Refusal, refusal};

use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Listener, Pending};
use crate::pin;

/// The name of the carrier.
pub const NAME: &str = "tcp";

/// The ALPN of a link over TLS by default: both ends offer it, and a peer
/// that takes another protocol is refused.
pub const ALPN: &[u8] = b"podlink/1";

/// The byte that a listener writes when it took the key of the dialer.
const ACCEPTED: u8 = 1;

/// The default time limit of a dial through the way out.
pub const CONNECT_TIMEOUT: Duration = pod_core::backoff::CONNECT_TIMEOUT;

/// The default time limit of the TLS handshake on each side, with the byte
/// of acceptance.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The default count of handshakes that a listener runs at once.
pub const MAX_HANDSHAKES: usize = 64;

/// The tcp carrier: the TLS of this end (its provider and its identity),
/// the way out of its dials, and the client keys of its listeners. A clone
/// shares the set of client keys.
#[derive(Clone)]
pub struct Tcp {
    provider: Arc<CryptoProvider>,
    identity: Arc<Identity>,
    way: Arc<dyn Dial>,
    clients: Keys,
    /// The time limit of a dial through the way out.
    pub connect_timeout: Duration,
    /// The time limit of a TLS handshake, on each side.
    pub handshake_timeout: Duration,
    /// The most handshakes that a listener runs at once (at least 1).
    pub max_handshakes: usize,
    /// The ALPN of both ends.
    pub alpn: &'static [u8],
}

impl fmt::Debug for Tcp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tcp")
            .field("identity", &self.identity.public())
            .field("way", &self.way.label())
            .field("clients", &self.clients.list())
            .field("connect_timeout", &self.connect_timeout)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("max_handshakes", &self.max_handshakes)
            .field("alpn", &String::from_utf8_lossy(self.alpn))
            .finish()
    }
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

impl Tcp {
    /// A carrier that presents `identity` on TLS of `provider` and dials
    /// through `way`. Its listeners take no client until
    /// [`Tcp::with_clients`] gives the keys.
    pub fn new(provider: Arc<CryptoProvider>, identity: Arc<Identity>, way: Arc<dyn Dial>) -> Tcp {
        Tcp {
            provider,
            identity,
            way,
            clients: Keys::default(),
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            max_handshakes: MAX_HANDSHAKES,
            alpn: ALPN,
        }
    }

    /// The client keys that a listener takes. The set is shared: a change
    /// of it applies at the next handshake of each listener.
    pub fn with_clients(mut self, clients: Keys) -> Tcp {
        self.clients = clients;
        self
    }

    pub fn clients(&self) -> &Keys {
        &self.clients
    }

    /// The key of the listener and its address in a dial address
    /// `KEY@HOST:PORT`.
    pub fn address(address: &str) -> Result<(KeyName, Target), CarrierError> {
        let (key, target) = pin::key_at(NAME, address, "KEY@HOST:PORT")?;
        let target = target.parse::<Target>().map_err(|e| err(CarrierErrorKind::Usage, e.text().to_string()))?;
        Ok((key, target))
    }

    /// Connect to `target` through the way out and take only the listener
    /// whose key `peer` names. It gives the stream and the key that the
    /// listener proved.
    pub async fn connect(&self, target: &Target, peer: &KeyName) -> Result<(BoxIo, PublicKey), CarrierError> {
        let config = peer::client_config(self.provider.clone(), self.identity.clone(), peer.clone(), &[self.alpn])
            .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
        let name = server_name(target)?;
        let label = self.way.label();
        let deadline = Instant::now() + self.connect_timeout;
        let connected = match timeout_at(deadline, self.way.dial(target, deadline)).await {
            Err(_) => {
                return Err(err(
                    CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                    format!("connect to {target} through {label}: no answer in {:?}", self.connect_timeout),
                ));
            }
            Ok(Err(failure)) => return Err(of_failure(target, &label, failure)),
            Ok(Ok(connected)) => connected,
        };
        let shake = async {
            let mut tls = tokio_rustls::TlsConnector::from(config)
                .connect(name, connected.io)
                .await
                .map_err(|e| tls_error(format!("the TLS handshake with {target}"), e))?;
            let conn = tls.get_ref().1;
            check_alpn(conn.alpn_protocol(), self.alpn, target)?;
            let key = proved(conn.peer_certificates(), target)?;
            let mut word = [0u8];
            match tls.read_exact(&mut word).await {
                Ok(_) if word[0] == ACCEPTED => Ok((Box::new(tls) as BoxIo, key)),
                Ok(_) => Err(err(
                    CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                    format!("the listener at {target} sent {} where its acceptance goes", word[0]),
                )),
                Err(e) => Err(answer_error(target, &self.identity.public(), e)),
            }
        };
        timeout(self.handshake_timeout, shake).await.unwrap_or_else(|_| {
            Err(err(
                CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                format!("the TLS handshake with {target} took more than {:?}", self.handshake_timeout),
            ))
        })
    }

    /// A listener at `addr` (port 0 takes a free port).
    pub async fn bind(&self, addr: SocketAddr) -> Result<TlsListener, CarrierError> {
        listen::bind(self, addr).await
    }
}

impl Carrier for Tcp {
    fn name(&self) -> &'static str {
        NAME
    }

    fn facts(&self) -> Facts {
        Facts::STREAM
    }

    /// The provider makes TLS 1.3 and verifies Ed25519, and a TCP socket
    /// binds on the loopback: the host allows the family.
    fn available(&self) -> Result<(), CarrierError> {
        pin::available(NAME, &self.provider)
    }

    /// Dial `KEY@HOST:PORT` ([`Tcp::address`]).
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (key, target) = Tcp::address(address)?;
            self.connect(&target, &key).await.map(|(io, _)| io)
        })
    }

    /// Listen at `IP:PORT`.
    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            let addr = address.parse::<SocketAddr>().map_err(|_| {
                err(CarrierErrorKind::Usage, format!("{address:?} is no IP:PORT (127.0.0.1:7000, [::]:7000)"))
            })?;
            Ok(Box::new(self.bind(addr).await?) as Box<dyn Listener>)
        })
    }
}

/// The name of the ClientHello ([`pin::server_name`]).
fn server_name(target: &Target) -> Result<ServerName<'static>, CarrierError> {
    pin::server_name(NAME, target)
}

/// A failure of the way out ([`pin::of_failure`]).
fn of_failure(target: &Target, label: &str, failure: Failure) -> CarrierError {
    pin::of_failure(NAME, target, label, failure)
}

/// A failed handshake ([`pin::tls_error`]).
fn tls_error(text: String, e: io::Error) -> CarrierError {
    pin::tls_error(NAME, text, e)
}

/// The error of the wait for the acceptance ([`pin::answer_error`]).
fn answer_error(target: &Target, own: &PublicKey, e: io::Error) -> CarrierError {
    pin::answer_error(NAME, target, own, e)
}

/// The ALPN of a handshake must be `want`.
fn check_alpn(alpn: Option<&[u8]>, want: &[u8], peer: &dyn fmt::Display) -> Result<(), CarrierError> {
    pin::check_alpn(NAME, alpn, want, peer)
}

/// The key that the peer proved ([`pin::proved`]).
fn proved(
    certificates: Option<&[rustls::pki_types::CertificateDer<'_>]>,
    peer: &dyn fmt::Display,
) -> Result<PublicKey, CarrierError> {
    pin::proved(NAME, certificates, peer)
}
