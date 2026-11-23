//! The object-storage carrier (feature `s3`, PodUtils/podnet#468 row E2-4,
//! PodUtils/podnet@T-279): each end writes its bytes as objects of a bucket
//! of the S3 API that both ends reach, and reads the objects of the other
//! end. The TLS 1.3 of the tcp carrier runs over the objects: each side
//! pins the Ed25519 key of the other (`pod_tls::peer`), and TLS seals each
//! byte, so a reader of the bucket learns nothing and a writer of the
//! bucket can end a link but not change it.
//!
//! * The keys under the prefix of a link (call C489): `open/SESSION` (an
//!   empty object: the dialer opens a session), `up/SESSION/SEQ` (the bytes
//!   of the dialer), `down/SESSION/SEQ` (the bytes of the listener). SESSION
//!   is 128 random bits of the dialer in hex, SEQ 16 hex digits from 0, so a
//!   listing gives the objects in order. An empty object is the end of the
//!   bytes of its direction.
//! * Each PUT has `If-None-Match: *`: a PUT again after a lost answer finds
//!   its object (412), and two writers cannot share a key. Each GET has
//!   `If-Match` with the ETag of the listing.
//! * An object holds at most [`S3::object`] bytes (256 KiB), at most
//!   [`S3::in_flight`] (8) PUTs and GETs are in flight on each side, and a
//!   reader that finds no new object polls again after 100 ms doubled up
//!   to 5 s; a PUT of this end ends the wait. A page of a listing has at
//!   most 1000 keys. The objects of one end of a session hold at most
//!   [`S3::budget`] (1 GiB); a warning of `tracing` comes at 80 %.
//! * The reader removes each object it read only when [`S3::prune`] is on,
//!   as the bucket can be shared; a listener with `prune` also removes the
//!   open object of each session it takes.
//! * A listener takes the sessions that open after its bind: the open
//!   objects in the bucket at the bind are old (call C490).
//! * The endpoint, the region, the key of the bucket and the TLS of an
//!   endpoint of `https` are the caller's ([`Endpoint`], [`Credentials`],
//!   [`S3::with_tls`]); the crate reads no environment and knows no host of
//!   a provider. The requests go through the caller's way out.
//! * The dial address is `KEY@BUCKET[/PREFIX]`, the listen address
//!   `BUCKET[/PREFIX]`.
//! * The feature `s3-standin` adds [`standin`]: a stand-in of the S3 API on
//!   the loopback for the tests of this crate and of a consumer.

mod client;
mod listen;
pub mod sign;
#[cfg(feature = "s3-standin")]
pub mod standin;
mod stream;
mod xml;

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_core::{Dial, Target};
use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer::Keys;
use rustls::ClientConfig;
use rustls::crypto::CryptoProvider;
use tokio::time::timeout;

pub use client::{Endpoint, Style};
pub use listen::S3Listener;
pub use sign::Credentials;

use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Cost, Facts, Listener, Pending};
use crate::{dgram_tls, pin};
use client::Client;
use stream::Dirs;

/// The name of the carrier.
pub const NAME: &str = "s3";

/// The ALPN by default: the one of the tcp carrier.
pub const ALPN: &[u8] = crate::tcp::ALPN;

/// The most bytes of one object by default (podnet's), and the bounds.
pub const OBJECT: usize = 256 << 10;
pub const MIN_OBJECT: usize = 1 << 10;
pub const MAX_OBJECT: usize = 4 << 20;

/// The PUTs and the GETs in flight on each side.
pub const IN_FLIGHT: usize = 8;

/// The wait of a reader that found no new object: the first and the
/// longest.
pub const POLL_MIN: Duration = Duration::from_millis(100);
pub const POLL_MAX: Duration = Duration::from_secs(5);

/// The bytes of the objects of one end of a session.
pub const BUDGET: u64 = 1 << 30;

/// The keys of one page of a listing (the most of the S3 API).
pub const PAGE: usize = 1000;

/// The time limits by default: a connection, an answer, the handshake of a
/// session, and the failures of one call in a row.
pub const CONNECT_TIMEOUT: Duration = pod_core::backoff::CONNECT_TIMEOUT;
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
pub const IDLE: Duration = Duration::from_secs(60);

/// The handshakes that a listener runs at once by default.
pub const MAX_HANDSHAKES: usize = 64;

/// What the two tasks of a session read.
#[derive(Debug, Clone)]
pub(crate) struct Tuning {
    pub object: usize,
    pub in_flight: usize,
    pub poll_min: Duration,
    pub poll_max: Duration,
    pub budget: u64,
    pub page: usize,
    pub prune: bool,
    pub idle: Duration,
}

