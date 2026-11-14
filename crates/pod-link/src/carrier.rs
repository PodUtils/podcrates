//! The trait of a carrier: a way to get a byte stream to a peer, and its
//! own check of the host (call C117). The registry of the carriers, their
//! order and their flags are the consumer's.

use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;

use pod_core::{Class, Failure, Stage};
use pod_identity::PublicKey;
use tokio::io::{AsyncRead, AsyncWrite};

/// A byte stream that a carrier gives: the link runs over it.
pub trait Io: AsyncRead + AsyncWrite + Send + Unpin + 'static {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Io for T {}

/// A carrier's byte stream, boxed.
pub type BoxIo = Box<dyn Io>;

/// The future of a carrier's operation.
pub type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T, CarrierError>> + Send + 'a>>;

/// What a carrier gives, for the consumer's choice and for the layers
/// above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    /// The bytes come in the order that they went.
    pub ordered: bool,
    /// No byte is lost: a carrier that loses bytes needs the ARQ above it.
    pub reliable: bool,
    /// Both directions move at the same time.
    pub duplex: bool,
    /// The largest unit of the carrier, when it has one (a datagram).
    pub mtu: Option<usize>,
    /// The bytes that the carrier spends on the wire for each byte of the
    /// link, for the consumer's order of the carriers.
    pub cost: Cost,
}

impl Facts {
    /// A reliable, ordered, duplex byte stream with no unit and a low cost.
    pub const STREAM: Facts = Facts { ordered: true, reliable: true, duplex: true, mtu: None, cost: Cost::Low };
}

/// The overhead class of a carrier, from the least to the most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cost {
    /// A byte stream with a small frame or none: unix, memory, tcp, ws, h2.
    Low,
    /// A request of HTTP for each batch: the long poll, a chunked stream.
    Medium,
    /// A larger body for each byte: the base64 of the events of SSE.
    High,
    /// A request for each small batch and a poll while idle: the simplex
    /// poll of HTTP.
    Highest,
}

/// A way to reach a peer. The address is the carrier's own text (a path
/// for the unix carrier, a name for the memory carrier, `KEY@HOST:PORT` to
/// dial and `IP:PORT` to listen for the tcp carrier).
pub trait Carrier: Send + Sync {
    /// The name of the carrier: one word, for the consumer's lists.
    fn name(&self) -> &'static str;

    /// What the carrier gives.
    fn facts(&self) -> Facts;

    /// Whether this host can use the carrier now. The error names why not:
    /// the host refused the socket family, the build has no support.
    fn available(&self) -> Result<(), CarrierError>;

    /// Connect to the peer at `address`.
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo>;

    /// Listen at `address` for peers.
    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>>;
}

/// A byte stream that a listener accepted, and the Ed25519 key that its
/// peer proved, when the carrier proves one (the tcp carrier).
pub struct Accepted {
    pub io: BoxIo,
    pub peer: Option<PublicKey>,
}

impl fmt::Debug for Accepted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Accepted").field("peer", &self.peer).finish_non_exhaustive()
    }
}

/// The accept side of a carrier.
pub trait Listener: Send {
    /// The address where peers dial, in the carrier's form.
    fn address(&self) -> String;

    /// The next peer. An error of one peer (it was refused) leaves the
    /// listener open: the caller can accept again.
    fn accept(&mut self) -> Pending<'_, BoxIo>;

    /// The next peer and the key that it proved. A carrier that proves no
    /// key gives `None`, as the unix and the memory carriers do.
    fn accept_peer(&mut self) -> Pending<'_, Accepted> {
        Box::pin(async move { Ok(Accepted { io: self.accept().await?, peer: None }) })
    }
}

/// The kind of a carrier's failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierErrorKind {
    /// The address cannot be used: its form, its length, a file that is not
    /// a socket at its path.
    Usage,
    /// Another listener holds the address.
    InUse,
    /// The host or the build cannot use the carrier.
    Unavailable,
    /// No listener is at the address.
    NotFound,
    /// The peer is not allowed: another user, a key that the pin of this
    /// end does not name, for example.
    Refused,
    /// The peer refused the key of this end: its pin does not name it. The
    /// same on each carrier that pins keys, on the dial and on the accept
    /// (call C293).
    KeyRefused,
    /// Another failure of I/O.
    Io(io::ErrorKind),
}

/// A failure of a carrier: its kind and its text.
#[derive(Debug)]
pub struct CarrierError {
    pub carrier: &'static str,
    pub kind: CarrierErrorKind,
    text: String,
    source: Option<io::Error>,
}

impl CarrierError {
    pub fn new(carrier: &'static str, kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
        CarrierError { carrier, kind, text: text.into(), source: None }
    }

    /// An error of `kind` caused by `e`: a failed handshake, a refused key.
    pub fn with_source(
        carrier: &'static str,
        kind: CarrierErrorKind,
        text: impl Into<String>,
        e: io::Error,
    ) -> CarrierError {
        CarrierError { carrier, kind, text: text.into(), source: Some(e) }
    }

    /// An I/O error, with the kind of its errno where a kind fits.
    pub fn io(carrier: &'static str, text: impl Into<String>, e: io::Error) -> CarrierError {
        let kind = match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => CarrierErrorKind::NotFound,
            io::ErrorKind::AddrInUse => CarrierErrorKind::InUse,
            io::ErrorKind::PermissionDenied => CarrierErrorKind::Refused,
            io::ErrorKind::InvalidInput => CarrierErrorKind::Usage,
            other => CarrierErrorKind::Io(other),
        };
        CarrierError { carrier, kind, text: text.into(), source: Some(e) }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The class for the caller's exit code: a bad address 64, a held
    /// address or an unusable carrier 69, no listener 66, a refused peer 77.
    pub fn class(&self) -> Class {
        match self.kind {
            CarrierErrorKind::Usage => Class::Usage,
            CarrierErrorKind::InUse | CarrierErrorKind::Unavailable => Class::Unavailable,
            CarrierErrorKind::NotFound => Class::NotFound,
            CarrierErrorKind::Refused | CarrierErrorKind::KeyRefused => Class::Refused,
            CarrierErrorKind::Io(kind) => Class::of_io(kind),
        }
    }
}

impl fmt::Display for CarrierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the {} carrier: {}", self.carrier, self.text)?;
        if let Some(e) = &self.source {
            write!(f, ": {e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CarrierError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e as _)
    }
}

impl From<CarrierError> for Failure {
    fn from(e: CarrierError) -> Failure {
        Failure::caused(Stage::Connect, e.class(), e.to_string(), e)
    }
}
