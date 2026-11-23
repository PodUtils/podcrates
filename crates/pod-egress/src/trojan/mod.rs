//! Trojan: a server that the user runs, trojan-gfw's, trojan-go's,
//! Xray-core's or sing-box's, takes the target from a short request after a
//! real TLS handshake and dials it, so that it is a road in place of the
//! relay. The stream goes in TLS alone, or in a WebSocket over TLS (`ws`),
//! which passes through a CDN and where a front end serves other paths. Each
//! connection goes through the caller's way out ([`Dial::way`]), its TLS
//! checked for the link's name.
//!
//! The link and its password come from a private file ([`link`]); the
//! request is `request`'s. Trojan has no answer, so the first bytes tell
//! the target from the server's fallback (`answer`): a session whose server
//! speaks first (SSH) waits for them, and one whose client may speak first
//! (a pipe) takes the stream after a short silence. The settings of a dial
//! are VLESS's ([`crate::vless::Dial`]), as both roads dial alike.
//!
//! A [`Road`] is a [`pod_core::Dial`]: a dial of a target is a session to it.

mod answer;
pub mod link;
mod request;
mod ws;

use std::time::Duration;

use pod_core::{BoxFuture, Class, Connected, Failure, Stage, Target};
use pod_proxy::DialError;
use tokio::time::Instant;

use crate::Duplex;

pub use crate::vless::{AtEnd, Dial};
pub use link::{Password, Server, Transport};

/// How long a session whose server speaks first waits for its first bytes.
pub const ANSWER_WAIT: Duration = Duration::from_secs(10);
/// How long a pipe waits before it takes the stream: a fallback answers
/// the request's bytes at once.
pub const EARLY_WAIT: Duration = Duration::from_secs(3);

/// The commands that Trojan's request names (`docs/protocol.md`): CONNECT,
/// and UDP associate.
pub const CONNECT: u8 = 0x01;
pub const UDP_ASSOCIATE: u8 = 0x03;

/// What this road carries, by name, so that a doctor's line says it without
/// reading a string that a failure built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// A stream to the target, Trojan's command 0x01.
    Tcp,
    /// UDP associate, Trojan's command 0x03.
    UdpAssociate,
}

impl Capability {
    /// Each command that Trojan names.
    pub const ALL: [Capability; 2] = [Capability::Tcp, Capability::UdpAssociate];

    /// Whether this road sends it. UDP associate is refused: this client
    /// supports TCP only, and no UDP path stands here.
    pub fn carried(self) -> bool {
        matches!(self, Capability::Tcp)
    }

    /// The command's number in Trojan's request.
    pub fn command(self) -> u8 {
        match self {
            Capability::Tcp => CONNECT,
            Capability::UdpAssociate => UDP_ASSOCIATE,
        }
    }

    /// The words of the outcome, for the line that a doctor prints.
    pub fn words(self) -> &'static str {
        match self {
            Capability::Tcp => "a TCP stream to the target (command 0x01) is carried",
            Capability::UdpAssociate => {
                "UDP associate (command 0x03) is refused: this client carries TCP to a target and no UDP path"
            }
        }
    }
}

/// Each of Trojan's commands with whether this road carries it, for the
/// line that a doctor prints without parsing a failure.
pub fn capability() -> [(Capability, bool); 2] {
    [(Capability::Tcp, Capability::Tcp.carried()), (Capability::UdpAssociate, Capability::UdpAssociate.carried())]
}

/// Why a Trojan server carried no session.
#[derive(Debug)]
pub enum TrojanError {
    /// The target cannot go in Trojan's request.
    Target(String),
    /// TCP, or the host's proxy.
    Dial(DialError),
    /// TLS or the upgrade failed: what failed.
    Carry(String),
    /// The WebSocket's upgrade was refused: its status, and what it refused.
    Refused { status: u16, why: String },
    /// The server's fallback answered: the password, or the request, was not
    /// the server's. The fallback's status line.
    Fallback(String),
    /// The server ended the session before any byte of the target.
    Ended,
    /// No byte within the wait.
    Silent(Duration),
}

