//! A lane of a dial: one connection to the listener at a time, with TLS
//! and the pin of its key, made again after a cut, and the backoff of its
//! failures.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{BoxedIo, Dial, Target};
use pod_identity::PublicKey;
use rustls::pki_types::ServerName;
use tokio::time::{Instant, timeout};

use super::pipes::Ends;
use super::wire::{self, Conn, Head};
use super::{ALPN, BACKOFF_FIRST, BACKOFF_MAX, BUSY_LIMIT, Form};
use crate::carrier::{CarrierError, CarrierErrorKind};
use crate::pin;

type Tls = tokio_rustls::client::TlsStream<BoxedIo>;

/// What the lanes of one dial share.
pub(super) struct Dialing {
    pub name: &'static str,
    pub form: Form,
    pub config: Arc<rustls::ClientConfig>,
    pub way: Arc<dyn Dial>,
    pub target: Target,
    pub server_name: ServerName<'static>,
    /// The `Host` of each request.
    pub host: String,
    /// The path of the lanes, with no `/` at its end.
    pub base: String,
    /// The id of the session, 32 hex digits.
    pub id: String,
    pub own: PublicKey,
    pub connect_timeout: Duration,
    pub handshake_timeout: Duration,
    pub answer_timeout: Duration,
    pub hold: Duration,
    pub idle: Duration,
    pub ends: Arc<Ends>,
}

impl Dialing {
    pub fn err(&self, kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
        CarrierError::new(self.name, kind, text)
    }

    /// End the session with the error that ended a lane.
    pub fn fail(&self, e: &CarrierError) {
        let kind = match e.kind {
            CarrierErrorKind::Usage => io::ErrorKind::InvalidInput,
            CarrierErrorKind::InUse => io::ErrorKind::AddrInUse,
            CarrierErrorKind::Unavailable => io::ErrorKind::ConnectionRefused,
            CarrierErrorKind::NotFound => io::ErrorKind::NotFound,
            CarrierErrorKind::Refused | CarrierErrorKind::KeyRefused => io::ErrorKind::PermissionDenied,
            CarrierErrorKind::Io(kind) => kind,
        };
        self.ends.fail(kind, e.to_string());
    }

    /// The target of a request to the lane `lane`, with `rest` of its query.
    pub fn target(&self, lane: &str, rest: &str) -> String {
        format!("{}/{lane}?s={}{rest}", self.base, self.id)
    }
}

/// Why a request of a lane failed.
#[derive(Debug)]
pub(super) enum Failed {
    /// A cut, a time limit, no listener: ask again.
    Again(CarrierError),
    /// 429: ask again after this wait.
    Busy(Duration),
    /// The listener answers no push (406).
    NoPush,
    /// The end of the lane.
    Fatal(CarrierError),
}

/// A lane: its connection, the key that the listener proved, its backoff.
pub(super) struct Lane {
    pub d: Arc<Dialing>,
    conn: Option<Conn<Tls>>,
    pub key: Option<PublicKey>,
    pub retry: Retry,
}

impl Lane {
    pub fn new(d: Arc<Dialing>) -> Lane {
        Lane { d, conn: None, key: None, retry: Retry::default() }
    }

    /// A connection through the way out with TLS, the pin and the ALPN.
    async fn connect(&mut self) -> Result<(), CarrierError> {
        let d = self.d.clone();
        let connected = pin::dial(d.name, &*d.way, &d.target, d.connect_timeout).await?;
        let shake = async {
            let tls = tokio_rustls::TlsConnector::from(d.config.clone())
                .connect(d.server_name.clone(), connected.io)
                .await
                .map_err(|e| pin::tls_error(d.name, format!("the TLS handshake with {}", d.target), e))?;
            let conn = tls.get_ref().1;
            pin::check_alpn(d.name, conn.alpn_protocol(), ALPN, &d.target)?;
            let key = pin::proved(d.name, conn.peer_certificates(), &d.target)?;
            Ok::<_, CarrierError>((tls, key))
        };
        let (tls, key) = timeout(d.handshake_timeout, shake).await.unwrap_or_else(|_| {
            Err(d.err(
                CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                format!("the TLS handshake with {} took more than {:?}", d.target, d.handshake_timeout),
            ))
        })?;
        self.conn = Some(Conn::new(tls));
        self.key = Some(key);
        Ok(())
    }

