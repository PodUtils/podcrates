//! The DNS simplex carrier (feature `dns`, PodUtils/podnet#468 row E2-3,
//! PodUtils/podnet@T-278): the frames of the link go up in the labels of a
//! query name and come down in TXT records, one way at a time. The carrier
//! loses, reorders and duplicates its frames, so the selective-repeat ARQ of
//! [`crate::arq`] runs over it, and the TLS 1.3 of the tcp carrier over the
//! ARQ: each side pins the Ed25519 key of the other (`pod_tls::peer`), and
//! TLS seals each byte, so a query of a stranger can end a link but not
//! change it.
//!
//! * The dialer sends each datagram up as `session || counter || payload`,
//!   base32 in the labels of a name under the zone (`dns::wire`), and reads the
//!   datagrams down of its session from the TXT records of the answer. It
//!   starts a query for each datagram up, and polls when it has none, so the
//!   listener can send its bytes down (call C274).
//! * The listener is an authoritative server on a UDP socket: it reads the
//!   question of each query, takes the datagram up, and answers with the
//!   datagrams down of the session. A new session id starts a session and
//!   its handshake ([`DnsListener`]).
//! * An answer keeps to the EDNS0 size of its query (512 bytes with no OPT
//!   record) and to [`DnsSettings::answer`], so a resolver on the path does
//!   not cut it. A cut answer (the TC bit) gives the whole records before
//!   the cut; the ARQ sends the rest again (calls C291, C292).
//! * The dial address is `KEY@ZONE`; the UDP address of the server is a
//!   field of the carrier ([`Dns::to`], or the datagrams of the caller's way
//!   out, [`Dns::with_way`]). The listen address is `IP:PORT`, and the zone
//!   is [`Dns::zone`]. The ALPN is [`Dns::alpn`] (`podlink/1` by default).
//! * [`Dns::connect_over`] and [`Dns::accept_over`] run the same handshake
//!   over any [`crate::datagram::Datagrams`] of one peer (a shim of loss in
//!   a test, another carrier of datagrams).
//! * A dialer behind an HTTP proxy with no UDP sends its queries over DNS
//!   over HTTPS ([`Dns::over_https`], T-1168): GETs of the wire form to one
//!   resolver, over one connection through [`Dns::with_way`].
//! * The bare form ([`Dns::bare`], T-1167) runs the ARQ with no TLS, for a
//!   caller that authenticates and seals its own stream. Both ends set it: a
//!   bare end and an end with TLS never pair. The dial address is `ZONE`.

mod bare;
mod client;
mod doh;
mod hello;
mod reply;
mod server;
mod wire;

pub mod settings;

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::entropy::{Entropy, OsEntropy};
use pod_core::{Datagrams as AddressedDatagrams, Dial, Target};
use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer::{self, Keys};
use rustls::crypto::CryptoProvider;

use client::{DnsClient, Path};
pub use doh::{DOH_PAUSE, DOH_PAUSE_MAX, DnsOverHttps};
pub use server::DnsListener;
pub use settings::{DnsSettings, DnsSettingsError, EDNS_SIZE, HOLD};

use crate::arq::ArqSettings;
use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Listener, Pending};
use crate::datagram::Datagrams;
use crate::{dgram_tls, pin};

/// The name of the carrier.
pub const NAME: &str = "dns";

/// The ALPN by default: the one of the tcp carrier.
pub const ALPN: &[u8] = crate::tcp::ALPN;

/// The default time limit of a handshake, with the byte of acceptance.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

/// The default count of handshakes that a listener runs at once.
pub const MAX_HANDSHAKES: usize = 64;

/// The default time that a session of a listener can stay silent before the
/// end of its handshake: more than the longest poll interval of a dialer
/// ([`DnsSettings::poll_max`], 2 s) and one lost poll.
pub const HANDSHAKE_QUIET: Duration = Duration::from_secs(5);