impl std::fmt::Display for TrojanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrojanError::Target(why) | TrojanError::Carry(why) => f.write_str(why),
            TrojanError::Dial(e) => write!(f, "{e}"),
            TrojanError::Refused { status, why } => write!(f, "the Trojan server refused {why}: HTTP {status}"),
            TrojanError::Fallback(line) => write!(f, "the Trojan server handed the session to its fallback ({line:?})"),
            TrojanError::Ended => f.write_str("the Trojan server ended the session before the target said anything"),
            TrojanError::Silent(after) => write!(f, "nothing of the Trojan server within {} s", after.as_secs()),
        }
    }
}

impl std::error::Error for TrojanError {}

impl From<TrojanError> for Failure {
    fn from(e: TrojanError) -> Failure {
        let (stage, class) = e.stage_and_class();
        Failure::caused(stage, class, e.to_string(), e)
    }
}

impl TrojanError {
    /// The stage and the class of the failure. The server is the relay of
    /// the road: its fallback is its refusal of the password.
    pub fn stage_and_class(&self) -> (Stage, Class) {
        match self {
            TrojanError::Target(_) => (Stage::Connect, Class::Usage),
            TrojanError::Dial(e) => e.stage_and_class(),
            TrojanError::Carry(_) => (Stage::WebSocket, Class::Unavailable),
            TrojanError::Refused { .. } => (Stage::WebSocket, Class::Refused),
            TrojanError::Fallback(_) | TrojanError::Ended => (Stage::Relay, Class::Refused),
            TrojanError::Silent(_) => (Stage::Relay, Class::Timeout),
        }
    }

    /// The line that says it, for the server `shown` and the target. No line
    /// holds the password or its hash.
    pub fn line(&self, shown: &str, target: &str) -> String {
        match self {
            TrojanError::Target(why) => format!("{target} cannot go to the Trojan server {shown}: {why}"),
            TrojanError::Dial(e) => format!("the Trojan server {shown}: {e}"),
            TrojanError::Carry(why) => format!("the Trojan server {shown}: {why}"),
            TrojanError::Refused { status, why } => format!("the Trojan server {shown} refused {why}: HTTP {status}"),
            TrojanError::Fallback(line) => format!(
                "the Trojan server {shown} handed the session to its fallback, which answered {line:?}: the password \
                 of the link is not the server's, or the server reads no request of Trojan at this port or path"
            ),
            TrojanError::Ended => format!(
                "the Trojan server {shown}, for {target}: the server ended the session before the target said \
                 anything: a wrong password where the server has no fallback, or the server could not reach the \
                 target"
            ),
            TrojanError::Silent(after) => format!(
                "the Trojan server {shown} gave nothing for {target} within {} s: the target said nothing, a proxy \
                 or a front end between holds the bytes, or the server holds the session",
                after.as_secs()
            ),
        }
    }
}

/// A session to the target.
pub struct Opened {
    pub stream: Box<dyn Duplex>,
    /// What carried it: "TLS", "a WebSocket over TLS".
    pub how: &'static str,
    /// The target's first bytes came within the wait.
    pub answered: bool,
}

/// Trojan's request for `host:port` under `password`, as the road sends it:
/// the password's SHA-224 in hex, CRLF, CONNECT, the address, the port,
/// CRLF. A conformance vector reads it, and a caller may.
pub fn request(password: &str, host: &str, port: u16) -> Result<Vec<u8>, String> {
    request::request(&link::Password::new(password)?, host, port)
}

