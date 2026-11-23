//! VLESS: a server that the user runs, Xray-core's or another that speaks
//! its protocol (shoes, sing-box), takes the target from a short header and
//! dials it, so that it is a road in place of the relay. The stream goes
//! over a WebSocket (`ws`), or over XHTTP's plain HTTP requests on HTTP/2
//! (`xhttp`), or on HTTP/1.1 and two connections where HTTP/2 is refused
//! (`h1_modes`), which pass where a proxy or a front end refuses the
//! WebSocket upgrade. Each connection goes through the caller's way out
//! ([`Dial::way`]), its TLS checked for the link's name.
//!
//! The link and its ID come from a private file ([`link`]); the request and
//! the answer are `header`'s. The server answers with the target's first
//! bytes, so a session whose server speaks first (SSH) waits for that answer,
//! and one whose client may speak first (a pipe) takes the stream after a
//! short silence.
//!
//! A [`Road`] is a [`pod_core::Dial`]: a dial of a target is a session to it.

pub mod early;
mod extra;
mod h1;
mod h1_modes;
mod header;
pub mod link;
mod packet;
mod split;
// Its pump of binary messages serves the Trojan and VMess roads too.
pub(crate) mod ws;
mod xhttp;

use std::time::Duration;

use pod_core::{BoxFuture, Class, Connected, Failure, Stage, Target};
use pod_proxy::DialError;
use pod_tls::Trust;
use pod_ws::Way;
use tokio::time::Instant;

use crate::Duplex;
use crate::coalesce;

pub use crate::end::AtEnd;
pub use crate::replay::{Admitted, ReplayWindow, WINDOW};
pub use extra::Extra;
pub use header::{ENDED, not_vless};
pub use link::{Id, Mode, Server, Transport};

/// How long a session whose server speaks first waits for VLESS's answer,
/// which comes with the target's first bytes.
pub const ANSWER_WAIT: Duration = Duration::from_secs(10);
/// How long a pipe waits for an end before it takes the stream: a refused
/// ID ends the session at once.
pub const EARLY_WAIT: Duration = Duration::from_secs(3);

/// What a VLESS road needs beside its server and its target. The Trojan and
/// VMess roads take the same.
#[derive(Debug, Clone)]
pub struct Dial {
    /// The way out: the dialer, the TLS context, the user agent.
    pub way: Way,
    /// The trust of the server's certificate.
    pub trust: Trust,
    /// The bound of each step: TCP, TLS, the upgrade, each HTTP answer.
    pub timeout: Duration,
    /// How long to wait for VLESS's answer.
    pub answer: Duration,
    /// Whether no answer within that wait is a stream to take (a pipe).
    pub silence_is_ready: bool,
    /// What the end of this side's bytes does to a WebSocket: its end at
    /// once (an SSH session), or after the server's end (a pipe). XHTTP's
    /// modes do not read it: that end ends their upload alone.
    pub at_end: AtEnd,
    /// Early data (0-RTT), where the caller has a nonce and a window: a
    /// nonce that the window has seen within 10 minutes makes the session
    /// go on at 1-RTT. No early data goes otherwise.
    pub early: Option<early::Early>,
}

/// Why a VLESS server carried no session.
#[derive(Debug)]
pub enum VlessError {
    /// The target cannot go in VLESS's header.
    Target(String),
    /// TCP, or the host's proxy.
    Dial(DialError),
    /// TLS, the upgrade or HTTP/2 failed: what failed.
    Carry(String),
    /// An HTTP answer refused it: the upgrade's or XHTTP's status, and what
    /// it refused.
    Refused { status: u16, why: String },
    /// HTTP/1.1, where XHTTP's mode needs HTTP/2: stream-one.
    Http1(String),
    /// The server ended the session before VLESS's answer.
    Ended,
    /// An answer of another protocol: its first byte.
    NotVless(u8),
    /// No answer within the wait.
    Silent(Duration),
}