/// The DNS carrier: the TLS of this end, the ARQ, the DNS settings, the zone
/// of a listener, the UDP address of the server for a dialer, and the client
/// keys of its listeners. A clone shares the set of client keys.
#[derive(Clone)]
pub struct Dns {
    pub(crate) provider: Arc<CryptoProvider>,
    pub(crate) identity: Arc<Identity>,
    pub(crate) clients: Keys,
    way: Option<Arc<dyn Dial>>,
    https: Option<Arc<DnsOverHttps>>,
    /// The UDP address of the authoritative server or the recursive resolver
    /// that a dial sends its queries to.
    pub server: Option<SocketAddr>,
    /// The zone of a listener: the suffix of each query name.
    pub zone: String,
    /// The ALPN of both ends.
    pub alpn: &'static [u8],
    /// The ARQ at this end.
    pub arq: ArqSettings,
    /// The DNS behaviour: the rate, the count in flight, the answer size.
    pub dns: DnsSettings,
    /// The time limit of a handshake, on each side. A listener keeps the id
    /// of an ended session for this time, so a late query of it starts no
    /// handshake.
    pub handshake_timeout: Duration,
    /// The most handshakes that a listener runs at once (at least 1). A new
    /// session id takes a slot only at a part of its ClientHello, and frees
    /// it at once when its first record is no ClientHello that offers
    /// [`Dns::alpn`]. 64 queries of new ids with no such part keep no dial
    /// out (T-1165: a dial after them took 23 s before, and takes the time of
    /// a handshake now).
    pub max_handshakes: usize,
    /// The time that a session of a listener can stay silent (no query)
    /// before the end of its handshake; the session then ends and frees its
    /// slot. The listener checks each second, so a slot is free within
    /// `handshake_quiet` and 1 s. Give more than the poll interval of the
    /// dialers ([`DnsSettings::poll_max`]). At least 1 ms; by default
    /// [`HANDSHAKE_QUIET`]. A sender of a part of a ClientHello for each new
    /// id holds the slots for this time, not for `handshake_timeout`.
    pub handshake_quiet: Duration,
    /// The bare form: the ARQ with no TLS above it, so no pin of a key and no
    /// seal (false by default). **The caller must authenticate the peer and
    /// seal each byte of the stream itself** (podssh's Noise layer), with
    /// nonces of its own: a stranger on the path can read, change, add and
    /// drop the bytes of a bare stream. Set it at both ends. The first record
    /// of a bare dial is a bare hello, not a ClientHello, so a listener with
    /// TLS refuses a bare dial and a bare listener refuses a dial with TLS;
    /// the refused dial ends at its handshake time limit. A bare dial takes
    /// [`Dns::connect_bare`] (`dial("ZONE")`); a bare listener gives each
    /// client with no key (`accept_peer`) and ignores [`Dns::clients`]. A
    /// datagram of the ARQ carries the bytes of the stream with no record of
    /// TLS around them (T-1167 gives the measure).
    pub bare: bool,
}

impl fmt::Debug for Dns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dns")
            .field("identity", &self.identity.public())
            .field("server", &self.server)
            .field("zone", &self.zone)
            .field("way", &self.way.as_ref().map_or_else(|| "the system".into(), |w| w.label()))
            .field("https", &self.https)
            .field("clients", &self.clients.list())
            .field("alpn", &String::from_utf8_lossy(self.alpn))
            .field("dns", &self.dns)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("max_handshakes", &self.max_handshakes)
            .field("handshake_quiet", &self.handshake_quiet)
            .field("bare", &self.bare)
            .finish()
    }
}

pub(crate) fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

impl Dns {
    /// A carrier that presents `identity` on TLS of `provider`. A dialer
    /// needs [`Dns::to`]; a listener needs [`Dns::zone`] and the client keys
    /// of [`Dns::with_clients`].
    pub fn new(provider: Arc<CryptoProvider>, identity: Arc<Identity>) -> Dns {
        Dns {
            provider,
            identity,
            clients: Keys::default(),
            way: None,
            https: None,
            server: None,
            zone: String::new(),
            alpn: ALPN,
            arq: ArqSettings::default(),
            dns: DnsSettings::default(),
            handshake_timeout: HANDSHAKE_TIMEOUT,
            max_handshakes: MAX_HANDSHAKES,
            handshake_quiet: HANDSHAKE_QUIET,
            bare: false,
        }
    }

    /// The bare form ([`Dns::bare`]): no TLS. The caller must authenticate
    /// and seal the stream itself.
    pub fn without_tls(mut self) -> Dns {
        self.bare = true;
        self
    }

    /// The client keys that a listener takes. The set is shared.
    pub fn with_clients(mut self, clients: Keys) -> Dns {
        self.clients = clients;
        self
    }

    /// The UDP address that a dial sends its queries to.
    pub fn to(mut self, server: SocketAddr) -> Dns {
        self.server = Some(server);
        self
    }

    /// The zone of a listener.
    pub fn zone(mut self, zone: impl Into<String>) -> Dns {
        self.zone = zone.into();
        self
    }

