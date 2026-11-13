//! The poll carriers (feature `poll`): a link over requests of HTTP/1.1
//! with TLS 1.3, each side pinning the Ed25519 key of the other as the tcp
//! carrier does (`pod_tls::peer`). No request lives as long as the link, so
//! a proxy that closes idle connections or a front of HTTP/1.1 alone can
//! carry it. One [`Form`] of the bytes down is the dialer's choice; the
//! bytes up go in POSTs in each form.
//!
//! * The dial goes through the way out of the caller (`pod_core::Dial`) to
//!   `KEY@https://HOST[:PORT]/PATH`, with the ALPN `http/1.1`. KEY names the
//!   listener's key as `pod_identity::KeyName` reads it. PATH is the base of
//!   the lanes (`PATH/open`, `PATH/up`, `PATH/down`, `PATH/out`,
//!   `PATH/push`); it has no query.
//! * The dialer opens a session of a random id with `POST PATH/open?s=ID`
//!   (201). The answer comes after the listener checked the key of the
//!   dialer, so a refused key is an error of the dial. A listener with
//!   [`Poll::max_sessions`] sessions answers 429; the third 429 in a row
//!   ends the dial.
//! * Up: `POST PATH/up?s=ID&seq=N&ack=M` with at most one batch, one in
//!   flight; 202 with `Pod-Ack`, the next offset that the listener waits
//!   for. N is the offset of the first byte of the body, so the POST again
//!   after a lost answer gives no byte twice. M acknowledges the bytes down.
//! * Down, by the form: [`Form::LongPoll`] holds `POST PATH/down?since=N`
//!   up to [`Poll::hold`]; [`Form::Simplex`] asks `GET PATH/out?since=N`,
//!   with 204 when the listener has nothing, and polls again after 100 ms to
//!   5 s; [`Form::Sse`] and [`Form::Chunked`] read one answer of
//!   `GET PATH/push?since=N` as it comes, the events of SSE (base64, 8 KiB
//!   each, a `: ping` comment each 15 s) or the raw chunks. The listener
//!   keeps the bytes down from the last offset acknowledged, so a down
//!   request again after a cut gives each byte. A listener with no
//!   [`Poll::push`] answers 406 to a push, and the dialer takes the long
//!   poll.
//! * A lane that loses its connection connects again with a backoff of
//!   100 ms to 5 s. A lane that fails for [`Poll::idle`], a key that the
//!   pin refused, a session that the listener does not know (404) end the
//!   link with an error. The listener ends a session with no request for
//!   [`Poll::idle`].
//!
//! A key that is not pinned is a [`CarrierErrorKind::Refused`] error (77)
//! that names the key that the peer proved.

mod answer;
mod dial;
mod down;
mod lane;
mod listen;
mod pipes;
mod session;
mod wire;

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::Dial;
use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer::Keys;
use rustls::crypto::CryptoProvider;

pub use crate::post_url::PostUrl;
pub use listen::{PollListener, Stats};

use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Cost, Facts, Listener, Pending};
use crate::{pin, post_url};

/// The ALPN of HTTP/1.1 over TLS.
pub const ALPN: &[u8] = b"http/1.1";

/// The default time limit of a dial through the way out.
pub const CONNECT_TIMEOUT: Duration = pod_core::backoff::CONNECT_TIMEOUT;

/// The default time limit of a handshake on each side: the TLS of each
/// connection, and the open of a session.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The default time limit of an answer that the listener does not hold.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// The default hold of a long poll, and its bounds.
pub const HOLD: Duration = Duration::from_secs(25);
pub const MIN_HOLD: Duration = Duration::from_secs(1);
pub const MAX_HOLD: Duration = Duration::from_secs(60);

/// The default time after which a session with no request ends on the
/// listener, and a lane that fails ends on the dialer.
pub const IDLE: Duration = Duration::from_secs(60);

/// The default count of handshakes that a listener runs at once.
pub const MAX_HANDSHAKES: usize = 64;

/// The default count of sessions that a listener holds.
pub const MAX_SESSIONS: usize = 256;

