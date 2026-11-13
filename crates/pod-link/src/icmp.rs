//! The ICMP echo carrier (feature `icmp`, PodUtils/podnet#469 row E3-2,
//! PodUtils/podnet@T-282): the frames of the link ride in the data of ICMP
//! echo messages. The carrier loses, reorders and duplicates its frames, so
//! the selective-repeat ARQ of [`crate::arq`] runs over it, and the TLS 1.3
//! of the tcp carrier over the ARQ: each side pins the Ed25519 key of the
//! other (`pod_tls::peer`), and TLS seals each byte, so an echo of a stranger
//! can end a link but not change it.
//!
//! * Each datagram of the ARQ rides in the data of one echo message, with a
//!   session that tells one link from another on a shared socket
//!   ([`echo::EchoFrames`], [`wire`]).
//! * The real path is a raw ICMPv4 socket, which needs `CAP_NET_RAW` or uid
//!   0. [`Icmp::available`] says so when the host refuses it, and the dialer
//!   and the listener give a typed `Unavailable` (call C276). A test with no
//!   privilege runs the framing over a memory pair instead
//!   ([`Icmp::connect_over`], [`Icmp::accept_over`]).
//! * The dial address is `KEY@HOST` (an IPv4 address or a name). The listen
//!   address is an IPv4 address (`0.0.0.0`, `127.0.0.1`). The ALPN is
//!   [`Icmp::alpn`] (`podlink/1` by default).

pub mod settings;
pub mod wire;

mod echo;
#[cfg(unix)]
mod listen;
#[cfg(unix)]
mod socket;

use std::fmt;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Resolve, Target};
use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer::{self, Keys};
use rustls::crypto::CryptoProvider;

pub use echo::EchoFrames;
#[cfg(unix)]
pub use listen::IcmpListener;
pub use settings::{IcmpSettings, IcmpSettingsError, PAYLOAD};

use crate::arq::ArqSettings;
use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Listener, Pending};
use crate::datagram::Datagrams;
use crate::{dgram_tls, pin};

/// The name of the carrier.
pub const NAME: &str = "icmp";

/// The ALPN by default: the one of the tcp carrier.
pub const ALPN: &[u8] = crate::tcp::ALPN;

/// The default time limit of the resolution of a name.
pub const CONNECT_TIMEOUT: Duration = pod_core::backoff::CONNECT_TIMEOUT;

/// The default time limit of a handshake, with the byte of acceptance.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

/// The default count of handshakes that a listener runs at once.
pub const MAX_HANDSHAKES: usize = 64;

/// The ICMP echo carrier: the TLS of this end, the ARQ, the ICMP settings,
/// and the client keys of its listeners. A clone shares the set of client
/// keys.
#[derive(Clone)]
pub struct Icmp {
    pub(crate) provider: Arc<CryptoProvider>,
    pub(crate) identity: Arc<Identity>,
    pub(crate) clients: Keys,
    resolver: Option<Arc<dyn Resolve>>,
    /// The ALPN of both ends.
    pub alpn: &'static [u8],
    /// The ARQ at this end.
    pub arq: ArqSettings,
    /// The ICMP behaviour (the payload size).
    pub icmp: IcmpSettings,
    /// The time limit of the resolution of a name.
    pub connect_timeout: Duration,
    /// The time limit of a handshake, on each side.
    pub handshake_timeout: Duration,
    /// The most handshakes that a listener runs at once (at least 1).
    pub max_handshakes: usize,
}

impl fmt::Debug for Icmp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Icmp")
            .field("identity", &self.identity.public())
            .field("clients", &self.clients.list())
            .field("alpn", &String::from_utf8_lossy(self.alpn))
            .field("icmp", &self.icmp)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("max_handshakes", &self.max_handshakes)
            .finish()
    }
}

pub(crate) fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

impl Icmp {
    /// A carrier that presents `identity` on TLS of `provider`. Its listeners
    /// take no client until [`Icmp::with_clients`] gives the keys.
    pub fn new(provider: Arc<CryptoProvider>, identity: Arc<Identity>) -> Icmp {
        Icmp {
            provider,
            identity,
            clients: Keys::default(),
            resolver: None,
            alpn: ALPN,
            arq: ArqSettings::default(),
            icmp: IcmpSettings::default(),
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            max_handshakes: MAX_HANDSHAKES,
        }
    }

    /// The client keys that a listener takes. The set is shared.
    pub fn with_clients(mut self, clients: Keys) -> Icmp {
        self.clients = clients;
        self
    }

    /// A name of a dial address resolves through `resolver`.
    pub fn with_resolver(mut self, resolver: Arc<dyn Resolve>) -> Icmp {
        self.resolver = Some(resolver);
        self
    }

    pub fn clients(&self) -> &Keys {
        &self.clients
    }

    /// The key of the peer and its address in a dial address `KEY@HOST`.
    pub fn address(address: &str) -> Result<(KeyName, Target), CarrierError> {
        let (key, host) = pin::key_at(NAME, address, "KEY@HOST")?;
        let target = match host.parse::<std::net::IpAddr>() {
            Ok(ip) => Target::Addr((ip, 0).into()),
            Err(_) => Target::Name { host: host.to_string(), port: 0 },
        };
        Ok((key, target))
    }

