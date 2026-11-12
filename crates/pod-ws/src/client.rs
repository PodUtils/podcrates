//! Connecting to the relay: TCP through the caller's way out ([`Way`]), TLS
//! with the caller's context, and the WebSocket upgrade. Every step is
//! bounded, and every failure says which step failed and why.
//!
//! Verdicts from [`doctor`] are three-valued (`ok`, `FAIL`, `????`): a check
//! that could not run is never reported as passing.

use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_proxy::http;
use pod_proxy::{Cut, DialError, authority};
use pod_tls::Trust;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{Verdict, WsError};
use crate::frame::{self, Frame};
use crate::handshake;
use crate::limits::Limits;
pub use crate::session::{RelaySession, RelayStream};
use crate::way::Way;

// The failure of a request of pod-proxy's client, as a ConnectError.
mod get;
pub use get::request_error;

#[cfg(test)]
mod cut_tests;

/// Default bound on connecting: proxy exchange, TCP, TLS and the upgrade.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// Default bound on waiting for the next frame. The relay sends a keepalive
/// every 25 s, so this only fires when the link is dead.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// How often a session pings the relay, and how many silent intervals in a
/// row mean the link is dead ([`crate::RelaySession::watch_liveness`]): 30 s
/// of nothing at all, while a healthy relay sends at least a keepalive every
/// 25 s (the relay answered every Ping, measured 2026-10-08).
pub const LIVENESS_EVERY: Duration = Duration::from_secs(10);
pub const LIVENESS_ALLOWED: u32 = 3;

/// Bound on one write to an open session.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(60);

/// The largest head of the relay's answer to the upgrade.
pub const UPGRADE_HEAD: usize = 16 * 1024;

/// What to connect to.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    /// The relay's WebSocket path, e.g. `/connect/example.org/22`.
    pub path: String,
}

/// How to connect. (Named so it is not confused with `rustls::ClientConfig`.)
#[derive(Debug, Clone)]
pub struct WsClientConfig {
    pub endpoint: Endpoint,
    /// Trust anchors for the relay's certificate.
    pub trust: Trust,
    /// The name the certificate must carry. Required: there is no way to
    /// skip hostname verification.
    pub server_name: String,
    /// Bound on connecting (proxy, TCP, TLS, upgrade).
    pub timeout: Duration,
    /// Bound on waiting for each frame once connected (`None`: no bound).
    pub idle_timeout: Option<Duration>,
    /// The way out and the TLS context.
    pub way: Way,
    /// The largest frame and message that the session takes in.
    pub limits: Limits,
}

impl WsClientConfig {
    /// Refuses a path whose query string is anything but the relay's connect
    /// knobs: the token travels in a header, and a query string ends up in
    /// proxy and access logs. The path is not echoed, in case a token is what
    /// it carries.
    pub fn validate(&self) -> Result<(), String> {
        check_path(&self.endpoint.path, &self.server_name)
    }
}

/// The checks of [`WsClientConfig::validate`] on a path of the relay and the
/// name that its certificate must carry.
pub fn check_path(path: &str, server_name: &str) -> Result<(), String> {
    if let Some((_, query)) = path.split_once('?')
        && !relay_knobs_only(query)
    {
        return Err("the WebSocket path carries a query string other than the relay's connect \
             knobs; the token belongs in the X-Relay-Token header, and a query string \
             ends up in proxy access logs"
            .to_string());
    }
    if !path.starts_with('/') {
        return Err(format!("the WebSocket path must begin with '/': {path:?}"));
    }
    if path.contains(['\r', '\n', ' ']) {
        return Err("the WebSocket path contains whitespace".into());
    }
    if server_name.is_empty() {
        return Err("a server name is required: hostname verification has no bypass".into());
    }
    Ok(())
}

/// Whether `query` holds only the relay's documented connect knobs
/// (`family=4|6`, `dial=lazy`, `path=vpc|direct`, `precheck=<ms>`), none of
/// which can carry a secret.
pub fn relay_knobs_only(query: &str) -> bool {
    !query.is_empty()
        && query.split('&').all(|pair| match pair.split_once('=') {
            Some(("family", v)) => v == "4" || v == "6",
            Some(("dial", v)) => v == "lazy",
            Some(("path", v)) => v == "vpc" || v == "direct",
            Some(("precheck", v)) => !v.is_empty() && v.len() <= 7 && v.bytes().all(|b| b.is_ascii_digit()),
            _ => false,
        })
}

