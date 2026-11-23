//! The trait of a carrier: a way to get a byte stream to a peer, and its
//! own check of the host (call C117). The registry of the carriers, their
//! order and their flags are the consumer's.

use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    /// The rate of the carrier: the payload that it moves for each second,
    /// the floor of its latency, and its cost class.
    pub rate: Rate,
}

impl Facts {
    /// A reliable, ordered, duplex byte stream with no unit, a low cost and
    /// no declared rate.
    pub const STREAM: Facts =
        Facts { ordered: true, reliable: true, duplex: true, mtu: None, cost: Cost::Low, rate: Rate::STREAM };

    /// The facts of a stream with `cost` as its cost class and in its rate.
    pub const fn with_cost(cost: Cost) -> Facts {
        Facts { cost, rate: Rate { bytes_per_second: None, poll_interval: None, cost }, ..Facts::STREAM }
    }

    /// The facts of this stream with `rate`, which also gives the cost
    /// class: one value names the price of the carrier.
    pub const fn with_rate(rate: Rate) -> Facts {
        Facts { cost: rate.cost, rate, ..Facts::STREAM }
    }
}

/// The rate of a carrier, as the operator reads it before choosing the
/// carrier (PodUtils/podnet#484): its payload rate, the floor of its
/// latency, and its cost class.
///
/// * `bytes_per_second` is the payload that the carrier moves in one
///   direction for each second, when it is fixed. `None` says that the
///   carrier has no rate of its own (a byte stream at the speed of the
///   host), or that the service sets it and has not said a value yet.
/// * `poll_interval` is the time that the carrier waits between two polls
///   of its service, which is the floor of its latency for the bytes that
///   wait. `None` says that the carrier pushes its bytes, or that it has
///   not measured its poll yet.
/// * `cost` is the same value as [`Facts::cost`], so that this one value
///   carries the whole price of the carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rate {
    pub bytes_per_second: Option<u64>,
    pub poll_interval: Option<Duration>,
    pub cost: Cost,
}

impl Rate {
    /// No declared rate, and the cost class `Low`.
    pub const STREAM: Rate = Rate { bytes_per_second: None, poll_interval: None, cost: Cost::Low };

    /// This rate with `cost` as its cost class.
    pub const fn priced(cost: Cost) -> Rate {
        Rate { cost, ..Rate::STREAM }
    }
}

/// The kind of a carrier limit that the service of a carrier set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitKind {
    /// The service cut a unit of the payload below the size that the
    /// carrier sent, for example an NTP reply that the server shortened.
    Truncated,
    /// The service refused a unit of the payload, for example an SMTP
    /// message over the size of the server.
    Refused,
    /// The service declared the largest unit that it takes, before the
    /// carrier sent one.
    Declared,
}

/// A limit of the service of a carrier. It is not a protocol error: the
/// session stays open, and the carrier keeps its bytes within the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Limit {
    pub kind: LimitKind,
    /// The bytes of the largest unit that the service takes, when it said
    /// one.
    pub bytes: Option<u64>,
}

impl Limit {
    /// The service cut a unit below `bytes`.
    pub const fn truncated(bytes: u64) -> Limit {
        Limit { kind: LimitKind::Truncated, bytes: Some(bytes) }
    }

    /// The service refused a unit; it names a size when it said one.
    pub const fn refused(bytes: Option<u64>) -> Limit {
        Limit { kind: LimitKind::Refused, bytes }
    }

    /// The service declared a unit of at most `bytes`.
    pub const fn declared(bytes: u64) -> Limit {
        Limit { kind: LimitKind::Declared, bytes: Some(bytes) }
    }
}

/// The limit of a carrier that its sessions report and a caller reads: the
/// last limit that a session of this carrier met, and no error.
#[derive(Debug, Clone, Default)]
pub struct Limits {
    inner: Arc<Mutex<Option<Limit>>>,
}

impl Limits {
    pub fn new() -> Limits {
        Limits::default()
    }

    /// Report `limit`. A later limit replaces an earlier one.
    pub fn set(&self, limit: Limit) {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = Some(limit);
    }

    /// The last limit that a session of this carrier met.
    pub fn get(&self) -> Option<Limit> {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One value names the price: the class of [`Facts`] is the class of
    /// its rate, whatever sets it.
    #[test]
    fn the_cost_class_of_the_facts_and_of_its_rate_agree() {
        assert_eq!(Facts::STREAM.cost, Facts::STREAM.rate.cost);
        for cost in [Cost::Low, Cost::Medium, Cost::High, Cost::Highest] {
            assert_eq!(Facts::with_cost(cost).cost, Facts::with_cost(cost).rate.cost);
        }
        let rate =
            Rate { bytes_per_second: Some(9), poll_interval: Some(Duration::from_secs(32)), cost: Cost::Highest };
        let facts = Facts::with_rate(rate);
        assert_eq!((facts.cost, facts.rate), (Cost::Highest, rate));
        assert_eq!(Rate::priced(Cost::Medium).cost, Cost::Medium);
    }

    /// A limit is a value that the caller reads, not an error.
    #[test]
    fn the_limits_of_a_carrier_keep_the_last_one() {
        let limits = Limits::new();
        assert_eq!(limits.get(), None);
        limits.set(Limit::declared(1024));
        assert_eq!(limits.get(), Some(Limit::declared(1024)));
        limits.set(Limit::refused(Some(512)));
        assert_eq!(limits.get(), Some(Limit { kind: LimitKind::Refused, bytes: Some(512) }));
        assert_eq!(Limit::truncated(300).bytes, Some(300));
    }
}
