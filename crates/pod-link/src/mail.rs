//! What the mail carriers share (the features `smtp` and `imap`): the
//! ports of each service, the way that TLS starts, the credential of a
//! session, the TLS handshake, and the reader of a line. The protocol of
//! each service is [`crate::smtp`] and [`crate::imap`].
//!
//! A mail carrier speaks to the server of the network, not to the peer: the
//! dialer submits its bytes and fetches the bytes of the peer, and the
//! server is the store between them. The address of a dial is `HOST:PORT`
//! (see [`Endpoint`]).

use std::fmt;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use rustls::ClientConfig;
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader, ReadBuf};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::carrier::{BoxIo, CarrierError, CarrierErrorKind, Limit, Limits};

/// The submission port of SMTP with STARTTLS.
pub const SMTP_STARTTLS: u16 = 587;
/// The submission port of SMTP with TLS from the first byte.
pub const SMTP_IMPLICIT: u16 = 465;
/// The port of SMTP that this crate never uses (PodUtils/podnet#484,
/// MAIL-5): the egress broker refuses it by a fixed rule, and a carrier
/// that needs that rule relaxed is a carrier that changes the policy.
pub const SMTP_FORBIDDEN: u16 = 25;
/// The IMAP port with STARTTLS.
pub const IMAP_STARTTLS: u16 = 143;
/// The IMAP port with TLS from the first byte.
pub const IMAP_IMPLICIT: u16 = 993;

/// The name that the client of a mail service gives in its greeting. It
/// reads no environment and names no host of this machine.
pub const CLIENT_NAME: &str = "podlink.local";

/// The longest line of a mail service that the reader takes: an SMTP reply
/// or an IMAP response line. A longer one ends the session.
pub const MAX_LINE: usize = 8 << 10;

/// Check that a TCP socket binds on the loopback.
pub(crate) fn tcp_available(carrier: &'static str) -> Result<(), CarrierError> {
    use std::net::{Ipv4Addr, Ipv6Addr, TcpListener};
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .or_else(|_| TcpListener::bind((Ipv6Addr::LOCALHOST, 0)))
        .map(drop)
        .map_err(|e| {
            CarrierError::new(carrier, CarrierErrorKind::Unavailable, format!("TCP sockets on the loopback: {e}"))
        })
}

/// How TLS starts on the connection of a mail service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// The plain connection first, then STARTTLS.
    Starttls,
    /// TLS from the first byte.
    Implicit,
}

impl Mode {
    /// The mode of a submission port: 587 STARTTLS, 465 implicit TLS. Port
    /// 25 is refused by name, and another port has no mode of its own.
    pub fn of_smtp(port: u16) -> Result<Mode, String> {
        match port {
            SMTP_STARTTLS => Ok(Mode::Starttls),
            SMTP_IMPLICIT => Ok(Mode::Implicit),
            SMTP_FORBIDDEN => {
                Err("port 25 is never used for submission: use 587 with STARTTLS or 465 with TLS (MAIL-5, \
                 PodUtils/podnet#484)"
                    .to_string())
            }
            other => Err(format!("port {other} has no mode: use 587 (STARTTLS) or 465 (TLS), or give a mode")),
        }
    }

    /// The mode of an IMAP port: 143 STARTTLS, 993 implicit TLS.
    pub fn of_imap(port: u16) -> Result<Mode, String> {
        match port {
            IMAP_STARTTLS => Ok(Mode::Starttls),
            IMAP_IMPLICIT => Ok(Mode::Implicit),
            other => Err(format!("port {other} has no mode: use 143 (STARTTLS) or 993 (TLS), or give a mode")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Starttls => "starttls",
            Mode::Implicit => "implicit",
        }
    }
}

/// The host and the port of a mail server, as the dial address reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

impl Endpoint {
    /// `HOST:PORT`. A name is kept as it came, for the check of the
    /// certificate of the server.
    pub fn parse(text: &str) -> Result<Endpoint, String> {
        let (host, port) = text.rsplit_once(':').ok_or_else(|| format!("{text:?} is no HOST:PORT"))?;
        if host.is_empty() {
            return Err(format!("{text:?} is no HOST:PORT"));
        }
        let port: u16 = port.parse().map_err(|_| format!("{text:?} has no port"))?;
        Ok(Endpoint { host: host.trim_start_matches('[').trim_end_matches(']').to_string(), port })
    }