/// A session to `host:port` through `server`, on the link's transport.
pub async fn open(server: &Server, host: &str, port: u16, dial: &Dial) -> Result<Opened, TrojanError> {
    let request = request::request(&server.password, host, port).map_err(TrojanError::Target)?;
    let stream: Box<dyn Duplex> = match server.transport {
        Transport::Tls => Box::new(tls(server, request, dial).await?),
        Transport::Ws => Box::new(ws::open(server, request, dial).await?),
    };
    let mut stream = answer::Answered::new(stream);
    let answered = match tokio::time::timeout(dial.answer, stream.first()).await {
        Ok(Ok(())) => true,
        Ok(Err(answer::FirstError::Ended)) => return Err(TrojanError::Ended),
        Ok(Err(answer::FirstError::Fallback(line))) => return Err(TrojanError::Fallback(line)),
        Ok(Err(answer::FirstError::Io(e))) => {
            return Err(TrojanError::Carry(format!("before the target's bytes: {e}")));
        }
        Err(_) if dial.silence_is_ready => false,
        Err(_) => return Err(TrojanError::Silent(dial.answer)),
    };
    Ok(Opened { stream: Box::new(stream), how: server.transport.name(), answered })
}

/// TLS for the link's name, then the request as the first bytes of its
/// stream, in one write, as trojan-gfw reads it whole from its first read.
async fn tls(server: &Server, request: Vec<u8>, dial: &Dial) -> Result<impl Duplex + use<>, TrojanError> {
    use tokio::io::AsyncWriteExt;
    let opened = dial.way.open_tls(&server.host, server.port, &server.sni, &dial.trust, dial.timeout, &[]).await;
    let (mut tls, _) = opened.map_err(|e| ws::failed(e, "/"))?;
    let sent = async {
        tls.write_all(&request).await?;
        tls.flush().await
    };
    tokio::time::timeout(dial.timeout, sent)
        .await
        .map_err(|_| TrojanError::Carry(format!("Trojan's request not sent within {} s", dial.timeout.as_secs())))?
        .map_err(|e| TrojanError::Carry(format!("sending Trojan's request failed: {e}")))?;
    Ok(tls)
}

/// A Trojan server as a dialer: each dial is a session to its target.
#[derive(Debug, Clone)]
pub struct Road {
    pub server: Server,
    pub dial: Dial,
}

impl pod_core::Dial for Road {
    fn label(&self) -> String {
        format!("Trojan {}", self.server.shown())
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
    fn each_failure_names_the_server_and_never_the_password() {
        let shown = "trojan://t.example:443";
        let fallback = TrojanError::Fallback("HTTP/1.1 400 Bad Request".into()).line(shown, "h.example:22");
        assert!(fallback.contains("fallback, which answered \"HTTP/1.1 400 Bad Request\""), "{fallback}");
        assert!(fallback.contains("password of the link is not the server's"), "{fallback}");
        assert!(TrojanError::Ended.line(shown, "h:22").contains("before the target said anything"));
        assert!(TrojanError::Silent(ANSWER_WAIT).line(shown, "h:22").contains("within 10 s"));
        let refused = TrojanError::Refused { status: 404, why: "the WebSocket upgrade at /tr".into() };
        assert_eq!(
            refused.line(shown, "h:22"),
            "the Trojan server trojan://t.example:443 refused the WebSocket upgrade at /tr: HTTP 404"
        );
    }

    /// VEC-6: UDP associate is refused by name, and the doctor reads it
    /// from the capability and not from a failure's words.
    #[test]
    fn udp_associate_is_refused_by_name_and_tcp_is_carried() {
        let capabilities = capability();
        assert_eq!(capabilities.len(), 2);
        for (command, carried) in capabilities {
            assert_eq!(carried, command.carried());
            match command {
                Capability::Tcp => {
                    assert!(carried);
                    assert_eq!(command.command(), 0x01);
                    assert!(command.words().contains("0x01"));
                }
                Capability::UdpAssociate => {
                    assert!(!carried);
                    assert_eq!((command.command(), UDP_ASSOCIATE), (0x03, 0x03));
                    let words = command.words();
                    assert!(words.contains("UDP associate") && words.contains("refused"), "{words}");
                }
            }
        }
    }
}