    fn check(&self) -> Result<(), CarrierError> {
        self.arq.check().map_err(|e| err(CarrierErrorKind::Usage, format!("ARQ: {e}")))?;
        self.icmp.check().map_err(|e| err(CarrierErrorKind::Usage, format!("ICMP: {e}")))
    }

    /// The MTU of the raw socket: the payload with the echo header and the
    /// session.
    #[cfg(unix)]
    fn socket_mtu(&self) -> usize {
        self.icmp.payload + wire::HEADER + wire::SESSION
    }

    /// Connect to `target` and take only the peer whose key `peer` names.
    #[cfg(unix)]
    pub async fn connect(&self, target: &Target, peer: &KeyName) -> Result<(BoxIo, PublicKey), CarrierError> {
        use pod_core::entropy::{Entropy, OsEntropy};
        self.check()?;
        let addr = pin::resolve(NAME, self.resolver.as_deref(), target, self.connect_timeout).await?;
        if !addr.is_ipv4() {
            return Err(err(CarrierErrorKind::Usage, format!("{addr} is not IPv4: the ICMP carrier is IPv4 only")));
        }
        let raw = socket::RawIcmp::open(self.socket_mtu()).map_err(listen::icmp_open)?;
        let mut sid = [0u8; 4];
        OsEntropy.fill(&mut sid).map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
        let session = u32::from_le_bytes(sid);
        let client = listen::ClientIcmp::new(Arc::new(raw), addr);
        let frames = EchoFrames::new(client, wire::REQUEST, session, session as u16);
        self.connect_over(frames, target, peer).await
    }

    /// The dial's handshake over the datagrams of one peer: the ARQ, TLS with
    /// the pin of `peer`, and the byte of acceptance. For a test, the
    /// datagrams are an [`EchoFrames`] over a memory pair.
    pub async fn connect_over<D: Datagrams>(
        &self,
        datagrams: D,
        target: &Target,
        peer: &KeyName,
    ) -> Result<(BoxIo, PublicKey), CarrierError> {
        self.check()?;
        dgram_tls::connect(
            NAME,
            datagrams,
            &self.provider,
            &self.identity,
            peer,
            self.alpn,
            self.arq,
            target,
            self.handshake_timeout,
        )
        .await
    }

    /// The listener's handshake over the datagrams of one peer: the ARQ, TLS
    /// that takes a pinned client key, and the byte of acceptance.
    pub async fn accept_over<D: Datagrams>(
        &self,
        datagrams: D,
        from: &(dyn fmt::Display + Sync),
    ) -> Result<(BoxIo, PublicKey), CarrierError> {
        self.check()?;
        let acceptor = dgram_tls::acceptor(NAME, &self.provider, &self.identity, &self.clients, self.alpn)?;
        dgram_tls::accept(NAME, datagrams, acceptor, self.alpn, self.arq, from, self.handshake_timeout).await
    }

    /// A listener on a raw ICMP socket bound to `ip`.
    #[cfg(unix)]
    pub async fn bind(&self, ip: Ipv4Addr) -> Result<IcmpListener, CarrierError> {
        self.check()?;
        listen::bind(self, ip)
    }
}

impl Carrier for Icmp {
    fn name(&self) -> &'static str {
        NAME
    }

    fn facts(&self) -> Facts {
        Facts { cost: crate::carrier::Cost::High, ..Facts::STREAM }
    }

    fn available(&self) -> Result<(), CarrierError> {
        peer::supports(&self.provider).map_err(|why| err(CarrierErrorKind::Unavailable, why))?;
        self.arq.check().map_err(|e| err(CarrierErrorKind::Unavailable, format!("ARQ: {e}")))?;
        self.icmp.check().map_err(|e| err(CarrierErrorKind::Unavailable, format!("ICMP: {e}")))?;
        #[cfg(unix)]
        {
            socket::RawIcmp::open(self.socket_mtu()).map(drop).map_err(listen::icmp_open)
        }
        #[cfg(not(unix))]
        {
            Err(err(CarrierErrorKind::Unavailable, "the ICMP carrier runs on unix only"))
        }
    }

    /// Dial `KEY@HOST` ([`Icmp::address`]).
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (key, target) = Icmp::address(address)?;
            #[cfg(unix)]
            {
                self.connect(&target, &key).await.map(|(io, _)| io)
            }
            #[cfg(not(unix))]
            {
                let _ = (key, target);
                Err(err(CarrierErrorKind::Unavailable, "the ICMP carrier runs on unix only"))
            }
        })
    }

    /// Listen at an IPv4 address (`0.0.0.0`, `127.0.0.1`).
    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            let ip = address
                .parse::<Ipv4Addr>()
                .map_err(|_| err(CarrierErrorKind::Usage, format!("{address:?} is no IPv4 address (0.0.0.0)")))?;
            #[cfg(unix)]
            {
                Ok(Box::new(self.bind(ip).await?) as Box<dyn Listener>)
            }
            #[cfg(not(unix))]
            {
                let _ = ip;
                Err(err(CarrierErrorKind::Unavailable, "the ICMP carrier runs on unix only"))
            }
        })
    }
}