    pub fn text(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// The name that TLS checks: the host.
    pub fn server_name(&self) -> &str {
        &self.host
    }
}

/// The credential of one session of a mail service (MAIL-3): a user and a
/// secret that the caller made for the session. It is never the password
/// of the account.
#[derive(Clone)]
pub struct Credential {
    user: String,
    secret: String,
}

impl Credential {
    /// A credential of `user` and `secret`, each with no CR, LF or NUL. An
    /// empty one is refused.
    pub fn new(user: impl Into<String>, secret: impl Into<String>) -> Result<Credential, String> {
        let (user, secret) = (user.into(), secret.into());
        for (what, value) in [("user", &user), ("secret", &secret)] {
            if value.is_empty() {
                return Err(format!("the {what} of a session credential is empty"));
            }
            if value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
                return Err(format!("the {what} of a session credential has a control character"));
            }
        }
        Ok(Credential { user, secret })
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub(crate) fn secret(&self) -> &str {
        &self.secret
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential").field("user", &self.user).field("secret", &"<hidden>").finish()
    }
}

/// The lines of a mail service, read from a buffer over the connection.
pub type Lines = BufReader<BoxIo>;

/// Connect to `endpoint` over TCP, within `bound`. It gives the plain
/// connection, and the name that TLS checks.
pub(crate) async fn open(
    carrier: &'static str,
    endpoint: &Endpoint,
    bound: Duration,
) -> Result<(BoxIo, String), CarrierError> {
    let text = endpoint.text();
    let connect = TcpStream::connect((endpoint.host.as_str(), endpoint.port));
    let stream = match timeout(bound, connect).await {
        Err(_) => {
            return Err(CarrierError::new(
                carrier,
                CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                format!("no connection to {text} in {bound:?}"),
            ));
        }
        Ok(Err(e)) => return Err(CarrierError::io(carrier, format!("connect to {text}"), e)),
        Ok(Ok(stream)) => stream,
    };
    stream.set_nodelay(true).ok();
    Ok((Box::new(stream) as BoxIo, endpoint.server_name().to_string()))
}

/// The TLS of `io` for the name `name`, with the configuration of the
/// caller.
pub(crate) async fn wrap(
    carrier: &'static str,
    io: BoxIo,
    name: &str,
    config: Arc<ClientConfig>,
    bound: Duration,
) -> Result<Lines, CarrierError> {
    let words = pod_tls::Words::new("pod-link");
    let tls = pod_tls::tls_over(io, name, config, bound, &words)
        .await
        .map_err(|e| CarrierError::new(carrier, CarrierErrorKind::Io(io::ErrorKind::InvalidData), e.to_string()))?;
    Ok(BufReader::new(Box::new(tls) as BoxIo))
}

/// The configuration of TLS that the caller gave, or the reason that the
/// carrier cannot start TLS without one.
pub(crate) fn tls_config(
    carrier: &'static str,
    config: &Option<Arc<ClientConfig>>,
    mode: Mode,
) -> Result<Arc<ClientConfig>, CarrierError> {
    config.clone().ok_or_else(|| {
        CarrierError::new(
            carrier,
            CarrierErrorKind::Usage,
            format!("{} needs a TLS configuration: give one with with_tls(config)", mode.name()),
        )
    })
}

/// Read one line of at most `MAX_LINE` bytes, CRLF or LF. An empty value is
/// the end of the connection.
pub(crate) async fn next_line(lines: &mut Lines, carrier: &'static str) -> Result<Vec<u8>, CarrierError> {
    let mut out = Vec::new();
    loop {
        let (chunk, found) = {
            let available =
                lines.fill_buf().await.map_err(|e| CarrierError::io(carrier, "a read of the service", e))?;
            if available.is_empty() {
                return Ok(out);
            }
            let at = available.iter().position(|b| *b == b'\n');
            let taken = at.map_or(available.len(), |i| i + 1);
            (available[..taken].to_vec(), at.is_some())
        };
        out.extend_from_slice(&chunk);
        lines.consume(chunk.len());
        if out.len() > MAX_LINE {
            return Err(CarrierError::new(
                carrier,
                CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                format!("a line of more than {MAX_LINE} bytes"),
            ));
        }
        if found {
            return Ok(out);
        }
    }
}

/// Write `bytes` and flush them.
pub(crate) async fn write(lines: &mut Lines, carrier: &'static str, bytes: &[u8]) -> Result<(), CarrierError> {
    lines.write_all(bytes).await.map_err(|e| CarrierError::io(carrier, "a write to the service", e))?;
    lines.flush().await.map_err(|e| CarrierError::io(carrier, "a flush of the service", e))
}

/// The text of a line without its CRLF.
pub(crate) fn text(line: &[u8]) -> String {
    String::from_utf8_lossy(line).trim_end_matches(['\r', '\n']).to_string()
}

/// Report `limit` as the limit of the carrier and keep it for the caller.
pub(crate) fn report(limits: &Limits, limit: Limit) {
    limits.set(limit);
}

/// The bytes of one message of this crate's mail carriers: the body, decoded
/// from base64 when the message carries the headers of [`crate::smtp`], and
/// the body as it is for a message of another tool.
pub fn body(message: &[u8]) -> Vec<u8> {
    use base64::Engine as _;
    let split = message.windows(4).position(|window| window == b"\r\n\r\n");
    let Some(at) = split else {
        return message.to_vec();
    };
    let (head, rest) = message.split_at(at);
    let body = &rest[4..];
    let head = String::from_utf8_lossy(head).to_lowercase();
    if !head.contains("content-transfer-encoding: base64") || !head.contains("x-podlink-batch:") {
        return body.to_vec();
    }
    let clean: Vec<u8> = body.iter().copied().filter(|byte| !byte.is_ascii_whitespace()).collect();
    base64::engine::general_purpose::STANDARD.decode(&clean).unwrap_or_else(|_| body.to_vec())
}

/// One byte stream of the two halves of a mail tunnel: the bytes up go to
/// the submission carrier, and the bytes down come from the retrieval
/// carrier. [`crate::mail_pair`] makes it.
pub struct Duplex {
    up: BoxIo,
    down: BoxIo,
}

/// Join the write side of `up` and the read side of `down` into one byte
/// stream.
pub fn join(up: BoxIo, down: BoxIo) -> BoxIo {
    Box::new(Duplex { up, down })
}

impl AsyncRead for Duplex {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().down).poll_read(cx, buf)
    }
}