/// The most bytes of one request or one answer of the long poll, SSE and
/// the chunked stream; the listener takes no larger body.
pub const BATCH: usize = 256 << 10;

/// The most bytes of one request or one answer of the simplex poll.
pub const SIMPLEX_BATCH: usize = 64 << 10;

/// The bytes down that a listener keeps for a session, from the last
/// offset acknowledged.
pub const WINDOW: usize = 1 << 20;

/// The bytes of a push after which the dialer acknowledges them in a POST.
pub const ACK_EVERY: u64 = 64 << 10;

/// The silence after which a push of SSE sends a `: ping` comment.
pub const PING_EVERY: Duration = Duration::from_secs(15);

/// The backoff of a lane: the first wait and the longest.
pub const BACKOFF_FIRST: Duration = Duration::from_millis(100);
pub const BACKOFF_MAX: Duration = Duration::from_secs(5);

/// The answers 429 in a row that end a lane or a dial.
pub const BUSY_LIMIT: u32 = 3;

/// The form of the bytes down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Form {
    /// A POST that the listener holds until it has bytes (`poll`).
    LongPoll,
    /// One answer of events of SSE, base64 (`sse`).
    Sse,
    /// One chunked answer of raw bytes (`chunked`).
    Chunked,
    /// A GET at once, 204 when the listener has nothing, again after a
    /// backoff (`httppoll`).
    Simplex,
}

impl Form {
    /// The name of the carrier of this form.
    pub fn name(self) -> &'static str {
        match self {
            Form::LongPoll => "poll",
            Form::Sse => "sse",
            Form::Chunked => "chunked",
            Form::Simplex => "httppoll",
        }
    }

    /// The most bytes of one request or answer.
    pub fn batch(self) -> usize {
        if self == Form::Simplex { SIMPLEX_BATCH } else { BATCH }
    }

    fn cost(self) -> Cost {
        match self {
            Form::LongPoll | Form::Chunked => Cost::Medium,
            Form::Sse => Cost::High,
            Form::Simplex => Cost::Highest,
        }
    }
}

/// The poll carriers: the TLS of this end (its provider and its identity),
/// the way out of its dials, the client keys of its listeners and the form
/// of its dials. A clone shares the set of client keys. A listener serves
/// each form.
#[derive(Clone)]
pub struct Poll {
    provider: Arc<CryptoProvider>,
    identity: Arc<Identity>,
    way: Arc<dyn Dial>,
    clients: Keys,
    /// The form of the bytes down of a dial.
    pub form: Form,
    /// The time limit of a dial through the way out.
    pub connect_timeout: Duration,
    /// The time limit of the TLS of each connection, and of the open.
    pub handshake_timeout: Duration,
    /// The time limit of an answer that the listener does not hold.
    pub answer_timeout: Duration,
    /// The hold of a long poll that the dialer asks for (1 s to 60 s); a
    /// listener holds at most 60 s.
    pub hold: Duration,
    /// A listener ends a session with no request for this time; a dialer
    /// ends a lane that fails for this time.
    pub idle: Duration,
    /// The most handshakes that a listener runs at once (at least 1).
    pub max_handshakes: usize,
    /// The most sessions that a listener holds (at least 1).
    pub max_sessions: usize,
    /// A listener answers a push (SSE, chunked); else 406.
    pub push: bool,
}

impl fmt::Debug for Poll {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Poll")
            .field("identity", &self.identity.public())
            .field("way", &self.way.label())
            .field("clients", &self.clients.list())
            .field("form", &self.form)
            .field("connect_timeout", &self.connect_timeout)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("answer_timeout", &self.answer_timeout)
            .field("hold", &self.hold)
            .field("idle", &self.idle)
            .field("max_handshakes", &self.max_handshakes)
            .field("max_sessions", &self.max_sessions)
            .field("push", &self.push)
            .finish()
    }
}

