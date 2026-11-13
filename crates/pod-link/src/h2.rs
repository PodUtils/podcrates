//! The h2 carrier (feature `h2`): one POST of HTTP/2 over TLS 1.3, each
//! side pinning the Ed25519 key of the other as the tcp carrier does
//! (`pod_tls::peer`). The request body carries the bytes of the dialer and
//! the response body the bytes of the listener; both stay open for the life
//! of the link, and END_STREAM is the half close.
//!
//! * The dial goes through the way out of the caller (`pod_core::Dial`) to
//!   `KEY@https://HOST[:PORT]/PATH`, with the ALPN `h2`. KEY names the
//!   listener's key as `pod_identity::KeyName` reads it.
//! * A listener binds `IP:PORT` and takes a client only when its key is in
//!   [`H2::clients`]. It runs each handshake (TLS, the prefaces, the POST) in
//!   a task of its own, at most [`H2::max_handshakes`] at once, each within
//!   [`H2::handshake_timeout`], and answers 200 with no end of its body.
//! * The SETTINGS of the listener come after it checked the key of the
//!   dialer, so the dial reads them before it gives the stream: a refused
//!   key is an error of the dial, as the byte of acceptance of `tcp` is.
//!
//! A key that is not pinned is a [`CarrierErrorKind::Refused`] error (77)
//! that names the key that the peer proved.

mod listen;
mod primed;

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use pod_core::Dial;
use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer::{self, Keys};
use rustls::crypto::CryptoProvider;
use tokio::time::timeout;

pub use crate::post_url::PostUrl;
pub use listen::H2Listener;

use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Listener, Pending};
use crate::{pin, post_url};

/// The name of the carrier.
pub const NAME: &str = "h2";

/// The ALPN of HTTP/2 over TLS.
pub const ALPN: &[u8] = b"h2";

/// The default time limit of a dial through the way out.
pub const CONNECT_TIMEOUT: Duration = pod_core::backoff::CONNECT_TIMEOUT;

/// The default time limit of a handshake on each side: TLS, the prefaces
/// of HTTP/2, the POST and its answer.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The default count of handshakes that a listener runs at once.
pub const MAX_HANDSHAKES: usize = 64;

/// The window of the stream and of the connection on each side: the bytes
/// that one end sends before the reader of the other takes them.
pub const WINDOW: u32 = 4 << 20;

/// The tcp carrier's sibling over one POST of HTTP/2: the TLS of this end
/// (its provider and its identity), the way out of its dials, and the
/// client keys of its listeners. A clone shares the set of client keys.
#[derive(Clone)]
pub struct H2 {
    provider: Arc<CryptoProvider>,
    identity: Arc<Identity>,
    way: Arc<dyn Dial>,
    clients: Keys,
    /// The time limit of a dial through the way out.
    pub connect_timeout: Duration,
    /// The time limit of a handshake, on each side.
    pub handshake_timeout: Duration,
    /// The most handshakes that a listener runs at once (at least 1).
    pub max_handshakes: usize,
}

impl fmt::Debug for H2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("H2")
            .field("identity", &self.identity.public())
            .field("way", &self.way.label())
            .field("clients", &self.clients.list())
            .field("connect_timeout", &self.connect_timeout)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("max_handshakes", &self.max_handshakes)
            .finish()
    }
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

impl H2 {
    /// A carrier that presents `identity` on TLS of `provider` and dials
    /// through `way`. Its listeners take no client until
    /// [`H2::with_clients`] gives the keys.
    pub fn new(provider: Arc<CryptoProvider>, identity: Arc<Identity>, way: Arc<dyn Dial>) -> H2 {
        H2 {
            provider,
            identity,
            way,
            clients: Keys::default(),
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            max_handshakes: MAX_HANDSHAKES,
        }
    }

    /// The client keys that a listener takes. The set is shared: a change
    /// of it applies at the next handshake of each listener.
    pub fn with_clients(mut self, clients: Keys) -> H2 {
        self.clients = clients;
        self
    }

    pub fn clients(&self) -> &Keys {
        &self.clients
    }

    /// The key of the listener and its URL in a dial address
    /// `KEY@https://HOST[:PORT]/PATH`.
    pub fn address(address: &str) -> Result<(KeyName, PostUrl), CarrierError> {
        post_url::address(NAME, address)
    }

    /// Connect to `url` through the way out and take only the listener
    /// whose key `peer` names. It gives the stream and the key that the
    /// listener proved.
    pub async fn connect(&self, url: &PostUrl, peer: &KeyName) -> Result<(BoxIo, PublicKey), CarrierError> {
        let config = peer::client_config(self.provider.clone(), self.identity.clone(), peer.clone(), &[ALPN])
            .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
        let target = url.target();
        let name = pin::server_name(NAME, &target)?;
        let connected = pin::dial(NAME, &*self.way, &target, self.connect_timeout).await?;
        let own = self.identity.public();
        let shake = async {
            let tls = tokio_rustls::TlsConnector::from(config)
                .connect(name, connected.io)
                .await
                .map_err(|e| pin::tls_error(NAME, format!("the TLS handshake with {target}"), e))?;
            let conn = tls.get_ref().1;
            pin::check_alpn(NAME, conn.alpn_protocol(), ALPN, &target)?;
            let key = pin::proved(NAME, conn.peer_certificates(), &target)?;
            // The first bytes of the listener (its SETTINGS) come after it
            // took the key of this end; an alert comes in their place.
            let tls = primed::Primed::read(tls).await.map_err(|e| pin::answer_error(NAME, &target, &own, e))?;
            let (mut ask, connection) = ::h2::client::Builder::new()
                .initial_window_size(WINDOW)
                .initial_connection_window_size(WINDOW)
                .handshake::<_, Bytes>(tls)
                .await
                .map_err(|e| h2_error(format!("the prefaces of HTTP/2 with {target}"), e))?;
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    tracing::debug!(error = %e, "the connection of an h2 carrier ended");
                }
            });
            let request = http::Request::post(format!("https://{}{}", url.authority(), url.path))
                .header("content-type", "application/octet-stream")
                .body(())
                .map_err(|e| err(CarrierErrorKind::Usage, format!("the request: {e}")))?;
            let (answer, send) =
                ask.send_request(request, false).map_err(|e| h2_error(format!("the POST to {target}"), e))?;
            let answer = answer.await.map_err(|e| h2_error(format!("the answer of {target}"), e))?;
            if answer.status() != http::StatusCode::OK {
                return Err(err(
                    CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                    format!("the listener at {target} answered the POST with {}", answer.status()),
                ));
            }
            Ok((Box::new(pod_ws::h2_road::duplex(send, answer.into_body())) as BoxIo, key))
        };
        timeout(self.handshake_timeout, shake).await.unwrap_or_else(|_| {
            Err(err(
                CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                format!("the handshake with {target} took more than {:?}", self.handshake_timeout),
            ))
        })
    }

    /// A listener at `addr` (port 0 takes a free port).
    pub async fn bind(&self, addr: SocketAddr) -> Result<H2Listener, CarrierError> {
        listen::bind(self, addr).await
    }
}

/// An error of HTTP/2, with the kind of its I/O where it has one.
fn h2_error(text: String, e: ::h2::Error) -> CarrierError {
    let kind = e.get_io().map_or(io::ErrorKind::InvalidData, io::Error::kind);
    CarrierError::with_source(NAME, CarrierErrorKind::Io(kind), text, io::Error::new(kind, e))
}

impl Carrier for H2 {
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

    /// Dial `KEY@https://HOST[:PORT]/PATH` ([`H2::address`]).
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (key, url) = H2::address(address)?;
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