impl AsyncWrite for Duplex {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().up).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().up).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().up).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_submission_port_gives_its_mode_and_port_25_is_refused() {
        assert_eq!(Mode::of_smtp(587), Ok(Mode::Starttls));
        assert_eq!(Mode::of_smtp(465), Ok(Mode::Implicit));
        assert_eq!(Mode::of_imap(143), Ok(Mode::Starttls));
        assert_eq!(Mode::of_imap(993), Ok(Mode::Implicit));
        let refused = Mode::of_smtp(25).expect_err("port 25");
        assert!(refused.contains("port 25"), "{refused}");
        assert!(Mode::of_smtp(2525).is_err(), "a port with no mode of its own");
    }

    #[test]
    fn a_credential_hides_its_secret_and_refuses_a_control_character() {
        let credential = Credential::new("session-user", "made-up-secret").expect("a credential");
        let shown = format!("{credential:?}");
        assert!(shown.contains("session-user"), "{shown}");
        assert!(!shown.contains("made-up-secret"), "the secret is hidden: {shown}");
        assert!(Credential::new("user", "a\rb").is_err());
        assert!(Credential::new("", "secret").is_err());
    }

    #[test]
    fn an_endpoint_keeps_the_name_that_tls_checks() {
        let endpoint = Endpoint::parse("mail.example:587").expect("an endpoint");
        assert_eq!((endpoint.server_name(), endpoint.port), ("mail.example", 587));
        assert_eq!(Endpoint::parse("[::1]:465").expect("a v6 endpoint").host, "::1");
        assert!(Endpoint::parse("mail.example").is_err());
    }
}