/// Why [`connect`] failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// The configuration or the trust store is unusable.
    Config(String),
    /// The TCP connection (direct or through the proxy) could not be opened.
    Dial(DialError),
    /// The TLS handshake failed: certificate, hostname or protocol, or the
    /// far end cut it, whose kind it keeps.
    Tls(pod_proxy::TlsFailure),
    /// A step did not finish in time.
    Timeout { step: &'static str, after: Duration },
    /// The relay answered the upgrade with an HTTP status instead of `101`.
    /// `body` is the start of the relay's explanation, e.g.
    /// `missing or wrong token` or `not in the ALLOW list`.
    Refused { status: u16, body: String },
    /// The relay answered `101` but not a valid WebSocket upgrade.
    Upgrade(String),
    /// A plain HTTPS request (such as minting a token) failed after TLS.
    Http(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::Config(why) => write!(f, "{why}"),
            ConnectError::Dial(e) => write!(f, "{e}"),
            ConnectError::Tls(why) => write!(f, "TLS with the relay failed: {why}"),
            ConnectError::Timeout { step, after } => {
                write!(f, "{step} timed out after {}s", after.as_secs())
            }
            ConnectError::Refused { status, body } if body.is_empty() => {
                write!(f, "the relay refused the session: HTTP {status}")
            }
            ConnectError::Refused { status, body } => {
                write!(f, "the relay refused the session: HTTP {status}: {body}")
            }
            ConnectError::Upgrade(why) => write!(f, "the WebSocket upgrade failed: {why}"),
            ConnectError::Http(why) => write!(f, "the HTTPS request failed: {why}"),
        }
    }
}

impl std::error::Error for ConnectError {}

impl ConnectError {
    /// How the connection ended, where an error of I/O or a bound ended it:
    /// the dial's, the handshake's, or the bound of a step.
    pub fn cut(&self) -> Option<Cut> {
        match self {
            ConnectError::Dial(e) => e.cut(),
            ConnectError::Tls(failure) => failure.cut,
            ConnectError::Timeout { .. } => Some(Cut::TimedOut),
            _ => None,
        }
    }

    /// The stage and the class of the failure.
    pub fn stage_and_class(&self) -> (Stage, Class) {
        match self {
            ConnectError::Config(_) => (Stage::WebSocket, Class::Config),
            ConnectError::Dial(e) => e.stage_and_class(),
            ConnectError::Tls(failure) if failure.cut.is_some() => (Stage::Network, Class::Unavailable),
            ConnectError::Tls(_) => (Stage::Tls, Class::Refused),
            ConnectError::Timeout { step, .. } if step.contains("TLS") => (Stage::Tls, Class::Timeout),
            ConnectError::Timeout { .. } => (Stage::WebSocket, Class::Timeout),
            ConnectError::Refused { status: 401 | 403 | 407, .. } => (Stage::WebSocket, Class::Refused),
            ConnectError::Refused { status: 404 | 410, .. } => (Stage::WebSocket, Class::NotFound),
            ConnectError::Refused { status: 429 | 503, .. } => (Stage::WebSocket, Class::TempFail),
            ConnectError::Refused { .. } => (Stage::WebSocket, Class::Unavailable),
            ConnectError::Upgrade(_) => (Stage::WebSocket, Class::Protocol),
            ConnectError::Http(_) => (Stage::Connect, Class::Unavailable),
        }
    }
}

impl From<ConnectError> for Failure {
    fn from(e: ConnectError) -> Failure {
        let (stage, class) = e.stage_and_class();
        Failure::caused(stage, class, e.to_string(), e)
    }
}

/// Whether the body of a refused upgrade is the relay's wording for a policy
/// refusal (spec: "not in the ALLOW list"), which a fresh token does not
/// repair. The one definition, which the client of the relay uses.
pub fn is_policy_refusal(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    body.contains("allow list") || body.contains("not allowed") || body.contains("blocked")
}

/// Connect, verify the relay's certificate and hostname, and upgrade.
///
/// The token goes into the `X-Relay-Token` header and nowhere else; it is not
/// stored, formatted or logged.
pub async fn connect(config: &WsClientConfig, token: &str) -> Result<RelaySession, ConnectError> {
    config.validate().map_err(ConnectError::Config)?;
    if token.contains(['\r', '\n']) {
        return Err(ConnectError::Config("the relay token contains a line break".into()));
    }
    let at = &config.endpoint;
    let way = &config.way;
    let (tls, peer) = way.open_tls(&at.host, at.port, &config.server_name, &config.trust, config.timeout, &[]).await?;
    let host_header = authority(&at.host, at.port);
    let (tls, pending) = tokio::time::timeout(config.timeout, upgrade(tls, &host_header, &at.path, token))
        .await
        .map_err(|_| ConnectError::Timeout { step: "the WebSocket upgrade", after: config.timeout })??;
    Ok(RelaySession::over_tls(tls, peer, pending, config.idle_timeout, WRITE_TIMEOUT).with_limits(config.limits))
}