    /// Each dial sends over the datagrams of `way` (`Dial::datagrams`), not
    /// from a socket of the system.
    pub fn with_way(mut self, way: Arc<dyn Dial>) -> Dns {
        self.way = Some(way);
        self
    }

    /// Each dial sends its queries over DNS over HTTPS (`doh`), through the
    /// way out of [`Dns::with_way`], not over UDP to [`Dns::server`]. The
    /// listener is the same.
    pub fn over_https(mut self, doh: DnsOverHttps) -> Dns {
        self.https = Some(Arc::new(doh));
        self
    }

    pub fn clients(&self) -> &Keys {
        &self.clients
    }

    /// The key of the listener and the zone in a dial address `KEY@ZONE`.
    pub fn address(address: &str) -> Result<(KeyName, String), CarrierError> {
        let (key, zone) = pin::key_at(NAME, address, "KEY@ZONE")?;
        wire::payload_mtu(zone).map_err(|e| err(CarrierErrorKind::Usage, e))?;
        Ok((key, zone.to_string()))
    }

    fn check(&self) -> Result<(), CarrierError> {
        self.check_as(false)
    }

    fn check_bare(&self) -> Result<(), CarrierError> {
        self.check_as(true)
    }

    /// The form of the call (`bare` for a call of the bare form), the ARQ and
    /// the DNS settings.
    fn check_as(&self, bare: bool) -> Result<(), CarrierError> {
        match (self.bare, bare) {
            (true, false) => {
                return Err(err(CarrierErrorKind::Usage, "a bare carrier proves no key: use the bare calls"));
            }
            (false, true) => {
                return Err(err(CarrierErrorKind::Usage, "a carrier with TLS takes no bare call: set Dns::bare"));
            }
            _ => {}
        }
        self.arq.check().map_err(|e| err(CarrierErrorKind::Usage, format!("ARQ: {e}")))?;
        self.dns.check().map_err(|e| err(CarrierErrorKind::Usage, format!("DNS: {e}")))
    }

    /// The datagrams of one dial under `zone`: over DNS over HTTPS when
    /// given, else over UDP toward [`Dns::server`].
    async fn client(&self, zone: &str) -> Result<DnsClient, CarrierError> {
        let path = match &self.https {
            Some(doh) => {
                let why = "DNS over HTTPS needs the way out of the dial: give Dns::with_way(way)";
                let way = self.way.clone().ok_or_else(|| err(CarrierErrorKind::Usage, why))?;
                Path::Https(Box::new(doh::Https::new(doh, way).map_err(|e| err(CarrierErrorKind::Usage, e))?))
            }
            None => {
                let why = "no DNS server: give Dns::to(addr)";
                let server = self.server.ok_or_else(|| err(CarrierErrorKind::Usage, why))?;
                Path::Udp(self.socket(server).await?, server)
            }
        };
        let mut sid = [0u8; 4];
        OsEntropy.fill(&mut sid).map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
        DnsClient::new(path, zone.to_string(), u32::from_le_bytes(sid), self.dns)
            .map_err(|e| err(CarrierErrorKind::Usage, e))
    }

    /// The socket of one dial toward the server.
    async fn socket(&self, server: SocketAddr) -> Result<Arc<dyn AddressedDatagrams>, CarrierError> {
        match &self.way {
            None => {
                let any: SocketAddr = if server.is_ipv6() {
                    (Ipv6Addr::UNSPECIFIED, 0).into()
                } else {
                    (Ipv4Addr::UNSPECIFIED, 0).into()
                };
                let socket = pod_core::SystemDatagrams::bind(any)
                    .await
                    .map_err(|e| CarrierError::io(NAME, format!("a UDP socket toward {server}"), e))?;
                Ok(Arc::new(socket))
            }
            Some(way) => {
                let label = way.label();
                way.datagrams().await.map_err(|f| pin::of_failure(NAME, &Target::Addr(server), &label, f))
            }
        }
    }

    /// Connect under `zone` and take only the listener whose key `peer`
    /// names. It gives the stream and the key that the listener proved.
    pub async fn connect(&self, zone: &str, peer: &KeyName) -> Result<(BoxIo, PublicKey), CarrierError> {
        self.check()?;
        let client = self.client(zone).await?;
        let target = Target::Name { host: zone.to_string(), port: 53 };
        dgram_tls::connect(
            NAME,
            client,
            &self.provider,
            &self.identity,
            peer,
            self.alpn,
            self.arq,
            &target,
            self.handshake_timeout,
        )
        .await
    }

