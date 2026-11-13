//! The ws carrier (feature `ws`): a WebSocket (RFC 6455) over TLS 1.3,
//! each side pinning the Ed25519 key of the other as the tcp carrier does
//! (`pod_tls::peer`). The bytes of the link go in binary messages.
//!
//! * The dial goes through the way out of the caller (`pod_core::Dial`) to
//!   `KEY@wss://HOST[:PORT]/PATH`. KEY names the listener's key as
//!   `pod_identity::KeyName` reads it. The request goes to PATH with the
//!   `Host` of the URL, so that a front that routes by path or by name can
//!   carry it, as long as it does not end the TLS.
//! * A listener binds `IP:PORT` and runs the accept loop of `pod-ws`
//!   ([`pod_ws::serve`]): the deadline of each stage and the caps of the
//!   connections before the caller accepts them ([`Ws::serve`]). It takes a
//!   client only when its key is in [`Ws::clients`], and any path.
//! * TLS 1.3 alone, the ALPN `http/1.1`, no resumption. The 101 comes after
//!   the listener checked the key of the dialer, so a refused key is an
//!   error of the dial. The dialer then sends an empty binary message, the
//!   first frame that the listener waits for.
//! * The rules of podnet's carrier (PodUtils/podnet@T-269), as settings: the
//!   subprotocol of [`Ws::protocol`], which the dial offers and the 101
//!   names, and which the listener asks for (426); binary messages alone,
//!   as a text message is an error of protocol (a Close 1003); a Ping each
//!   20 s, and the end of the session when no frame comes within 10 s
//!   ([`Beat`]).
//!
//! A key that is not pinned is a [`CarrierErrorKind::Refused`] error (77)
//! that names the key that the peer proved.

mod listen;
mod pump;

use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Dial, Target};
use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer::{self, Keys};
use pod_ws::RelaySession;
use pod_ws::serve::Serve;
use pod_ws::url::WsUrl;
use rustls::crypto::CryptoProvider;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::time::timeout;

pub use listen::WsListener;
pub use pump::{Beat, PING_EVERY, PONG_WITHIN, carry};

use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Listener, Pending};
use crate::pin;

/// The name of the carrier.
pub const NAME: &str = "ws";

/// The ALPN of a WebSocket over TLS: an upgrade does not go over HTTP/2.
pub const ALPN: &[u8] = b"http/1.1";

/// The user agent of a dial.
pub const USER_AGENT: &str = "pod-link";

/// The default time limit of a dial through the way out.
pub const CONNECT_TIMEOUT: Duration = pod_core::backoff::CONNECT_TIMEOUT;

/// The default time limit of the TLS handshake and the upgrade of a dial.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest head of the answer to the upgrade.
const MAX_ANSWER: usize = 16 << 10;

/// The ws carrier: the TLS of this end (its provider and its identity),
/// the way out of its dials, the client keys and the accept loop of its
/// listeners. A clone shares the set of client keys.
#[derive(Clone)]
pub struct Ws {
    provider: Arc<CryptoProvider>,
    identity: Arc<Identity>,
    way: Arc<dyn Dial>,
    clients: Keys,
    /// The time limit of a dial through the way out.
    pub connect_timeout: Duration,
    /// The time limit of the TLS handshake and the upgrade of a dial.
    pub handshake_timeout: Duration,
    /// The deadlines and the caps of the accept loop of a listener. Its
    /// `protocol` is not read: [`Ws::protocol`] goes in its place.
    pub serve: Serve,
    /// The subprotocol of both ends, a constant of the consumer (podnet's
    /// is `podnet/1`): the dial offers it and refuses a 101 that does not
    /// name it; a listener refuses a request that does not offer it (426).
    /// None offers none, and refuses a 101 that names one.
    pub protocol: Option<&'static str>,
    /// The Pings of each session, dialed or accepted.
    pub beat: Beat,
}

impl fmt::Debug for Ws {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ws")
            .field("identity", &self.identity.public())
            .field("way", &self.way.label())
            .field("clients", &self.clients.list())
            .field("connect_timeout", &self.connect_timeout)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("serve", &self.serve)
            .field("protocol", &self.protocol)
            .field("beat", &self.beat)
            .finish()
    }
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

impl Ws {
    /// A carrier that presents `identity` on TLS of `provider` and dials
    /// through `way`. Its listeners take no client until
    /// [`Ws::with_clients`] gives the keys.
    pub fn new(provider: Arc<CryptoProvider>, identity: Arc<Identity>, way: Arc<dyn Dial>) -> Ws {
        Ws {
            provider,
            identity,
            way,
            clients: Keys::default(),
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            serve: Serve::default(),
            protocol: None,
            beat: Beat::default(),
        }
    }

    /// The subprotocol of both ends ([`Ws::protocol`]).
    pub fn with_protocol(mut self, protocol: &'static str) -> Ws {
        self.protocol = Some(protocol);
        self
    }

    /// The settings of the carrier: a subprotocol name that
    /// `pod_ws::server::is_protocol_name` takes, each
    /// time of [`Ws::beat`] more than zero. The accept loop checks its own.
    pub fn check(&self) -> Result<(), CarrierError> {
        if let Some(protocol) = self.protocol
            && !pod_ws::server::is_protocol_name(protocol)
        {
            return Err(err(
                CarrierErrorKind::Usage,
                format!("the subprotocol {protocol:?} is not visible ASCII with no comma"),
            ));
        }
        self.beat.check().map_err(|why| err(CarrierErrorKind::Usage, why))
    }