/// TCP through `way` and a verified TLS handshake.
pub async fn open_tls(
    way: &Way,
    host: &str,
    port: u16,
    server_name: &str,
    trust: &Trust,
    timeout: Duration,
) -> Result<RelayStream, ConnectError> {
    way.open_tls(host, port, server_name, trust, timeout, &[]).await.map(|(tls, _)| tls)
}

/// Send the upgrade request and read the answer. On `101`, returns the stream
/// and any bytes that arrived after the response head (the relay dials the
/// target first, so its first frame can share a segment with the headers). On
/// any other status, reads a little of the body for the error message.
pub async fn upgrade<S>(tls: S, host_header: &str, path: &str, token: &str) -> Result<(S, Vec<u8>), ConnectError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let key = handshake::generate_key().map_err(|e| ConnectError::Upgrade(e.to_string()))?;
    upgrade_with(tls, &handshake::build_request(host_header, path, token, &key), &key).await
}

/// [`upgrade`] with a request that the caller built with `key`, as another
/// road's upgrade needs. The `Location` of a redirect comes back as
/// the body of the refusal, without its query, which a login page may fill.
pub async fn upgrade_with<S>(mut tls: S, request: &[u8], key: &str) -> Result<(S, Vec<u8>), ConnectError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let io = |e: std::io::Error| ConnectError::Upgrade(e.to_string());
    tls.write_all(request).await.map_err(io)?;
    tls.flush().await.map_err(io)?;

    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
        if buf.len() > UPGRADE_HEAD {
            return Err(ConnectError::Upgrade("no end of the response headers within 16 KiB".into()));
        }
        let n = tls.read(&mut chunk).await.map_err(io)?;
        if n == 0 {
            return Err(ConnectError::Upgrade("the relay closed the connection before answering the upgrade".into()));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let (status, reason, headers) = http::parse_head(&head).map_err(ConnectError::Upgrade)?;
    if status == 101 {
        handshake::check_response(&head, key).map_err(|e| ConnectError::Upgrade(e.to_string()))?;
        return Ok((tls, buf.split_off(head_end)));
    }
    let location = headers.iter().find(|(name, _)| name == "location").map(|(_, to)| to);
    if let Some(to) = location.filter(|_| (300..400).contains(&status)) {
        let to = to.split(['?', '#']).next().unwrap_or_default();
        return Err(ConnectError::Refused { status, body: format!("a redirect to {}", pod_core::text::one_line(to)) });
    }
    // Not upgraded: the body says why. A short, bounded read; on any trouble
    // the status line alone is reported.
    let body = tokio::time::timeout(Duration::from_secs(3), http::read_response(&mut tls, buf, 4096))
        .await
        .ok()
        .and_then(Result::ok)
        .map(|r| r.body_text(200))
        .filter(|b| !b.is_empty())
        .unwrap_or(reason);
    Err(ConnectError::Refused { status, body })
}

/// POST a JSON body over HTTPS and return the response, through the same
/// way out and the same verified path as [`connect`]. Used to mint relay
/// tokens.
pub async fn https_post_json(
    way: &Way,
    host: &str,
    port: u16,
    path: &str,
    body: &[u8],
    trust: &Trust,
    timeout: Duration,
) -> Result<http::Response, ConnectError> {
    let headers = [("Content-Type", "application/json"), ("Accept", "application/json")];
    https_request(way, "POST", host, port, path, &headers, body, 64 * 1024, trust, timeout).await
}

/// A request over HTTPS with a relay token in its `X-Relay-Token` header, as
/// the pairing calls need (`/v1/status/<name>`, `/v1/stop/<name>`), through
/// the same way out and verified path as [`connect`]. The token goes into the
/// header and nowhere else: no error of this path quotes a header.
#[allow(clippy::too_many_arguments)]
pub async fn https_with_token(
    way: &Way,
    method: &str,
    host: &str,
    port: u16,
    path: &str,
    body: &[u8],
    token: &str,
    max_body: usize,
    trust: &Trust,
    timeout: Duration,
) -> Result<http::Response, ConnectError> {
    if token.contains(['\r', '\n']) {
        return Err(ConnectError::Config("the relay token contains a line break".into()));
    }
    let headers =
        [("Content-Type", "application/json"), ("Accept", "application/json"), (handshake::TOKEN_HEADER, token)];
    https_request(way, method, host, port, path, &headers, body, max_body, trust, timeout).await
}

#[allow(clippy::too_many_arguments)]
/// The `Host` of an HTTPS request: an IPv6 address in brackets, on 443 too.
pub(crate) fn https_host(host: &str, port: u16) -> String {
    pod_proxy::host::host_header(host, port)
}

