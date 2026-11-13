//! The udp carrier (feature `udp`): KCP over UDP (PodUtils/podnet#468, row
//! E2-1), with the TLS 1.3 of the tcp carrier above it: each side pins the
//! Ed25519 key of the other (`pod_tls::peer`), and TLS seals each byte, so
//! a datagram of a stranger can end a link but not change it.
//!
//! * The wire of each datagram is the wire of KCP (the crate `kcp`, a port
//!   of ikcp) in stream mode: a conversation of 32 bits that the dialer
//!   draws, segments of 1 KiB in datagrams of at most 1200 bytes, the
//!   congestion window of KCP ([`KcpSettings`], call C271).
//! * The dial goes to `KEY@HOST:PORT`, from a UDP socket of the system or
//!   over the datagrams of the caller's way out ([`Udp::with_way`]); a name
//!   resolves through the caller's resolver ([`Udp::with_resolver`]), else
//!   through the system.
//! * A listener binds `IP:PORT` (or runs over the caller's datagrams,
//!   [`Udp::bind_over`]), and gives the datagrams of each address and
//!   conversation to its session. A new session starts at the first
//!   segment of a conversation alone, at most [`Udp::max_handshakes`] at
//!   once, each within [`Udp::handshake_timeout`].
//! * TLS 1.3 alone, the ALPN [`Udp::alpn`] (`podlink/1` by default), no
//!   resumption. After its handshake the listener writes the byte 1, as the
//!   tcp carrier does.
//! * [`Udp::connect_over`] and [`Udp::accept_over`] run the same handshake
//!   over any [`crate::datagram::Datagrams`] of one peer (a shim of loss in
//!   a test, another carrier of datagrams).

mod ends;
mod listen;
mod settings;
mod stream;

use std::fmt;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::entropy::{Entropy, OsEntropy};
use pod_core::{Datagrams as AddressedDatagrams, Dial, Resolve, Target};
use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer::{self, Keys};
use rustls::crypto::CryptoProvider;
use tokio::io::AsyncReadExt as _;
use tokio::time::timeout;

pub use listen::UdpListener;
pub use settings::{IDLE, KCP_HEADER, KcpSettings, KcpSettingsError, MAX_WINDOW, MIN_WINDOW, MTU, SEGMENT};
pub use stream::KcpStream;

use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Listener, Pending};
use crate::datagram::Datagrams;
use crate::pin;

/// The name of the carrier.
pub const NAME: &str = "udp";

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

/// The udp carrier: the TLS of this end (its provider and its identity),
/// the way of its datagrams, KCP, and the client keys of its listeners. A
/// clone shares the set of client keys.
#[derive(Clone)]
pub struct Udp {
    provider: Arc<CryptoProvider>,
    identity: Arc<Identity>,
    way: Option<Arc<dyn Dial>>,
    resolver: Option<Arc<dyn Resolve>>,
    clients: Keys,
    /// The ALPN of both ends.
    pub alpn: &'static [u8],
    /// KCP at this end.
    pub kcp: KcpSettings,
    /// The time limit of the resolution of a name and of the datagrams of
    /// the way out.
    pub connect_timeout: Duration,
    /// The time limit of a handshake, on each side.
    pub handshake_timeout: Duration,
    /// The most handshakes that a listener runs at once (at least 1).
    pub max_handshakes: usize,
}

impl fmt::Debug for Udp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Udp")
            .field("identity", &self.identity.public())
            .field("way", &self.way.as_ref().map_or_else(|| "the system".into(), |w| w.label()))
            .field("resolver", &self.resolver.as_ref().map_or_else(|| "the system".into(), |r| r.label()))
            .field("clients", &self.clients.list())
            .field("alpn", &String::from_utf8_lossy(self.alpn))
            .field("kcp", &self.kcp)
            .field("connect_timeout", &self.connect_timeout)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("max_handshakes", &self.max_handshakes)
            .finish()
    }
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

impl Udp {
    /// A carrier that presents `identity` on TLS of `provider` and sends
    /// from a UDP socket of the system. Its listeners take no client until
    /// [`Udp::with_clients`] gives the keys.
    pub fn new(provider: Arc<CryptoProvider>, identity: Arc<Identity>) -> Udp {
        Udp {
            provider,
            identity,
            way: None,
            resolver: None,
            clients: Keys::default(),
            alpn: ALPN,
            kcp: KcpSettings::default(),
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            max_handshakes: MAX_HANDSHAKES,
        }
    }

    /// The client keys that a listener takes. The set is shared: a change
    /// of it applies at the next handshake of each listener.
    pub fn with_clients(mut self, clients: Keys) -> Udp {
        self.clients = clients;
        self
    }

    /// Each dial sends over the datagrams of `way` (`Dial::datagrams`), not
    /// from a socket of the system.
    pub fn with_way(mut self, way: Arc<dyn Dial>) -> Udp {
        self.way = Some(way);
        self
    }