impl std::fmt::Display for VlessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VlessError::Target(why) | VlessError::Carry(why) | VlessError::Http1(why) => f.write_str(why),
            VlessError::Dial(e) => write!(f, "{e}"),
            VlessError::Refused { status, why } => write!(f, "the VLESS server refused {why}: HTTP {status}"),
            VlessError::Ended => f.write_str(ENDED),
            VlessError::NotVless(v) => f.write_str(&not_vless(*v)),
            VlessError::Silent(after) => write!(f, "no answer of the VLESS server within {} s", after.as_secs()),
        }
    }
}

impl std::error::Error for VlessError {}

impl From<VlessError> for Failure {
    fn from(e: VlessError) -> Failure {
        let (stage, class) = e.stage_and_class();
        Failure::caused(stage, class, e.to_string(), e)
    }
}

impl VlessError {
    /// The stage and the class of the failure. The server is the relay of
    /// the road: an end before its answer is its refusal of the ID.
    pub fn stage_and_class(&self) -> (Stage, Class) {
        match self {
            VlessError::Target(_) => (Stage::Connect, Class::Usage),
            VlessError::Dial(e) => e.stage_and_class(),
            VlessError::Carry(_) => (Stage::WebSocket, Class::Unavailable),
            VlessError::Refused { .. } => (Stage::WebSocket, Class::Refused),
            VlessError::Http1(_) => (Stage::WebSocket, Class::Unsupported),
            VlessError::Ended => (Stage::Relay, Class::Refused),
            VlessError::NotVless(_) => (Stage::Relay, Class::Protocol),
            VlessError::Silent(_) => (Stage::Relay, Class::Timeout),
        }
    }

    /// The line that says it, for the server `shown` and the target.
    pub fn line(&self, shown: &str, target: &str) -> String {
        match self {
            VlessError::Target(why) => format!("{target} cannot go to the VLESS server {shown}: {why}"),
            VlessError::Dial(e) => format!("the VLESS server {shown}: {e}"),
            VlessError::Carry(why) | VlessError::Http1(why) => format!("the VLESS server {shown}: {why}"),
            VlessError::Refused { status, why } => format!("the VLESS server {shown} refused {why}: HTTP {status}"),
            VlessError::Ended => format!("the VLESS server {shown}, for {target}: {ENDED}"),
            VlessError::NotVless(v) => format!("the VLESS server {shown}: {}", not_vless(*v)),
            VlessError::Silent(after) => format!(
                "the VLESS server {shown} gave no answer for {target} within {} s: the target said nothing, a \
                 proxy or a front end between holds the bytes, or the server holds the session with no target; \
                 Xray-core does so for a private address unless the finalRules of its freedom outbound allow it",
                after.as_secs()
            ),
        }
    }
}

impl From<header::AnswerError> for VlessError {
    fn from(e: header::AnswerError) -> VlessError {
        match e {
            header::AnswerError::Ended => VlessError::Ended,
            header::AnswerError::NotVless(v) => VlessError::NotVless(v),
            header::AnswerError::Io(e) => VlessError::Carry(format!("before VLESS's answer: {e}")),
        }
    }
}

/// A session to the target.
pub struct Opened {
    pub stream: Box<dyn Duplex>,
    /// What carried it: "a WebSocket", "XHTTP packet-up over HTTP/2", "XHTTP
    /// packet-up over HTTP/1.1 on two connections".
    pub how: String,
    /// VLESS's answer came within the wait.
    pub answered: bool,
    /// The write that carried the opening request and the first data: its
    /// bytes, whether the two were short of [`coalesce::TARGET`], and that
    /// the write was coalesced.
    pub first_write: coalesce::Report,
    /// What the session offered of early data.
    pub early: early::Offer,
}

/// A session to `host:port` through `server`, on the link's transport.
pub async fn open(server: &Server, host: &str, port: u16, dial: &Dial) -> Result<Opened, VlessError> {
    open_as(server, server.transport, host, port, dial).await
}

