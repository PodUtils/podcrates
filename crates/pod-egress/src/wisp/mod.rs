//! A Wisp client: many TCP streams over one WebSocket, by Mercury
//! Workshop's protocol, version 2 and version 1, written from its document
//! (MercuryWorkshop/wisp-protocol:protocol.md). The server dials the host
//! and the port that each CONNECT names; its CONTINUE says how many DATA
//! packets it can buffer for a stream, and this side sends none past that
//! credit; its CLOSE gives a reason. Version 2 opens with an INFO from each
//! side, and only where the upgrade offers a protocol; a server that sends
//! its CONTINUE first speaks version 1, with no INFO. Each stream is given
//! to its caller as a byte stream ([`WispStream`]).
//!
//! The dial is that of a WebSocket by its URL (`pod_ws::url`): TLS checked
//! for the server's name, through the caller's way out, one connection, and
//! no token. A server of version 2 that asks for a password takes the
//! caller's [`Login`] (the password extension). A forwarder whose template
//! is `wisp+wss://HOST/PATH/` takes this road (`forward`). A [`Road`] is a
//! [`pod_core::Dial`] on one connection: a dial of a target is a stream to
//! it. A [`Reopening`] is one that opens a new connection on the next dial
//! after its connection ended.

#[cfg(feature = "forwarder")]
pub(crate) mod forward;
mod handshake;
mod login;
mod mux;
pub mod packet;
mod reopen;
mod stream;

use std::sync::Arc;
use std::time::Duration;

pub use handshake::{Opened, answer};
pub use login::{Login, MAX_USER};
pub use packet::Reason;
pub use reopen::{Reopening, TRIES};
pub use stream::WispStream;
// What the end of a stream's input does, as on each forwarder's road.
pub use crate::end::AtEnd;

use pod_core::{BoxFuture, Class, Connected, Event, Failure, Stage, Target};
use pod_tls::Trust;
use pod_ws::url::{self, WsUrl};
use pod_ws::{ConnectError, Way};
use tokio::time::Instant;

use crate::Note;

/// The kind of the event of a Wisp server's own words.
pub const NOTE: &str = "wisp.note";

/// The protocol that the upgrade offers. A server speaks version 2 only
/// where the request names one, whatever its value; `wisp-v2` is the value
/// of Mercury Workshop's own client (read in
/// MercuryWorkshop/wisp-js:src/client/connection.mjs).
pub const PROTOCOL: &str = "wisp-v2";

/// The newest version that this side speaks: 2.1, the document's.
pub const VERSION: (u8, u8) = (2, 1);

/// The largest DATA payload sent: 32 KiB, as each forwarder's frame.
const CHUNK: usize = pod_ws::frame::FORWARD_SEND_PAYLOAD;

/// The largest message taken from a server, which publishes no size:
/// wisp-server-python sends what one read of 64 KiB gave.
pub const RECEIVE_CAP: usize = 1024 * 1024;

/// The most characters of a server's words that a note shows.
const NOTE_CHARS: usize = 200;

/// What a connection needs beside its URL.
#[derive(Clone)]
pub struct Connect {
    /// The way out: the dialer, the TLS context, the user agent.
    pub way: Way,
    /// The trust of the server's certificate.
    pub trust: Trust,
    /// Bound on the dial, on the upgrade, and on the opening.
    pub timeout: Duration,
    /// Where the server's message of the day and its other words go.
    pub note: Note,
    /// The user name and the password for a server of version 2 that lists
    /// the password extension; it goes over `wss://` alone, or to this host.
    pub login: Option<Login>,
}

impl std::fmt::Debug for Connect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connect")
            .field("way", &self.way)
            .field("timeout", &self.timeout)
            .field("login", &self.login)
            .finish_non_exhaustive()
    }
}

/// Why a connection or a stream carried nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WispError {
    /// The dial or the upgrade, as `pod_ws::url::dial` gives it.
    Connect(ConnectError),
    /// The opening: a packet out of turn, an extension that this side
    /// cannot meet, or no opening within the bound.
    Opening(String),
    /// The server's CLOSE of the connection, on stream 0.
    Refused(Reason),
    /// The server's CLOSE of a stream, with its reason.
    Closed(Reason),
    /// The WebSocket's Close.
    Gone { code: Option<u16>, reason: String },
    /// A failure of the WebSocket, or a target that cannot go.
    Failed(String),
}