    /// A name of a dial address resolves through `resolver`.
    pub fn with_resolver(mut self, resolver: Arc<dyn Resolve>) -> Udp {
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

    fn check(&self) -> Result<(), CarrierError> {
        self.kcp.check().map_err(|e| err(CarrierErrorKind::Usage, format!("KCP: {e}")))
    }

    /// The socket of one dial toward `addr`.
    async fn socket(&self, target: &Target, addr: SocketAddr) -> Result<Arc<dyn AddressedDatagrams>, CarrierError> {
        match &self.way {
            None => {
                let any: SocketAddr =
                    if addr.is_ipv6() { (Ipv6Addr::UNSPECIFIED, 0).into() } else { (Ipv4Addr::UNSPECIFIED, 0).into() };
                let socket = pod_core::SystemDatagrams::bind(any)
                    .await
                    .map_err(|e| CarrierError::io(NAME, format!("a UDP socket toward {target}"), e))?;
                Ok(Arc::new(socket))
            }
            Some(way) => {
                let label = way.label();
                timeout(self.connect_timeout, way.datagrams())
                    .await
                    .map_err(|_| {
                        err(
                            CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                            format!("the datagrams of {label} took more than {:?}", self.connect_timeout),
                        )
                    })?
                    .map_err(|f| pin::of_failure(NAME, target, &label, f))
            }
        }
    }

    /// Connect to `target` and take only the listener whose key `peer`
    /// names. It gives the stream and the key that the listener proved.
    pub async fn connect(&self, target: &Target, peer: &KeyName) -> Result<(BoxIo, PublicKey), CarrierError> {
        self.check()?;
        let addr = pin::resolve(NAME, self.resolver.as_deref(), target, self.connect_timeout).await?;
        let socket = self.socket(target, addr).await?;
        let toward = ends::Toward { socket, peer: addr, mtu: self.kcp.mtu };
        self.connect_over(toward, target, peer).await
    }

    /// The dial's handshake over the datagrams of one peer: KCP with a new
    /// conversation, TLS with the pin of `peer`, and the byte of acceptance.
    /// `target` names the peer (and its SNI when it is a name).
    pub async fn connect_over<D: Datagrams>(
        &self,
        datagrams: D,
        target: &Target,
        peer: &KeyName,
    ) -> Result<(BoxIo, PublicKey), CarrierError> {
        self.check()?;
        let config = peer::client_config(self.provider.clone(), self.identity.clone(), peer.clone(), &[self.alpn])
            .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
        let name = pin::server_name(NAME, target)?;
        let mut conv = [0u8; 4];
        OsEntropy.fill(&mut conv).map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
        let stream = KcpStream::new(datagrams, Some(u32::from_le_bytes(conv)), self.kcp)
            .map_err(|e| err(CarrierErrorKind::Usage, format!("KCP: {e}")))?;
        let own = self.identity.public();
        let shake = async {
            let mut tls = tokio_rustls::TlsConnector::from(config)
                .connect(name, stream)
                .await
                .map_err(|e| pin::tls_error(NAME, format!("the TLS handshake with {target}"), e))?;
            let conn = tls.get_ref().1;
            pin::check_alpn(NAME, conn.alpn_protocol(), self.alpn, target)?;
            let key = pin::proved(NAME, conn.peer_certificates(), target)?;
            let mut word = [0u8];
            match tls.read_exact(&mut word).await {
                Ok(_) if word[0] == ACCEPTED => Ok((Box::new(tls) as BoxIo, key)),
                Ok(_) => Err(err(
                    CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                    format!("the listener at {target} sent {} where its acceptance goes", word[0]),
                )),
                Err(e) => Err(pin::answer_error(NAME, target, &own, e)),
            }
        };
        timeout(self.handshake_timeout, shake).await.unwrap_or_else(|_| {
            Err(err(
                CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                format!("the handshake with {target} took more than {:?}", self.handshake_timeout),
            ))
        })
    }

    /// The listener's handshake over the datagrams of one peer: KCP in the
    /// conversation of its first datagram, TLS that takes a client key of
    /// [`Udp::clients`], and the byte of acceptance.
    pub async fn accept_over<D: Datagrams>(
        &self,
        datagrams: D,
        from: &(dyn fmt::Display + Sync),
    ) -> Result<(BoxIo, PublicKey), CarrierError> {
        self.check()?;
        let acceptor = listen::acceptor(self)?;
        let stream =
            KcpStream::new(datagrams, None, self.kcp).map_err(|e| err(CarrierErrorKind::Usage, format!("KCP: {e}")))?;
        listen::handshake(acceptor, stream, self.alpn, from, self.handshake_timeout).await
    }

    /// A listener on a UDP socket of the system at `addr` (port 0 takes a
    /// free port).
    pub async fn bind(&self, addr: SocketAddr) -> Result<UdpListener, CarrierError> {
        let socket = pod_core::SystemDatagrams::bind(addr)
            .await
            .map_err(|e| CarrierError::io(NAME, format!("bind {addr}"), e))?;
        self.bind_over(Arc::new(socket)).await
    }

    /// A listener over the datagrams of the caller: its address is theirs.
    pub async fn bind_over(&self, datagrams: Arc<dyn AddressedDatagrams>) -> Result<UdpListener, CarrierError> {
        self.check()?;
        listen::bind(self, datagrams)
    }
}

impl Carrier for Udp {
    fn name(&self) -> &'static str {
        NAME
    }

    fn facts(&self) -> Facts {
        Facts::STREAM
    }

    /// The provider makes TLS 1.3 and verifies Ed25519, the settings of KCP
    /// hold, and, with no way out of the caller, a UDP socket binds on the
    /// loopback: the host allows the family. Where it does not, the carrier
    /// sends no datagram.
    fn available(&self) -> Result<(), CarrierError> {
        use std::net::UdpSocket;
        peer::supports(&self.provider).map_err(|why| err(CarrierErrorKind::Unavailable, why))?;
        self.kcp.check().map_err(|e| err(CarrierErrorKind::Unavailable, format!("KCP: {e}")))?;
        if self.way.is_some() {
            return Ok(());
        }
        UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .or_else(|_| UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)))
            .map(drop)
            .map_err(|e| err(CarrierErrorKind::Unavailable, format!("UDP sockets on the loopback: {e}")))
    }

    /// Dial `KEY@HOST:PORT` ([`Udp::address`]).
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (key, target) = Udp::address(address)?;
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