impl Poll {
    /// A carrier of the long poll that presents `identity` on TLS of
    /// `provider` and dials through `way`. Its listeners take no client
    /// until [`Poll::with_clients`] gives the keys.
    pub fn new(provider: Arc<CryptoProvider>, identity: Arc<Identity>, way: Arc<dyn Dial>) -> Poll {
        Poll {
            provider,
            identity,
            way,
            clients: Keys::default(),
            form: Form::LongPoll,
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            answer_timeout: ANSWER_TIMEOUT,
            hold: HOLD,
            idle: IDLE,
            max_handshakes: MAX_HANDSHAKES,
            max_sessions: MAX_SESSIONS,
            push: true,
        }
    }

    /// The form of the bytes down of a dial.
    pub fn with_form(mut self, form: Form) -> Poll {
        self.form = form;
        self
    }

    /// The client keys that a listener takes. The set is shared: a change
    /// of it applies at the next handshake of each listener.
    pub fn with_clients(mut self, clients: Keys) -> Poll {
        self.clients = clients;
        self
    }

    pub fn clients(&self) -> &Keys {
        &self.clients
    }

    fn err(&self, kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
        CarrierError::new(self.form.name(), kind, text)
    }

    /// The settings: a hold of 1 s to 60 s, an idle time longer than the
    /// hold, each time limit and count more than zero.
    pub fn check(&self) -> Result<(), CarrierError> {
        let usage = |text: &str| Err(self.err(CarrierErrorKind::Usage, text));
        if !(MIN_HOLD..=MAX_HOLD).contains(&self.hold) {
            return usage("the hold is out of 1 s to 60 s");
        }
        if self.idle <= self.hold {
            return usage("the idle time is not longer than the hold");
        }
        if [self.connect_timeout, self.handshake_timeout, self.answer_timeout].contains(&Duration::ZERO) {
            return usage("a time limit is 0");
        }
        if self.max_handshakes == 0 || self.max_sessions == 0 {
            return usage("max_handshakes or max_sessions is 0: give 1 or more");
        }
        Ok(())
    }

    /// The key of the listener and its URL in a dial address
    /// `KEY@https://HOST[:PORT]/PATH`. PATH has no query.
    pub fn address(&self, address: &str) -> Result<(KeyName, PostUrl), CarrierError> {
        let (key, url) = post_url::address(self.form.name(), address)?;
        if url.path.contains('?') {
            return Err(self.err(CarrierErrorKind::Usage, "the path of a poll carrier has no query"));
        }
        Ok((key, url))
    }

    /// Connect to `url` through the way out, take only the listener whose
    /// key `peer` names, and open a session. It gives the stream and the
    /// key that the listener proved.
    pub async fn connect(&self, url: &PostUrl, peer: &KeyName) -> Result<(BoxIo, PublicKey), CarrierError> {
        self.check()?;
        dial::connect(self, url, peer).await
    }

    /// A listener at `addr` (port 0 takes a free port).
    pub async fn bind(&self, addr: SocketAddr) -> Result<PollListener, CarrierError> {
        self.check()?;
        listen::bind(self, addr).await
    }
}

impl Carrier for Poll {
    fn name(&self) -> &'static str {
        self.form.name()
    }

    fn facts(&self) -> Facts {
        Facts { cost: self.form.cost(), ..Facts::STREAM }
    }

    /// The provider makes TLS 1.3 and verifies Ed25519, and a TCP socket
    /// binds on the loopback: the host allows the family.
    fn available(&self) -> Result<(), CarrierError> {
        pin::available(self.form.name(), &self.provider)
    }

    /// Dial `KEY@https://HOST[:PORT]/PATH` ([`Poll::address`]).
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (key, url) = self.address(address)?;
            self.connect(&url, &key).await.map(|(io, _)| io)
        })
    }

    /// Listen at `IP:PORT`.
    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            let addr = address.parse::<SocketAddr>().map_err(|_| {
                self.err(CarrierErrorKind::Usage, format!("{address:?} is no IP:PORT (127.0.0.1:7000, [::]:7000)"))
            })?;
            Ok(Box::new(self.bind(addr).await?) as Box<dyn Listener>)
        })
    }
}