/// The object-storage carrier: the TLS of this end, the bucket's endpoint
/// and key, the way out, and the client keys of its listeners. A clone
/// shares the set of client keys.
#[derive(Clone)]
pub struct S3 {
    provider: Arc<CryptoProvider>,
    identity: Arc<Identity>,
    way: Arc<dyn Dial>,
    clients: Keys,
    /// The endpoint of the S3 API.
    pub endpoint: Endpoint,
    /// The region of the signature (`us-east-1` by default).
    pub region: String,
    pub credentials: Credentials,
    /// Where the bucket goes in a request.
    pub style: Style,
    /// The TLS of an endpoint of `https`: the caller's roots and provider.
    pub tls: Option<Arc<ClientConfig>>,
    /// The ALPN of both ends.
    pub alpn: &'static [u8],
    /// The most bytes of one object (1 KiB to 4 MiB).
    pub object: usize,
    /// The PUTs and the GETs in flight on each side (1 to 64).
    pub in_flight: usize,
    /// The wait of a reader that found no new object, doubled from
    /// `poll_min` up to `poll_max`.
    pub poll_min: Duration,
    pub poll_max: Duration,
    /// The bytes of the objects of one end of a session (at least one
    /// object).
    pub budget: u64,
    /// The keys of one page of a listing (1 to 1000).
    pub page: usize,
    /// The reader removes each object it read, and a listener each open
    /// object it took.
    pub prune: bool,
    pub connect_timeout: Duration,
    pub answer_timeout: Duration,
    pub handshake_timeout: Duration,
    /// A call that fails for this time ends its session.
    pub idle: Duration,
    /// The handshakes that a listener runs at once (at least 1).
    pub max_handshakes: usize,
}

impl fmt::Debug for S3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3")
            .field("identity", &self.identity.public())
            .field("way", &self.way.label())
            .field("clients", &self.clients.list())
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("credentials", &self.credentials)
            .field("style", &self.style)
            .field("tls", &self.tls.is_some())
            .field("object", &self.object)
            .field("in_flight", &self.in_flight)
            .field("budget", &self.budget)
            .field("prune", &self.prune)
            .finish_non_exhaustive()
    }
}

fn usage(text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, CarrierErrorKind::Usage, text)
}

impl S3 {
    /// A carrier that presents `identity` on TLS of `provider`, and reaches
    /// the endpoint with the key `credentials` through `way`. Its listeners
    /// take no client until [`S3::with_clients`] gives the keys.
    pub fn new(
        provider: Arc<CryptoProvider>,
        identity: Arc<Identity>,
        way: Arc<dyn Dial>,
        endpoint: Endpoint,
        credentials: Credentials,
    ) -> S3 {
        S3 {
            provider,
            identity,
            way,
            clients: Keys::default(),
            endpoint,
            region: "us-east-1".into(),
            credentials,
            style: Style::Path,
            tls: None,
            alpn: ALPN,
            object: OBJECT,
            in_flight: IN_FLIGHT,
            poll_min: POLL_MIN,
            poll_max: POLL_MAX,
            budget: BUDGET,
            page: PAGE,
            prune: false,
            connect_timeout: CONNECT_TIMEOUT,
            answer_timeout: ANSWER_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            idle: IDLE,
            max_handshakes: MAX_HANDSHAKES,
        }
    }

    /// The client keys that a listener takes. The set is shared.
    pub fn with_clients(mut self, clients: Keys) -> S3 {
        self.clients = clients;
        self
    }

    pub fn with_region(mut self, region: impl Into<String>) -> S3 {
        self.region = region.into();
        self
    }

    /// The TLS of an endpoint of `https` (for example the configuration of
    /// a `pod_tls::Context` over its roots).
    pub fn with_tls(mut self, tls: Arc<ClientConfig>) -> S3 {
        self.tls = Some(tls);
        self
    }

    pub fn clients(&self) -> &Keys {
        &self.clients
    }

    /// The settings within their bounds.
    pub fn check(&self) -> Result<(), CarrierError> {
        if !(MIN_OBJECT..=MAX_OBJECT).contains(&self.object) {
            return Err(usage("the object size is out of 1 KiB to 4 MiB"));
        }
        if !(1..=64).contains(&self.in_flight) || !(1..=PAGE).contains(&self.page) || self.max_handshakes == 0 {
            return Err(usage("in_flight is out of 1 to 64, page out of 1 to 1000, or max_handshakes is 0"));
        }
        if self.poll_min.is_zero() || self.poll_max < self.poll_min || self.budget < self.object as u64 {
            return Err(usage("poll_min is 0, poll_max is less than poll_min, or the budget is less than one object"));
        }
        let limits = [self.connect_timeout, self.answer_timeout, self.handshake_timeout, self.idle];
        if limits.contains(&Duration::ZERO) || self.region.is_empty() {
            return Err(usage("a time limit is 0, or the region is empty"));
        }
        if self.endpoint.secure && self.tls.is_none() {
            let text = "an endpoint of https needs a TLS configuration (S3::with_tls)";
            return Err(CarrierError::new(NAME, CarrierErrorKind::Unavailable, text));
        }
        Ok(())
    }