    /// Send a request, and read the head of its answer within `limit`. A
    /// status out of `want` is a failure: 429 waits, 404, 409 and 410 end
    /// the lane, 406 is [`Failed::NoPush`].
    pub async fn ask(
        &mut self,
        method: &str,
        target: &str,
        body: Option<&[u8]>,
        accept: Option<&str>,
        limit: Duration,
        want: &[u16],
    ) -> Result<Head, Failed> {
        let d = self.d.clone();
        if self.conn.is_none() {
            self.connect().await.map_err(again_or_refused)?;
        }
        let Some(conn) = self.conn.as_mut() else { unreachable!("a lane with no connection after its connect") };
        let mut out = wire::request(method, target, &d.host, body.map(<[u8]>::len), accept).into_bytes();
        out.extend_from_slice(body.unwrap_or_default());
        let run = async {
            conn.send(&out).await?;
            conn.head().await
        };
        let head = match timeout(limit, run).await {
            Ok(Ok(Some(head))) => head,
            Ok(Ok(None)) => return Err(self.cut(io::ErrorKind::UnexpectedEof.into())),
            Ok(Err(e)) => return Err(self.cut(e)),
            Err(_) => return Err(self.cut(io::Error::new(io::ErrorKind::TimedOut, format!("no answer in {limit:?}")))),
        };
        let Some(status) = head.status() else {
            self.conn = None;
            return Err(Failed::Fatal(
                d.err(CarrierErrorKind::Io(io::ErrorKind::InvalidData), "an answer with no status"),
            ));
        };
        if want.contains(&status) {
            return Ok(head);
        }
        // The body of a refusal is short; it goes before the next request.
        let skip = match head.length() {
            Ok(Some(len)) if len <= 4096 => timeout(limit, self.body(len)).await.is_ok_and(|r| r.is_ok()),
            _ => false,
        };
        if !skip || head.close() {
            self.conn = None;
        }
        let fatal = |kind, text: String| Err(Failed::Fatal(d.err(kind, text)));
        let at = &d.target;
        match status {
            429 => {
                let after = head.field("retry-after").and_then(|v| v.parse::<u64>().ok()).unwrap_or(1).min(60);
                Err(Failed::Busy(Duration::from_secs(after)))
            }
            406 => Err(Failed::NoPush),
            404 => fatal(CarrierErrorKind::NotFound, format!("the listener at {at} has no session {}", d.id)),
            410 => fatal(
                CarrierErrorKind::Io(io::ErrorKind::BrokenPipe),
                format!("the listener at {at} ended the session"),
            ),
            409 => fatal(
                CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                format!("the listener at {at} found the offsets of the session out of order"),
            ),
            _ => fatal(
                CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                format!("the listener at {at} answered {target} with {status}"),
            ),
        }
    }

    /// A failed exchange: the connection goes; a refused key ends the lane.
    fn cut(&mut self, e: io::Error) -> Failed {
        self.conn = None;
        again_or_refused(pin::answer_error(self.d.name, &self.d.target, &self.d.own, e))
    }

    /// A body of `len` bytes of the answer.
    pub async fn body(&mut self, len: usize) -> Result<Vec<u8>, Failed> {
        let Some(conn) = self.conn.as_mut() else { return Err(self.cut(io::ErrorKind::NotConnected.into())) };
        match conn.body(len).await {
            Ok(body) => Ok(body),
            Err(e) => Err(self.cut(e)),
        }
    }

    /// The next chunk of the answer, within `limit`; none at its end.
    pub async fn chunk(&mut self, max: usize, limit: Duration) -> Result<Option<Vec<u8>>, Failed> {
        let Some(conn) = self.conn.as_mut() else { return Err(self.cut(io::ErrorKind::NotConnected.into())) };
        match timeout(limit, conn.chunk(max)).await {
            Ok(Ok(chunk)) => Ok(chunk),
            Ok(Err(e)) => Err(self.cut(e)),
            Err(_) => Err(self.cut(io::Error::new(io::ErrorKind::TimedOut, format!("no chunk in {limit:?}")))),
        }
    }

    /// The end of an answer: the connection stays unless the listener
    /// closes it.
    pub fn done(&mut self, head: &Head) {
        if head.close() {
            self.conn = None;
        }
    }

    /// The length of the body of `head`, at most `max`.
    pub fn length(&mut self, head: &Head, max: usize) -> Result<usize, Failed> {
        match head.length() {
            Ok(Some(len)) if len <= max => Ok(len),
            _ => {
                self.conn = None;
                let text = format!("an answer of {} with no length of at most {max} bytes", self.d.target);
                Err(Failed::Fatal(self.d.err(CarrierErrorKind::Io(io::ErrorKind::InvalidData), text)))
            }
        }
    }
}

fn again_or_refused(e: CarrierError) -> Failed {
    if matches!(e.kind, CarrierErrorKind::Refused | CarrierErrorKind::KeyRefused) {
        Failed::Fatal(e)
    } else {
        Failed::Again(e)
    }
}

/// The backoff of a lane: 100 ms doubled up to 5 s after each failure,
/// the end of the lane when it fails for `idle`, or at the third 429 in a
/// row.
#[derive(Debug)]
pub(super) struct Retry {
    since: Option<Instant>,
    wait: Duration,
    busy: u32,
}

impl Default for Retry {
    fn default() -> Retry {
        Retry { since: None, wait: BACKOFF_FIRST, busy: 0 }
    }
}

impl Retry {
    pub fn ok(&mut self) {
        *self = Retry::default();
    }

    /// Wait after `failed`, or give the error that ends the lane.
    pub async fn after(&mut self, d: &Dialing, failed: Failed) -> Result<(), CarrierError> {
        match failed {
            Failed::Fatal(e) => Err(e),
            Failed::NoPush => Err(d.err(CarrierErrorKind::Unavailable, "the listener answers no push")),
            Failed::Busy(after) => {
                self.busy += 1;
                if self.busy >= BUSY_LIMIT {
                    let text = format!("the listener at {} answered 429 {BUSY_LIMIT} times in a row", d.target);
                    return Err(d.err(CarrierErrorKind::Unavailable, text));
                }
                tokio::time::sleep(after).await;
                Ok(())
            }
            Failed::Again(e) => {
                self.busy = 0;
                let since = *self.since.get_or_insert_with(Instant::now);
                if since.elapsed() >= d.idle {
                    let text = format!("the lane failed for {:?}: {e}", d.idle);
                    return Err(d.err(CarrierErrorKind::Io(io::ErrorKind::TimedOut), text));
                }
                tokio::time::sleep(self.wait).await;
                self.wait = (self.wait * 2).min(BACKOFF_MAX);
                Ok(())
            }
        }
    }
}