async fn https_request(
    way: &Way,
    method: &str,
    host: &str,
    port: u16,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    max_body: usize,
    trust: &Trust,
    timeout: Duration,
) -> Result<http::Response, ConnectError> {
    let mut tls = open_tls(way, host, port, host, trust, timeout).await?;
    let host_header = https_host(host, port);
    let ua = &way.user_agent;
    tokio::time::timeout(timeout, http::exchange(&mut tls, ua, method, &host_header, path, headers, body, max_body))
        .await
        .map_err(|_| ConnectError::Timeout { step: "the HTTPS request", after: timeout })?
        .map_err(|e| ConnectError::Http(e.to_string()))
}

/// Read one frame from a stream that is both the read and the write side,
/// answering Pings on it. The session uses split halves instead; this
/// single-stream form is kept for tests that script the peer's bytes.
///
/// A control frame that RFC 6455 section 5.5 forbids (fragmented, or over 125
/// bytes) is refused by the decoder: the read fails after a Close 1002, as
/// the session's does, and nothing is echoed. No Pong is sent once a Close
/// has been received.
pub async fn read_frame_over<S>(
    stream: &mut S,
    pending: &mut Vec<u8>,
    close_received: &mut bool,
    timeout: Duration,
) -> Result<Frame, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut chunk = [0u8; 4096];
    loop {
        let event = match next_event(pending) {
            Ok(event) => event,
            Err(e) => {
                let _ =
                    write_frame_over(stream, frame::OPCODE_CLOSE, &crate::session::close_payload(Some(1002), "")).await;
                return Err(format!("{e}"));
            }
        };
        match event {
            Some(Event::Pong(payload)) => {
                if payload.len() <= frame::MAX_CONTROL_PAYLOAD && !*close_received {
                    write_frame_over(stream, frame::OPCODE_PONG, &payload).await?;
                }
            }
            Some(Event::Frame(frame)) => {
                if frame.opcode == frame::OPCODE_CLOSE {
                    *close_received = true;
                }
                return Ok(frame);
            }
            None => {
                let n = tokio::time::timeout(timeout, stream.read(&mut chunk))
                    .await
                    .map_err(|_| "reading a WebSocket frame timed out".to_string())?
                    .map_err(|e| format!("{e}"))?;
                if n == 0 {
                    return Err("the relay closed the connection".into());
                }
                pending.extend_from_slice(&chunk[..n]);
            }
        }
    }
}

/// What the reader found in its buffer. A Ping is an event to answer, not a
/// frame to return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Frame(Frame),
    Pong(Vec<u8>),
}

/// Decode at most one frame from the front of `buf`, leaving exactly the bytes
/// not yet consumed (one read can carry several frames).
pub fn next_event(buf: &mut Vec<u8>) -> Result<Option<Event>, WsError> {
    next_event_capped(buf, frame::FORWARD_MAX_FRAME)
}

/// [`next_event`], with `cap` on a received frame: a server's frame,
/// as a client reads it.
pub fn next_event_capped(buf: &mut Vec<u8>, cap: usize) -> Result<Option<Event>, WsError> {
    crate::wire::next_event_from(buf, frame::Role::Server, cap)
}

/// Write one frame of a client's, masked with a fresh key (RFC 6455 §5.3
/// requires a new key per frame). Client frames are always masked; there is
/// no unmasked path for a client ([`crate::wire::write_frame_as`]).
pub(crate) async fn write_frame_over<S>(
    stream: &mut S,
    opcode: u8,
    payload: &[u8],
) -> Result<(), crate::error::SessionError>
where
    S: tokio::io::AsyncWrite + Unpin,
{
    crate::wire::write_frame_as(stream, frame::Role::Client, false, opcode, payload).await
}

/// What can be checked without a token: the trust store. The TLS handshake
/// is reported as not attempted (`????`), never as passing, because a
/// session needs a minted token. The check of the crypto provider is the
/// binary's, which chose it.
pub async fn doctor(config: &WsClientConfig) -> Vec<(String, Verdict)> {
    let mut out = Vec::new();
    match config.way.tls.roots_for(&config.trust) {
        Err(_) if matches!(config.trust, Trust::Caller(_)) => out.push((
            "trust store".to_string(),
            Verdict::Unknown {
                why: "the caller's own TLS configuration: its roots and its verifier are the \
                      caller's, and they cannot be checked here"
                    .into(),
            },
        )),
        Ok(r) => out.push((
            "trust store".to_string(),
            Verdict::Ok { detail: format!("{} trust anchors: {}", r.count, r.source) },
        )),
        Err(e) => {
            out.push(("trust store".to_string(), Verdict::Failed { detail: WsError::from(e).to_string() }));
            out.push((
                "TLS handshake".to_string(),
                Verdict::Unknown {
                    why: "not attempted: the trust store could not be loaded, so there is no \
                          anchor to verify a chain against"
                        .into(),
                },
            ));
            return out;
        }
    }
    out.push((
        "TLS handshake".to_string(),
        Verdict::Unknown { why: "not attempted here: a session needs a minted relay token".into() },
    ));
    out
}