impl std::fmt::Display for WispError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WispError::Connect(e) => write!(f, "{e}"),
            WispError::Opening(why) | WispError::Failed(why) => f.write_str(why),
            WispError::Refused(r) => write!(f, "the Wisp server refused the connection: {r}"),
            WispError::Closed(r) => write!(f, "the Wisp server closed the stream: {r}"),
            WispError::Gone { code, reason } => {
                let code = code.map_or_else(|| "no code".to_string(), |c| c.to_string());
                match reason.is_empty() {
                    true => write!(f, "the Wisp server closed the WebSocket: {code}"),
                    false => write!(f, "the Wisp server closed the WebSocket: {code} {reason}"),
                }
            }
        }
    }
}

impl std::error::Error for WispError {}

impl WispError {
    /// The stage and the class of the failure.
    pub fn stage_and_class(&self) -> (Stage, Class) {
        match self {
            WispError::Connect(e) => e.stage_and_class(),
            WispError::Opening(_) => (Stage::WebSocket, Class::Protocol),
            // The server refused the credential: a new try meets the same.
            WispError::Refused(r) if r.is_credential() => (Stage::Relay, Class::Refused),
            WispError::Refused(_) => (Stage::Origin, Class::Refused),
            WispError::Closed(r) if *r == Reason::BLOCKED => (Stage::Origin, Class::Policy),
            WispError::Closed(_) => (Stage::Origin, Class::Unavailable),
            WispError::Gone { .. } | WispError::Failed(_) => (Stage::WebSocket, Class::Unavailable),
        }
    }
}

impl From<WispError> for Failure {
    fn from(e: WispError) -> Failure {
        let (stage, class) = e.stage_and_class();
        Failure::caused(stage, class, e.to_string(), e)
    }
}

/// How a stream ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// The server's CLOSE: [`Reason::VOLUNTARY`] when the target closed.
    Closed(Reason),
    /// This side's CLOSE.
    ClosedHere,
    /// The WebSocket's Close, which ends each stream.
    Gone { code: Option<u16>, reason: String },
    /// A failure of the WebSocket.
    Failed(String),
}

impl End {
    fn into_error(self) -> WispError {
        match self {
            End::Closed(r) => WispError::Closed(r),
            End::ClosedHere => WispError::Failed("this side closed the stream".into()),
            End::Gone { code, reason } => WispError::Gone { code, reason },
            End::Failed(why) => WispError::Failed(why),
        }
    }
}

/// Called once, with a stream's end, before its caller reads the end of its
/// bytes: a forwarder's status is set by it, so that the line of an end is
/// there when the session sees it.
pub(crate) type OnEnd = Box<dyn Fn(&End) + Send + Sync>;

/// A server's words for a line: one line, made safe for a terminal, short.
fn note_text(bytes: &[u8]) -> String {
    let line = pod_core::text::one_line(&String::from_utf8_lossy(bytes));
    match line.char_indices().nth(NOTE_CHARS) {
        Some((at, _)) => format!("{}...", &line[..at]),
        None => line,
    }
}

/// A Wisp connection: one WebSocket, which closes once this handle and
/// each of its streams are gone.
pub struct Wisp {
    shared: Arc<mux::Shared>,
}

impl std::fmt::Debug for Wisp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wisp").field("opened", &self.shared.opened).finish()
    }
}

impl Wisp {
    /// Dial `url`, upgrade offering [`PROTOCOL`], and open: version 2 when
    /// the server sends its INFO, version 1 when it sends its CONTINUE. A
    /// server that refuses the offer of a protocol (HTTP 400) is dialed once
    /// more with none, as version 1 asks none.
    pub async fn connect(url: &WsUrl, how: &Connect) -> Result<Wisp, WispError> {
        if how.login.is_some() && !url.tls && !pod_proxy::env::is_loopback(&url.host) {
            let why = "a Wisp password goes over wss:// alone, or to this host: the server's URL is ws://";
            return Err(WispError::Connect(ConnectError::Config(why.into())));
        }
        let idle = Some(pod_ws::DEFAULT_IDLE_TIMEOUT);
        let offered = url::dial_offering(url, Some(PROTOCOL), &how.way, &how.trust, how.timeout, idle).await;
        let session = match offered {
            Ok(session) => session,
            Err(ConnectError::Refused { status: 400, .. }) => {
                url::dial(url, &how.way, &how.trust, how.timeout, idle).await.map_err(WispError::Connect)?
            }
            Err(e) => return Err(WispError::Connect(e)),
        };
        let session = session.naming("the Wisp server");
        session.set_receive_cap(RECEIVE_CAP);
        let opened = handshake::open(&session, how.timeout, how.login.as_ref()).await?;
        if let Some(motd) = &opened.motd {
            how.note.event(Event::info(NOTE, format!("the Wisp server's message: {motd}")));
        }
        let shared = Arc::new(mux::Shared::new(session, opened, how.note.clone()));
        tokio::spawn(mux::read(shared.clone()));
        Ok(Wisp { shared })
    }

