//! The capture of the HTTP exchanges of a tunnel for the consumer (an
//! inspector of the requests): the heads, the payload of the bodies and the
//! end of each exchange of an HTTP origin.
//!
//! The consumer gives a [`Capture`] in [`crate::Host::capture`]. The crate
//! calls it after each write to the peer, so a slow sink holds no byte
//! back. Each method is a non-blocking call: it must return at once (for
//! example, a push into a ring under a short lock, or a `try_send` into a
//! bounded channel that refuses when it is full). The crate has no queue of
//! its own. A method that returns [`Refused`] drops the sink for that
//! exchange: the crate calls no other method for it, also not
//! [`Capture::end`], and the copy goes on with the same bytes. A failure
//! of the crate is not a refusal: the sink gets [`Capture::fault`] and the
//! end of the exchange.
//!
//! What the sink gets:
//!
//! - The ingress rule that matched and the hostname of the visitor
//!   ([`Route`], by [`Capture::route`]), before the request head. A
//!   method that the consumer does not write does nothing.
//! - The request head that went to the origin ([`Head`]), after the
//!   credential in front of the tunnel is removed. A request that this
//!   credential refuses (401) reaches no origin and no sink.
//! - The request body as the origin got it; a chunked request body as its
//!   decoded payload, with no size lines and no trailer.
//! - The answer head of the origin as the edge got it, and its body as its
//!   payload: a chunked answer gives the decoded payload, never the size
//!   lines.
//! - For an upgrade (a WebSocket), the two heads (the answer with its
//!   status 101) and then no body in either direction.
//! - A failure of the crate in the capture of a body ([`Fault`], by
//!   [`Capture::fault`]). The sink stays live: it gets the other parts and
//!   the end of the exchange.
//! - The end, with the status and the time ([`End`]).
//!
//! The heads keep the headers of the visitor as they are: an
//! `Authorization` for the origin, a `Cookie`, a `Set-Cookie` of the origin.
//! The consumer redacts them before it shows or keeps them. The crate adds
//! no credential to the head of the origin. A raw TCP stream, a forward rule
//! and a service of the consumer are not HTTP exchanges of an origin: the
//! sink gets none of them.

use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

mod dechunk;

pub(crate) use dechunk::Dechunk;

/// The direction of a head or a body part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// From the visitor to the origin.
    Request,
    /// From the origin to the visitor.
    Response,
}

/// The first line of a head.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// A request: the method and the target (the path and the query).
    Request { method: String, target: String },
    /// An answer of the origin: its status.
    Response { status: u16 },
}

/// One head of an exchange: its first line and its headers in their order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    pub line: Line,
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// The direction of the head.
    pub fn direction(&self) -> Direction {
        match self.line {
            Line::Request { .. } => Direction::Request,
            Line::Response { .. } => Direction::Response,
        }
    }

    /// The first value of the header `name` (in any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    /// The head of a request that the crate built for the origin
    /// (`build_request_head`): one line for each header, each one valid.
    pub(crate) fn of_request(text: &str) -> Head {
        let mut lines = text.split("\r\n");
        let first = lines.next().unwrap_or_default();
        let mut words = first.splitn(3, ' ');
        let method = words.next().unwrap_or_default().to_string();
        let target = words.next().unwrap_or_default().to_string();
        let headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.to_string(), value.trim().to_string()))
            .collect();
        Head { line: Line::Request { method, target }, headers }
    }
}

/// The ingress rule that took an exchange, and the visitor of it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Route {
    /// The index of the rule in `Ingress::rules`.
    pub rule: usize,
    /// The `hostname` of the rule; `None` for a rule with no hostname.
    pub hostname: Option<String>,
    /// The `Host` of the request of the visitor, before `httpHostHeader`
    /// of the rule changes it (the host of the URL when the request has
    /// no `Host`).
    pub host: String,
}

/// A failure of the crate in the capture of an exchange. The copy to the
/// peers is not changed by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fault {
    /// The framing of the chunked request body is not valid. The sink gets
    /// no more of the request body.
    RequestChunking,
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::RequestChunking => f.write_str("the chunked request body is not valid"),
        }
    }
}

/// The end of an exchange that the sink did not refuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct End {
    /// The status of the answer, or `None` when no answer head reached the
    /// visitor.
    pub status: Option<u16>,
    /// When the first part of the exchange went to the sink.
    pub started: SystemTime,
    /// The time from `started` to the end of both copies.
    pub elapsed: Duration,
    /// True when the copy ended with no error. A visitor or an origin that
    /// closes early gives false.
    pub complete: bool,
}

/// The answer of a sink that keeps no more of an exchange (a full ring, a
/// closed reader).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused;

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the capture sink refused the exchange")
    }
}

impl std::error::Error for Refused {}

/// The sink of the consumer for the HTTP exchanges of a tunnel. `exchange`
/// joins the heads, the body parts and the end of one exchange; it is
/// unique in one run of the tunnel. Each method must return at once.
pub trait Capture: Send + Sync {
    /// The rule and the visitor of the exchange. It comes once, before the
    /// request head, for a request that an ingress rule took.
    fn route(&self, _exchange: u64, _route: &Route) -> Result<(), Refused> {
        Ok(())
    }

