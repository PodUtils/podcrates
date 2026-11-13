//! The quic carrier (feature `quic`): one bidirectional stream of a QUIC
//! connection of `pod-quic`, with the TLS 1.3 of the tcp carrier: each side
//! pins the Ed25519 key of the other (`pod_tls::peer`).
//!
//! * The dial goes to `KEY@HOST:PORT`, from a UDP socket of the system or
//!   over the datagrams of the caller's way out ([`Quic::with_way`],
//!   `pod_core::Dial::datagrams`). KEY names the listener's key as
//!   `pod_identity::KeyName` reads it. A name resolves through the caller's
//!   resolver ([`Quic::with_resolver`]), else through the system.
//! * A listener binds `IP:PORT` (or runs over the caller's datagrams,
//!   [`Quic::bind_over`]) and takes a client only when its key is in
//!   [`Quic::clients`]. It runs each handshake in a task of its own, at
//!   most [`Quic::max_handshakes`] at once, each within
//!   [`Quic::handshake_timeout`].
//! * The ALPN is [`Quic::alpn`] ([`ALPN`] by default; podnet gives
//!   `podnet/1`). No resumption, no 0-RTT, no datagram of QUIC, no stream
//!   of the client: after its handshake the listener opens the one stream
//!   and writes the byte 1 on it, so a refused key is an error of the dial,
//!   as on the tcp carrier.
//! * A shutdown ends the stream (FIN). A drop ends the stream, waits until
//!   the peer acknowledged each byte (at most [`Quic::linger`]), then
//!   closes the connection.
//!
//! A key that is not pinned is a [`CarrierErrorKind::Refused`] error (77)
//! that names the key that the peer proved; `tcp::refusal` gives it typed.

mod io;
mod listen;

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Datagrams, Dial, Resolve, Target};
use pod_identity::{Identity, KeyName, PublicKey};
use pod_quic::noq::crypto::rustls::QuicClientConfig;
use pod_quic::noq::{self, VarInt};
use pod_tls::peer::{self, Keys};
use rustls::crypto::CryptoProvider;
use tokio::time::timeout;

pub use listen::QuicListener;

use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Listener, Pending};
use crate::pin;

/// The name of the carrier.
pub const NAME: &str = "quic";

/// The ALPN by default: the one of the tcp carrier.
pub const ALPN: &[u8] = crate::tcp::ALPN;

/// The byte that a listener writes when it took the key of the dialer.
const ACCEPTED: u8 = 1;

/// The default time limit of the resolution of a name and of the datagrams
/// of the way out.
pub const CONNECT_TIMEOUT: Duration = pod_core::backoff::CONNECT_TIMEOUT;

/// The default time limit of a handshake on each side, with the byte of
/// acceptance.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The default count of handshakes that a listener runs at once.
pub const MAX_HANDSHAKES: usize = 64;

/// The default silence that ends a connection: the `dead_after` of a link.
pub const SILENCE: Duration = Duration::from_secs(30);

/// The default interval of the keepalive of QUIC: the `keepalive` of a link.
pub const KEEPALIVE: Duration = Duration::from_secs(10);

/// The default wait after a drop for the acknowledgement of each byte.
pub const LINGER: Duration = Duration::from_secs(10);

/// The transport of the carrier by default: `pod-quic`'s, with the silence,
/// the keepalive and the handshake's wait of a link, and one stream.
pub fn transport() -> pod_quic::Settings {
    pod_quic::Settings {
        handshake_wait: HANDSHAKE_TIMEOUT,
        silence: SILENCE,
        keepalive: Some(KEEPALIVE),
        max_streams: 1,
        ..pod_quic::Settings::default()
    }
}

/// The quic carrier: the TLS of this end (its provider and its identity),
/// the way of its datagrams, and the client keys of its listeners. A clone
/// shares the set of client keys.
#[derive(Clone)]
pub struct Quic {
    provider: Arc<CryptoProvider>,
    identity: Arc<Identity>,
    way: Option<Arc<dyn Dial>>,
    resolver: Option<Arc<dyn Resolve>>,
    clients: Keys,
    /// The ALPN of both ends.
    pub alpn: &'static [u8],
    /// The transport of QUIC: the silence, the keepalive, the first packets
    /// and the ceiling of the MTU. The carrier sets the streams.
    pub transport: pod_quic::Settings,
    /// The time limit of the resolution of a name and of the datagrams of
    /// the way out.
    pub connect_timeout: Duration,
    /// The time limit of a handshake, on each side.
    pub handshake_timeout: Duration,
    /// The most handshakes that a listener runs at once (at least 1).
    pub max_handshakes: usize,
    /// The most time that a dropped stream waits for the acknowledgement
    /// of its bytes before the close of its connection.
    pub linger: Duration,
}

impl fmt::Debug for Quic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Quic")
            .field("identity", &self.identity.public())
            .field("way", &self.way.as_ref().map_or_else(|| "the system".into(), |w| w.label()))
            .field("resolver", &self.resolver.as_ref().map_or_else(|| "the system".into(), |r| r.label()))
            .field("clients", &self.clients.list())
            .field("alpn", &String::from_utf8_lossy(self.alpn))
            .field("transport", &self.transport)
            .field("connect_timeout", &self.connect_timeout)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("max_handshakes", &self.max_handshakes)
            .field("linger", &self.linger)
            .finish()
    }
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