    /// The client keys that a listener takes. The set is shared: a change
    /// of it applies at the next handshake of each listener.
    pub fn with_clients(mut self, clients: Keys) -> Ws {
        self.clients = clients;
        self
    }

    pub fn clients(&self) -> &Keys {
        &self.clients
    }

    /// The key of the listener and its URL in a dial address
    /// `KEY@wss://HOST[:PORT]/PATH`.
    pub fn address(address: &str) -> Result<(KeyName, WsUrl), CarrierError> {
        let (key, url) = address.split_once('@').ok_or_else(|| {
            err(
                CarrierErrorKind::Usage,
                format!("{address:?}: give KEY@wss://HOST:PORT/PATH, with the key of the listener"),
            )
        })?;
        let key = KeyName::parse(key).ok_or_else(|| {
            err(
                CarrierErrorKind::Usage,
                format!(
                    "{key:?} is no key: give the key in hex or base32, its SHA256: fingerprint or its display form"
                ),
            )
        })?;
        let url = WsUrl::parse(url).map_err(|why| err(CarrierErrorKind::Usage, why))?;
        if !url.tls {
            return Err(err(CarrierErrorKind::Usage, "the ws carrier dials wss:// alone: its pins need TLS"));
        }
        Ok((key, url))
    }

    /// Connect to `url` through the way out and take only the listener
    /// whose key `peer` names. It gives the stream and the key that the
    /// listener proved.
    pub async fn connect(&self, url: &WsUrl, peer: &KeyName) -> Result<(BoxIo, PublicKey), CarrierError> {
        self.check()?;
        let config = peer::client_config(self.provider.clone(), self.identity.clone(), peer.clone(), &[ALPN])
            .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
        let target = match url.host.parse::<IpAddr>() {
            Ok(ip) => Target::Addr(SocketAddr::new(ip, url.port)),
            Err(_) => Target::Name { host: url.host.clone(), port: url.port },
        };
        let name = pin::server_name(NAME, &target)?;
        let connected = pin::dial(NAME, &*self.way, &target, self.connect_timeout).await?;
        let own = self.identity.public();
        let shake = async {
            let mut tls = tokio_rustls::TlsConnector::from(config)
                .connect(name, connected.io)
                .await
                .map_err(|e| pin::tls_error(NAME, format!("the TLS handshake with {target}"), e))?;
            let conn = tls.get_ref().1;
            pin::check_alpn(NAME, conn.alpn_protocol(), ALPN, &target)?;
            let key = pin::proved(NAME, conn.peer_certificates(), &target)?;
            let pending = upgrade(&mut tls, url, &target, &own, self.protocol).await?;
            let session = RelaySession::new(tls, pending, None, pod_ws::client::WRITE_TIMEOUT).naming("the listener");
            // The first frame, which the accept loop of the listener waits for.
            session.send_binary(&[]).await.map_err(|e| {
                err(CarrierErrorKind::Io(io::ErrorKind::BrokenPipe), format!("the first frame to {target}: {e}"))
            })?;
            Ok((session, key))
        };
        let (session, key) = timeout(self.handshake_timeout, shake).await.unwrap_or_else(|_| {
            Err(err(
                CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                format!("the TLS handshake and the upgrade with {target} took more than {:?}", self.handshake_timeout),
            ))
        })?;
        Ok((carry(session, self.beat), key))
    }

    /// A listener at `addr` (port 0 takes a free port).
    pub async fn bind(&self, addr: SocketAddr) -> Result<WsListener, CarrierError> {
        listen::bind(self, addr).await
    }
}

/// The upgrade of a dial that offers `protocol`: the request, and the 101
/// that the listener sends after it took the key of this end, and that
/// names `protocol`. The bytes after the head.
async fn upgrade<S>(
    tls: &mut S,
    url: &WsUrl,
    target: &Target,
    own: &PublicKey,
    protocol: Option<&str>,
) -> Result<Vec<u8>, CarrierError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let key = pod_ws::handshake::generate_key().map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
    let request = pod_ws::url::offering(url, &key, USER_AGENT, protocol)
        .map_err(|e| err(CarrierErrorKind::Usage, e.to_string()))?;
    let answer = |e| pin::answer_error(NAME, target, own, e);
    tls.write_all(&request).await.map_err(answer)?;
    tls.flush().await.map_err(answer)?;
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        if buf.len() > MAX_ANSWER {
            return Err(err(
                CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                format!("the listener at {target} sent no end of its answer in {MAX_ANSWER} bytes"),
            ));
        }
        let n = tls.read(&mut chunk).await.map_err(answer)?;
        if n == 0 {
            return Err(answer(io::ErrorKind::UnexpectedEof.into()));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    pod_ws::handshake::check_response(&head, &key)
        .and_then(|()| pod_ws::handshake::check_chosen_protocol(&head, protocol))
        .map_err(|e| {
            err(CarrierErrorKind::Io(io::ErrorKind::InvalidData), format!("the upgrade at {}: {e}", url.shown()))
        })?;
    Ok(buf.split_off(end))
}

impl Carrier for Ws {
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

    /// Dial `KEY@wss://HOST[:PORT]/PATH` ([`Ws::address`]).
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (key, url) = Ws::address(address)?;
            self.connect(&url, &key).await.map(|(io, _)| io)
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