    /// A failure of the crate in the capture of the exchange. It comes at
    /// most once, before [`Capture::end`]; the end follows, with
    /// `complete` as the copy gave it.
    fn fault(&self, _exchange: u64, _fault: Fault) {}

    /// A head that went to its peer.
    fn head(&self, exchange: u64, head: &Head) -> Result<(), Refused>;

    /// A part of a body that went to its peer, as its payload.
    fn body(&self, exchange: u64, direction: Direction, bytes: &[u8]) -> Result<(), Refused>;

    /// The end of the exchange. It comes once, after each other call.
    fn end(&self, exchange: u64, end: &End);
}

/// The capture of one exchange in the crate. With no sink, each call does
/// nothing. Its drop gives the end to a sink that took a part.
pub(crate) struct Tap {
    sink: Option<Arc<dyn Capture>>,
    id: u64,
    begun: OnceLock<(Instant, SystemTime)>,
    live: AtomicBool,
    heads_only: AtomicBool,
    complete: AtomicBool,
    /// The status of the answer; 0 before the answer head.
    status: AtomicU16,
    /// The decoder of a chunked request body, for the sink only.
    chunks: Mutex<Option<Dechunk>>,
    /// The first failure of the capture; the sink gets it before the end.
    fault: OnceLock<Fault>,
}

impl Tap {
    pub(crate) fn new(sink: Option<Arc<dyn Capture>>, id: u64) -> Tap {
        Tap {
            live: AtomicBool::new(sink.is_some()),
            sink,
            id,
            begun: OnceLock::new(),
            heads_only: AtomicBool::new(false),
            complete: AtomicBool::new(false),
            status: AtomicU16::new(0),
            chunks: Mutex::new(None),
            fault: OnceLock::new(),
        }
    }

    /// A tap with no sink.
    pub(crate) fn off() -> Tap {
        Tap::new(None, 0)
    }

    fn sink(&self) -> Option<&dyn Capture> {
        self.sink.as_deref().filter(|_| self.live.load(Ordering::Relaxed))
    }

    fn take(&self, given: Result<(), Refused>) {
        if given.is_err() && self.live.swap(false, Ordering::Relaxed) {
            tracing::debug!(exchange = self.id, "the capture sink refused an exchange; the copy goes on");
        }
    }

    fn begin(&self) {
        self.begun.get_or_init(|| (Instant::now(), SystemTime::now()));
    }

    /// The bodies of this exchange do not go to the sink (an upgrade).
    pub(crate) fn heads_only(&self) {
        self.heads_only.store(true, Ordering::Relaxed);
    }

    /// The request body is chunked: the sink gets its decoded payload.
    pub(crate) fn chunked_request(&self) {
        if self.sink().is_some() {
            *self.chunks.lock().unwrap_or_else(|e| e.into_inner()) = Some(Dechunk::default());
        }
    }

    /// The rule and the visitor of the exchange.
    pub(crate) fn route(&self, route: impl FnOnce() -> Route) {
        if let Some(sink) = self.sink() {
            self.begin();
            self.take(sink.route(self.id, &route()));
        }
    }

    /// The request head that went to the origin. `head` runs only for a
    /// live sink.
    pub(crate) fn request(&self, head: impl FnOnce() -> Head) {
        if let Some(sink) = self.sink() {
            self.begin();
            self.take(sink.head(self.id, &head()));
        }
    }

    /// The answer head that went to the edge.
    pub(crate) fn response(&self, status: u16, headers: &[(String, String)]) {
        self.status.store(status, Ordering::Relaxed);
        if let Some(sink) = self.sink() {
            self.begin();
            let head = Head { line: Line::Response { status }, headers: headers.to_vec() };
            self.take(sink.head(self.id, &head));
        }
    }

    /// A part of a body that went to its peer.
    pub(crate) fn body(&self, direction: Direction, bytes: &[u8]) {
        let Some(sink) = self.sink().filter(|_| !bytes.is_empty() && !self.heads_only.load(Ordering::Relaxed)) else {
            return;
        };
        self.begin();
        if direction == Direction::Request {
            let mut chunks = self.chunks.lock().unwrap_or_else(|e| e.into_inner());
            if self.fault.get().is_some() {
                return;
            }
            if let Some(decoder) = chunks.as_mut() {
                let mut given = Ok(());
                let framed = decoder.feed(bytes, &mut |payload| {
                    given = sink.body(self.id, direction, payload);
                    given.is_ok()
                });
                // A chunked body that the crate cannot decode stops the
                // capture of the request body; the origin judges the bytes,
                // and the sink still gets its end.
                if framed.is_err() {
                    let _ = self.fault.set(Fault::RequestChunking);
                }
                self.take(given);
                return;
            }
        }
        self.take(sink.body(self.id, direction, bytes));
    }

    /// The copy ended with no error.
    pub(crate) fn complete(&self) {
        self.complete.store(true, Ordering::Relaxed);
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        let (Some(sink), Some((at, started))) = (self.sink(), self.begun.get()) else {
            return;
        };
        if let Some(fault) = self.fault.get() {
            sink.fault(self.id, *fault);
        }
        let status = Some(self.status.load(Ordering::Relaxed)).filter(|s| *s != 0);
        let end =
            End { status, started: *started, elapsed: at.elapsed(), complete: self.complete.load(Ordering::Relaxed) };
        sink.end(self.id, &end);
    }
}