    /// What the opening settled: the version, the buffer of each stream,
    /// whether the server confirms a stream, its message of the day.
    pub fn opened(&self) -> &Opened {
        &self.shared.opened
    }

    /// Whether the server confirms each stream once its target's socket is
    /// open (stream open confirmation, on both sides' INFO).
    pub fn confirms(&self) -> bool {
        self.shared.opened.confirm
    }

    /// Whether the connection is still open: false once its WebSocket
    /// closed or failed, or the server closed stream 0. A connection that is
    /// not open opens no stream.
    pub fn is_open(&self) -> bool {
        self.shared.is_open()
    }

    /// A stream to `host:port`: its CONNECT is sent, and its bytes may go at
    /// once, within the buffer of the opening. With [`AtEnd::Now`], the end
    /// of the caller's bytes sends a CLOSE (Wisp has no half-close); with
    /// [`AtEnd::AfterPeer`], the stream waits for the server's CLOSE, as the
    /// target may still answer. The host is checked as each target of the
    /// relay is.
    pub async fn open(&self, host: &str, port: u16, at: AtEnd) -> Result<WispStream, WispError> {
        self.open_noting(host, port, at, None).await
    }

    /// [`Wisp::open`], with `on_end` called at the stream's end.
    pub(crate) async fn open_noting(
        &self,
        host: &str,
        port: u16,
        at: AtEnd,
        on_end: Option<OnEnd>,
    ) -> Result<WispStream, WispError> {
        let opening = self.shared.open(host, port, at, on_end).await?;
        Ok(WispStream::new(opening, self.shared.clone()))
    }
}

/// A Wisp connection as a dialer: each dial is a stream of it, whose end
/// of input does what `at_end` says.
#[derive(Debug)]
pub struct Road {
    pub wisp: Wisp,
    pub at_end: AtEnd,
    /// The name of the server in a line: its URL with no query.
    pub shown: String,
}

impl pod_core::Dial for Road {
    fn label(&self) -> String {
        format!("wisp {}", self.shown)
    }

    fn proxy_label(&self, _target: &Target) -> Option<String> {
        Some(self.shown.clone())
    }

    /// A stream to `target`, open before `deadline`: confirmed where the
    /// server confirms streams, else not closed by the server within
    /// [`crate::carry::EARLY_WAIT`].
    fn dial<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(stream_to(&self.wisp, target, deadline, self.at_end, &self.shown))
    }

    fn names_inside(&self) -> bool {
        true
    }
}

/// A stream of `wisp` to `target`, open before `deadline`, as a dial of a
/// road gives it.
async fn stream_to(
    wisp: &Wisp,
    target: &Target,
    deadline: Instant,
    at_end: AtEnd,
    shown: &str,
) -> Result<Connected, Failure> {
    let left = crate::road::left(deadline, target)?;
    let host = target.host();
    let opening = wisp.open(&host, target.port(), at_end);
    let mut stream =
        tokio::time::timeout_at(deadline, opening).await.map_err(|_| crate::road::late(target, shown))??;
    let wait = if wisp.confirms() { left } else { left.min(crate::carry::EARLY_WAIT) };
    if let Err(e) = stream.opened(wait).await {
        stream.close().await;
        return Err(e.into());
    }
    Ok(Connected::new(Box::new(stream)))
}

impl Drop for Wisp {
    fn drop(&mut self) {
        self.shared.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_says_the_server_and_its_reason() {
        assert_eq!(
            WispError::Closed(Reason::REFUSED).to_string(),
            "the Wisp server closed the stream: 0x44, the destination refused the connection"
        );
        assert_eq!(
            WispError::Refused(Reason::INCOMPATIBLE).to_string(),
            "the Wisp server refused the connection: 0x04, incompatible extensions"
        );
        let gone = WispError::Gone { code: Some(1011), reason: "upstream".into() };
        assert_eq!(gone.to_string(), "the Wisp server closed the WebSocket: 1011 upstream");
        assert_eq!(End::Closed(Reason::BLOCKED).into_error(), WispError::Closed(Reason(0x48)));
    }

    #[test]
    fn a_note_is_one_safe_short_line() {
        assert_eq!(note_text(b"Welcome\r\n\x1b[2Jto Wisp"), "Welcome to Wisp");
        assert_eq!(note_text("y".repeat(300).as_bytes()).len(), NOTE_CHARS + 3);
    }
}