    /// The dial's handshake over the datagrams of one peer: the ARQ, TLS with
    /// the pin of `peer`, and the byte of acceptance. `target` names the peer
    /// (and its SNI when it is a name).
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

    /// The bare dial under `zone` ([`Dns::bare`]): the ARQ and the bare
    /// hello, no TLS and no key. The caller authenticates the listener and
    /// seals the stream itself.
    pub async fn connect_bare(&self, zone: &str) -> Result<BoxIo, CarrierError> {
        self.check_bare()?;
        let client = self.client(zone).await?;
        bare::connect(client, self.alpn, self.arq, &zone, self.handshake_timeout).await
    }

    /// The bare dial over the datagrams of one peer ([`Dns::bare`]).
    pub async fn connect_bare_over<D: Datagrams>(
        &self,
        datagrams: D,
        target: &(dyn fmt::Display + Sync),
    ) -> Result<BoxIo, CarrierError> {
        self.check_bare()?;
        bare::connect(datagrams, self.alpn, self.arq, target, self.handshake_timeout).await
    }

    /// The bare accept over the datagrams of one peer ([`Dns::bare`]): a
    /// first record that is no bare hello of [`Dns::alpn`] is refused.
    pub async fn accept_bare_over<D: Datagrams>(
        &self,
        datagrams: D,
        from: &(dyn fmt::Display + Sync),
    ) -> Result<BoxIo, CarrierError> {
        self.check_bare()?;
        bare::accept(datagrams, self.alpn, self.arq, from, self.handshake_timeout).await
    }

    /// A listener on a UDP socket of the system at `addr` (port 0 takes a
    /// free port). The zone is [`Dns::zone`].
    pub async fn bind(&self, addr: SocketAddr) -> Result<DnsListener, CarrierError> {
        let socket = pod_core::SystemDatagrams::bind(addr)
            .await
            .map_err(|e| CarrierError::io(NAME, format!("bind {addr}"), e))?;
        self.bind_over(Arc::new(socket)).await
    }

    /// A listener over the datagrams of the caller: its address is theirs.
    pub async fn bind_over(&self, socket: Arc<dyn AddressedDatagrams>) -> Result<DnsListener, CarrierError> {
        self.check_as(self.bare)?;
        if self.zone.is_empty() {
            return Err(err(CarrierErrorKind::Usage, "no zone: give Dns::zone(name)"));
        }
        server::bind(self, socket, self.zone.clone())
    }
}

impl Carrier for Dns {
    fn name(&self) -> &'static str {
        NAME
    }

    fn facts(&self) -> Facts {
        // A query for each small batch, and a poll while idle.
        Facts::with_cost(crate::carrier::Cost::Highest)
    }

    fn available(&self) -> Result<(), CarrierError> {
        use std::net::UdpSocket;
        peer::supports(&self.provider).map_err(|why| err(CarrierErrorKind::Unavailable, why))?;
        self.arq.check().map_err(|e| err(CarrierErrorKind::Unavailable, format!("ARQ: {e}")))?;
        self.dns.check().map_err(|e| err(CarrierErrorKind::Unavailable, format!("DNS: {e}")))?;
        if self.way.is_some() {
            return Ok(());
        }
        if self.https.is_some() {
            return Err(err(CarrierErrorKind::Unavailable, "DNS over HTTPS with no way out of the dial"));
        }
        UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .or_else(|_| UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)))
            .map(drop)
            .map_err(|e| {
                err(
                    CarrierErrorKind::Unavailable,
                    format!("UDP sockets on the loopback: {}", pod_core::text::io_words(&e)),
                )
            })
    }

    /// Dial `KEY@ZONE` ([`Dns::address`]); a bare carrier dials `ZONE`.
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            if self.bare {
                if address.contains('@') {
                    let why = format!("{address:?}: a bare dial pins no key: give ZONE");
                    return Err(err(CarrierErrorKind::Usage, why));
                }
                return self.connect_bare(address).await;
            }
            let (key, zone) = Dns::address(address)?;
            self.connect(&zone, &key).await.map(|(io, _)| io)
        })
    }

    /// Listen at `IP:PORT` ([`Dns::zone`] is the zone).
    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            let addr = address.parse::<SocketAddr>().map_err(|_| {
                err(CarrierErrorKind::Usage, format!("{address:?} is no IP:PORT (127.0.0.1:53, [::]:53)"))
            })?;
            Ok(Box::new(self.bind(addr).await?) as Box<dyn Listener>)
        })
    }
}