    /// The bucket and the prefix of `BUCKET[/PREFIX]`. A bucket has 3 to 63
    /// of `a-z 0-9 . -`; a prefix has `A-Z a-z 0-9 . _ - /`, no `/` at an
    /// end and no `//`, at most 256 bytes.
    pub fn place(text: &str) -> Result<(String, String), CarrierError> {
        let (bucket, prefix) = text.split_once('/').unwrap_or((text, ""));
        let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        let good = (3..=63).contains(&bucket.len())
            && bucket.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
            && edge(bucket.chars().next())
            && edge(bucket.chars().last());
        if !good {
            return Err(usage(format!(
                "{bucket:?} is no bucket: 3 to 63 of a-z 0-9 . -, a letter or a digit at each end"
            )));
        }
        let safe = prefix.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'));
        if prefix.len() > 256 || !safe || prefix.starts_with('/') || prefix.ends_with('/') || prefix.contains("//") {
            return Err(usage(format!("{prefix:?} is no prefix: A-Z a-z 0-9 . _ - and single / inside")));
        }
        Ok((bucket.to_string(), prefix.to_string()))
    }

    /// The key of the listener, the bucket and the prefix of a dial address
    /// `KEY@BUCKET[/PREFIX]`.
    pub fn address(text: &str) -> Result<(KeyName, String, String), CarrierError> {
        let (key, rest) = pin::key_at(NAME, text, "KEY@BUCKET[/PREFIX]")?;
        let (bucket, prefix) = S3::place(rest)?;
        Ok((key, bucket, prefix))
    }

    pub(crate) fn tuning(&self) -> stream::TuningRef {
        Arc::new(Tuning {
            object: self.object,
            in_flight: self.in_flight,
            poll_min: self.poll_min,
            poll_max: self.poll_max,
            budget: self.budget,
            page: self.page,
            prune: self.prune,
            idle: self.idle,
        })
    }

    fn client(&self, bucket: &str) -> Arc<Client> {
        Arc::new(Client {
            name: NAME,
            endpoint: self.endpoint.clone(),
            bucket: bucket.to_string(),
            style: self.style,
            region: self.region.clone(),
            credentials: self.credentials.clone(),
            tls: self.tls.clone(),
            way: self.way.clone(),
            connect_timeout: self.connect_timeout,
            answer_timeout: self.answer_timeout,
            pool: Mutex::new(Vec::new()),
        })
    }

    /// Open a session under `prefix` of `bucket`, and run TLS over its
    /// objects with the pin of `peer`. It gives the stream and the key that
    /// the listener proved.
    pub async fn connect(
        &self,
        bucket: &str,
        prefix: &str,
        peer: &KeyName,
    ) -> Result<(BoxIo, PublicKey), CarrierError> {
        self.check()?;
        let mut id = [0u8; 16];
        self.provider.secure_random.fill(&mut id).map_err(|_| {
            CarrierError::new(NAME, CarrierErrorKind::Unavailable, "the provider gave no random bytes for the session")
        })?;
        let keys = stream::Keys::of(prefix, &sign::hex(&id));
        let (client, tuning) = (self.client(bucket), self.tuning());
        let start = tokio::time::Instant::now();
        let late = || {
            let text = format!("the open of a session in {bucket} took more than {:?}", self.handshake_timeout);
            CarrierError::new(NAME, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), text)
        };
        timeout(self.handshake_timeout, stream::again(&tuning, || client.put(&keys.open, &[])))
            .await
            .map_err(|_| late())??;
        let io = stream::start(client, Dirs { out: keys.up, into: keys.down }, tuning);
        let target = Target::Name { host: bucket.to_string(), port: self.endpoint.port };
        let left = self.handshake_timeout.saturating_sub(start.elapsed());
        let (provider, identity) = (&self.provider, &self.identity);
        dgram_tls::connect_stream(NAME, io, provider, identity, peer, self.alpn, &target, left).await
    }

    /// A listener of the sessions under `prefix` of `bucket` that open
    /// after this call.
    pub async fn bind(&self, bucket: &str, prefix: &str) -> Result<S3Listener, CarrierError> {
        self.check()?;
        let acceptor = dgram_tls::acceptor(NAME, &self.provider, &self.identity, &self.clients, self.alpn)?;
        listen::bind(self, self.client(bucket), prefix, acceptor).await
    }
}

impl Carrier for S3 {
    fn name(&self) -> &'static str {
        NAME
    }

    fn facts(&self) -> Facts {
        Facts::with_cost(Cost::Highest)
    }

    /// The provider makes TLS 1.3 and verifies Ed25519, the settings are in
    /// their bounds, and an endpoint of `https` has its TLS.
    fn available(&self) -> Result<(), CarrierError> {
        pin::available(NAME, &self.provider)?;
        self.check()
    }

    /// Dial `KEY@BUCKET[/PREFIX]` ([`S3::address`]).
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (key, bucket, prefix) = S3::address(address)?;
            self.connect(&bucket, &prefix, &key).await.map(|(io, _)| io)
        })
    }

    /// Listen at `BUCKET[/PREFIX]` ([`S3::place`]).
    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            let (bucket, prefix) = S3::place(address)?;
            Ok(Box::new(self.bind(&bucket, &prefix).await?) as Box<dyn Listener>)
        })
    }
}