/// [`open`] on `transport`, as `doctor` tries each mode of XHTTP.
pub async fn open_as(
    server: &Server,
    transport: Transport,
    host: &str,
    port: u16,
    dial: &Dial,
) -> Result<Opened, VlessError> {
    let offer = early::Offer::of(dial.early.as_ref());
    let announced;
    let server = if offer.zero_rtt() {
        announced = server.with_early_data();
        &announced
    } else {
        server
    };
    let request = header::request(&server.id, host, port).map_err(VlessError::Target)?;
    let (stream, how, first_write): (Box<dyn Duplex>, String, coalesce::Report) = match transport {
        Transport::Ws => {
            let (stream, report) = ws::open(server, request, dial).await?;
            (Box::new(stream), transport.name(), report)
        }
        Transport::Xhttp(mode) => xhttp::open(server, mode.chosen(), request, dial).await?,
    };
    let mut stream = header::Answered::new(stream);
    let answered = match tokio::time::timeout(dial.answer, stream.answer()).await {
        Ok(Ok(())) => true,
        Ok(Err(e)) => return Err(e.into()),
        Err(_) if dial.silence_is_ready => false,
        Err(_) => return Err(VlessError::Silent(dial.answer)),
    };
    Ok(Opened { stream: Box::new(stream), how, answered, first_write, early: offer })
}

/// The modes that `doctor` tries for a link of XHTTP: the link's own, or
/// each where it says auto.
pub fn modes(transport: Transport) -> Vec<Transport> {
    match transport {
        Transport::Xhttp(Mode::Auto) => {
            [Mode::PacketUp, Mode::StreamUp, Mode::StreamOne].into_iter().map(Transport::Xhttp).collect()
        }
        other => vec![other],
    }
}

/// A VLESS server as a dialer: each dial is a session to its target.
#[derive(Debug, Clone)]
pub struct Road {
    pub server: Server,
    pub dial: Dial,
}

impl pod_core::Dial for Road {
    fn label(&self) -> String {
        format!("VLESS {}", self.server.shown())
    }

    fn proxy_label(&self, _target: &Target) -> Option<String> {
        Some(self.server.shown())
    }

    /// A session to `target`, open before `deadline`.
    fn dial<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let left = crate::road::left(deadline, target)?;
            let dial = Dial { timeout: self.dial.timeout.min(left), ..self.dial.clone() };
            let shown = self.server.shown();
            let host = target.host();
            let opening = open(&self.server, &host, target.port(), &dial);
            let opened = tokio::time::timeout_at(deadline, opening)
                .await
                .map_err(|_| crate::road::late(target, &shown))?
                .map_err(|e| {
                    let (stage, class) = e.stage_and_class();
                    Failure::caused(stage, class, e.line(&shown, &target.to_string()), e)
                })?;
            Ok(Connected::new(Box::new(opened.stream)))
        })
    }

    fn names_inside(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_failure_names_the_server_and_never_the_id() {
        let shown = "vless://v.example:443/ws";
        let ended = VlessError::Ended.line(shown, "h.example:22");
        let words = "the VLESS server vless://v.example:443/ws, for h.example:22: the server";
        assert!(ended.starts_with(words), "{ended}");
        assert!(ended.contains("refused the ID"), "{ended}");
        let refused = VlessError::Refused { status: 404, why: "the WebSocket upgrade at /ws".into() };
        assert_eq!(
            refused.line(shown, "h:22"),
            "the VLESS server vless://v.example:443/ws refused the WebSocket upgrade at /ws: HTTP 404"
        );
        assert!(VlessError::NotVless(72).line(shown, "h:22").contains("first byte is 72"));
        assert!(VlessError::Silent(ANSWER_WAIT).line(shown, "h:22").contains("within 10 s"));
        let auto = modes(Transport::Xhttp(Mode::Auto));
        assert_eq!(auto.len(), 3);
        assert_eq!(modes(Transport::Ws), vec![Transport::Ws]);
    }
}