impl Quic {
    /// A carrier that presents `identity` on TLS of `provider` and sends
    /// from a UDP socket of the system. Its listeners take no client until
    /// [`Quic::with_clients`] gives the keys.
    pub fn new(provider: Arc<CryptoProvider>, identity: Arc<Identity>) -> Quic {
        Quic {
            provider,
            identity,
            way: None,
            resolver: None,
            clients: Keys::default(),
            alpn: ALPN,
            transport: transport(),
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            max_handshakes: MAX_HANDSHAKES,
            linger: LINGER,
        }
    }

    /// The client keys that a listener takes. The set is shared: a change
    /// of it applies at the next handshake of each listener.
    pub fn with_clients(mut self, clients: Keys) -> Quic {
        self.clients = clients;
        self
    }

    /// Each dial sends over the datagrams of `way` (`Dial::datagrams`), not
    /// from a socket of the system.
    pub fn with_way(mut self, way: Arc<dyn Dial>) -> Quic {
        self.way = Some(way);
        self
    }

    /// A name of a dial address resolves through `resolver`.
    pub fn with_resolver(mut self, resolver: Arc<dyn Resolve>) -> Quic {
        self.resolver = Some(resolver);
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

    /// The transport toward an address of the family `v6`, with `streams`
    /// bidirectional streams that the peer may open.
    fn transport_config(&self, v6: bool, streams: u32) -> noq::TransportConfig {
        let mut transport = self.transport.transport(v6);
        transport
            .max_concurrent_bidi_streams(VarInt::from_u32(streams))
            .max_concurrent_uni_streams(VarInt::from_u32(0))
            .datagram_receive_buffer_size(None);
        transport
    }

    /// The endpoint of one dial toward `addr`.
    async fn endpoint(&self, target: &Target, addr: SocketAddr) -> Result<noq::Endpoint, CarrierError> {
        let failed = |label: &str, f| pin::of_failure(NAME, target, label, f);
        match &self.way {
            None => pod_quic::endpoint::system(addr.is_ipv6()).map_err(|f| failed("the system", f)),
            Some(way) => {
                let label = way.label();
                let datagrams = timeout(self.connect_timeout, way.datagrams())
                    .await
                    .map_err(|_| {
                        io_timeout(format!("the datagrams of {label} took more than {:?}", self.connect_timeout))
                    })?
                    .map_err(|f| failed(&label, f))?;
                pod_quic::endpoint::over(datagrams, None).map_err(|f| failed(&label, f))
            }
        }
    }

    /// Connect to `target` and take only the listener whose key `peer`
    /// names. It gives the stream and the key that the listener proved.
    pub async fn connect(&self, target: &Target, peer: &KeyName) -> Result<(BoxIo, PublicKey), CarrierError> {
        let tls = peer::client_config(self.provider.clone(), self.identity.clone(), peer.clone(), &[self.alpn])
            .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
        let crypto = QuicClientConfig::try_from(tls).map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
        let addr = pin::resolve(NAME, self.resolver.as_deref(), target, self.connect_timeout).await?;
        let mut config = noq::ClientConfig::new(Arc::new(crypto));
        config.transport_config(Arc::new(self.transport_config(addr.is_ipv6(), 1)));
        let endpoint = self.endpoint(target, addr).await?;
        let name = match target {
            Target::Name { host, .. } => host.clone(),
            Target::Addr(addr) => addr.ip().to_string(),
        };
        let own = self.identity.public();
        let shake = io::dial(&endpoint, config, addr, &name, self.alpn, &own, target);
        let (io, key) = timeout(self.handshake_timeout, shake).await.unwrap_or_else(|_| {
            Err(io_timeout(format!("the QUIC handshake with {target} took more than {:?}", self.handshake_timeout)))
        })?;
        Ok((Box::new(io.linger(self.linger, Some(endpoint))) as BoxIo, key))
    }

    /// A listener on a UDP socket of the system at `addr` (port 0 takes a
    /// free port).
    pub async fn bind(&self, addr: SocketAddr) -> Result<QuicListener, CarrierError> {
        listen::bind(self, listen::Socket::System(addr))
    }

    /// A listener over the datagrams of the caller: its address is theirs.
    pub async fn bind_over(&self, datagrams: Arc<dyn Datagrams>) -> Result<QuicListener, CarrierError> {
        listen::bind(self, listen::Socket::Over(datagrams))
    }

    /// The provider makes TLS 1.3, its suite for the first packets of QUIC
    /// and Ed25519.
    fn check_provider(&self) -> Result<(), CarrierError> {
        let own = KeyName::Key(self.identity.public());
        let tls = peer::client_config(self.provider.clone(), self.identity.clone(), own, &[self.alpn])
            .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
        QuicClientConfig::try_from(tls)
            .map(drop)
            .map_err(|e| err(CarrierErrorKind::Unavailable, format!("the TLS provider cannot carry QUIC: {e}")))
    }
}

fn io_timeout(text: String) -> CarrierError {
    err(CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), text)
}

impl Carrier for Quic {
    fn name(&self) -> &'static str {
        NAME
    }

    fn facts(&self) -> Facts {
        Facts::STREAM
    }

    /// The provider can carry QUIC with the pins, and, with no way out of
    /// the caller, a UDP socket binds on the loopback: the host allows the
    /// family. The datagrams of a way out are known at a dial alone.
    fn available(&self) -> Result<(), CarrierError> {
        use std::net::{Ipv4Addr, Ipv6Addr, UdpSocket};
        self.check_provider()?;
        if self.way.is_some() {
            return Ok(());
        }
        UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .or_else(|_| UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)))
            .map(drop)
            .map_err(|e| err(CarrierErrorKind::Unavailable, format!("UDP sockets on the loopback: {e}")))
    }

    /// Dial `KEY@HOST:PORT` ([`Quic::address`]).
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (key, target) = Quic::address(address)?;
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
