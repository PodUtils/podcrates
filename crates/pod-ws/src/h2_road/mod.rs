//! The WebSocket over HTTP/2's extended CONNECT (RFC 8441): the sessions of
//! one process to a relay host as streams of one TLS connection, where the
//! host offers it, as podssh's relay role does. TLS of this road alone
//! offers `h2`, then `http/1.1`, by ALPN; each other road offers none. On
//! `h2`: HTTP/2's handshake with the
//! road's windows, a PING whose answer comes after the server's SETTINGS,
//! and then, only where those SETTINGS offered the protocol
//! (`SETTINGS_ENABLE_CONNECT_PROTOCOL`, RFC 8441 section 3), a CONNECT for
//! each session with `:protocol websocket`, the `:scheme`, `:path` and
//! `:authority` of the upgrade, `sec-websocket-version` and the token, in a
//! field that HPACK never indexes (section 4). A 2xx opens the session,
//! whose frames of RFC 6455 run on the stream as on TCP (section 5); another
//! status is the relay's refusal, read as a refused upgrade is.
//!
//! Where TLS chose `http/1.1`, or no protocol, the upgrade goes on that same
//! connection, as [`crate::connect`] sends it. Where it chose `h2` and the
//! SETTINGS offered no extended CONNECT, or HTTP/2 did not carry the
//! CONNECT, the connection is closed first and the upgrade goes on a new
//! one, so that a session holds one connection at a time; the [`Pool`] then
//! takes HTTP/1.1 to that host for [`REMEMBERED`]. A link that replaces
//! another link of its session ([`connect_link`]) takes no connection that a
//! link of the session rode: a connection that went silent holds a new
//! stream as it holds the old one, until its TCP recovers. A CONNECT with no
//! answer within [`ANSWER_WAIT`] asks its connection by a PING; with no
//! answer within [`PING_WAIT`], the pool retires the connection and the
//! CONNECT goes once to a new one, so that no new session waits on a silent
//! connection for the whole timeout. A caller's own TLS
//! configuration is used as it is, with no ALPN (`Trust::caller`), so it
//! takes the upgrade alone. The caller owns the pool: one for each process,
//! or one for each test.

mod dial;
mod duplex;
mod fallback;
mod pool;
pub mod shape;

use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use h2::{RecvStream, SendStream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use pod_tls::Trust;

use crate::client::{ConnectError, RelayStream, WRITE_TIMEOUT, WsClientConfig};
use crate::headers::Extra;
use crate::session::RelaySession;

pub use dial::{Offered, probe};
pub use fallback::{Fallback, Road};
pub use pool::Pool;
pub use shape::{Padding, Priority, Setting, Shape};

/// The protocols that this road offers by ALPN, in its order.
pub const H2: &[u8] = b"h2";
pub const HTTP1: &[u8] = b"http/1.1";

/// The most sessions on one connection: the connection's window is theirs
/// together, so that no stream that a slow reader holds full takes the
/// window of another; a session past them opens another connection.
pub const STREAMS: usize = 16;

/// The window of each session's stream: 160 Mbit/s at 50 ms, four times the
/// relay's largest frame.
pub const STREAM_WINDOW: u32 = 1 << 20;

/// The connection's window: each session's window, together.
pub const CONNECTION_WINDOW: u32 = STREAM_WINDOW * STREAMS as u32;

/// How long a connection with no session stays open, for the next session
/// of a fan-out.
pub const IDLE: Duration = Duration::from_secs(30);

/// How long a host that took HTTP/1.1 at the cost of a connection is not
/// asked for HTTP/2 again.
pub const REMEMBERED: Duration = Duration::from_secs(600);

/// How long a CONNECT waits for its answer before its connection is asked
/// by a PING whether it still speaks: a relay that dials its target answers
/// late on a live connection; a silent connection answers neither.
pub const ANSWER_WAIT: Duration = Duration::from_secs(2);

/// The least wait for the answer to that PING; four round trips of the
/// connection's first PING, where longer. No answer retires the connection,
/// and the CONNECT goes once to a new one.
pub const PING_WAIT: Duration = Duration::from_secs(2);

/// How long, and how much of, a refusal's body is read, as of a refused
/// upgrade.
const REFUSAL_WAIT: Duration = Duration::from_secs(3);
const REFUSAL_BODY: usize = 4096;

/// How long a CONNECT's 4xx is given to say why before the upgrade of
/// HTTP/1.1 takes the session: the fallback is chosen inside a second, not
/// left to the CONNECT's own wait.
const REFUSAL_4XX_WAIT: Duration = Duration::from_millis(500);

/// A session's stream of HTTP/2: the DATA of one stream both ways, and
/// END_STREAM at its shutdown; a client's gives its place on its
/// connection back when it drops.
pub struct Stream {
    inner: duplex::Stream,
    _place: Option<pool::Place>,
}

/// The stream of `send` and `recv`, as a server answers an extended
/// CONNECT with them (a relay role of HTTP/2).
pub fn duplex(send: SendStream<Bytes>, recv: RecvStream) -> Stream {
    Stream { inner: duplex::Stream::from_parts(send, recv), _place: None }
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Why a CONNECT on a connection gave no session.
enum Carry {
    /// The relay answered, or the step's bound passed: the session's end.
    Answer(ConnectError),
    /// The connection's own end (a GOAWAY, its I/O, a stream refused before
    /// it was processed, RFC 9113 section 8.7): another connection may pass.
    Connection(String),
    /// The server reset this CONNECT: HTTP/1.1 carries the session.
    Reset(String),
    /// The server answered 4xx: a proxy that refuses the extended CONNECT,
    /// so HTTP/1.1 carries the session on a new connection.
    Refused { status: u16 },
}

impl Carry {
    fn of(e: h2::Error) -> Carry {
        let refused = e.is_reset() && e.reason() == Some(h2::Reason::REFUSED_STREAM);
        if e.is_go_away() || e.is_io() || refused || !e.is_reset() {
            return Carry::Connection(e.to_string());
        }
        Carry::Reset(e.to_string())
    }
}

/// The connections of [`Pool`] that the links of one session rode: a link
/// of [`connect_link`] takes only a connection newer than each of them.
/// Each link that replaces another (a spare beside a silent link, a resume
/// after a loss, a move) thus takes no connection of the link that it
/// replaces; the first link of a session shares a connection kept.
#[derive(Debug, Default)]
pub struct Links {
    /// The serial number of the newest connection that a link rode; 0 for
    /// none.
    newest: AtomicU64,
}

impl Links {
    /// The links of a new session: none yet.
    pub fn new() -> Links {
        Links::default()
    }
}

/// Connect as [`crate::connect`] does, with `token` in a field and nowhere
/// else, on this road where the host takes it, else by the upgrade of
/// HTTP/1.1: the session, and how it is carried. `pool` keeps the
/// connections and what it found of each host.
pub async fn connect(pool: &Pool, config: &WsClientConfig, token: &str) -> Result<(RelaySession, Road), ConnectError> {
    connect_link(pool, &Links::new(), config, token).await
}

/// [`connect`] with the shape of its HTTP/2: the settings and the
/// connection's window of [`Shape::browser`], say. The pool keeps the
/// connections that it makes either way.
pub async fn connect_with(
    pool: &Pool,
    config: &WsClientConfig,
    token: &str,
    shape: &Shape,
) -> Result<(RelaySession, Road), ConnectError> {
    connect_link_with(pool, &Links::new(), config, token, shape).await
}

/// [`connect`] for a link of the session of `links`: on a connection newer
/// than each that a link of the session rode, which `links` then holds.
pub async fn connect_link(
    pool: &Pool,
    links: &Links,
    config: &WsClientConfig,
    token: &str,
) -> Result<(RelaySession, Road), ConnectError> {
    connect_link_with(pool, links, config, token, &Shape::existing()).await
}

/// [`connect_link`] with the shape of a connection that it makes.
pub async fn connect_link_with(
    pool: &Pool,
    links: &Links,
    config: &WsClientConfig,
    token: &str,
    shape: &Shape,
) -> Result<(RelaySession, Road), ConnectError> {
    config.validate().map_err(ConnectError::Config)?;
    if token.contains(['\r', '\n']) {
        return Err(ConnectError::Config("the relay token contains a line break".into()));
    }
    // Checked before anything connects.
    let extra = config.way.extra(&config.endpoint.host)?;
    if matches!(config.trust, Trust::Caller(_)) {
        return Ok((crate::connect(config, token).await?, Road::Http1(Fallback::Caller)));
    }
    // A connection that ended under the CONNECT, as at a GOAWAY, gives way
    // to a new one, once; the last one's words go into the fallback's.
    let mut ended = String::new();
    for _ in 0..2 {
        let after = links.newest.load(Ordering::SeqCst);
        let (conn, place, shared) = match pool.place(config, after, shape).await? {
            pool::Got::Place { conn, place, shared } => (conn, place, shared),
            pool::Got::Http1(tls, peer) => {
                return Ok((upgrade_on(*tls, peer, config, token, &extra).await?, Road::Http1(Fallback::NotChosen)));
            }
            pool::Got::Fallback(why) => return Ok((crate::connect(config, token).await?, Road::Http1(why))),
        };
        match open_on(&conn, place, config, token, &extra).await {
            Ok(session) => {
                links.newest.fetch_max(conn.serial(), Ordering::SeqCst);
                return Ok((session, Road::Stream { shared }));
            }
            Err(Carry::Answer(e)) => {
                // A connection that answered its PING but not the CONNECT
                // carries no next session either.
                if matches!(e, ConnectError::Timeout { .. }) {
                    conn.retire();
                }
                return Err(e);
            }
            Err(Carry::Connection(why)) => {
                ended = why;
                conn.retire();
            }
            Err(Carry::Reset(why)) => {
                let why = Fallback::Carried(why);
                pool.remember(config, why.clone()).await;
                return Ok((crate::connect(config, token).await?, Road::Http1(why)));
            }
            Err(Carry::Refused { status }) => {
                let why = Fallback::Refused(status);
                // A refusal of this session's credential is not a property of
                // the host: only a refusal of the method is remembered.
                if !matches!(status, 401 | 403 | 407) {
                    pool.remember(config, why.clone()).await;
                }
                return Ok((crate::connect(config, token).await?, Road::Http1(why)));
            }
        }
    }
    let why = Fallback::Carried(format!("two connections of HTTP/2 ended under the CONNECT, the last: {ended}"));
    Ok((crate::connect(config, token).await?, Road::Http1(why)))
}

/// The CONNECT of RFC 8441 for `config`'s path, with `token`, and the
/// headers of the caller in `extra` after it.
fn request(config: &WsClientConfig, token: &str, extra: &Extra) -> Result<http::Request<()>, ConnectError> {
    let authority = pod_proxy::authority(&config.endpoint.host, config.endpoint.port);
    let uri = http::Uri::builder()
        .scheme("https")
        .authority(authority.as_str())
        .path_and_query(config.endpoint.path.as_str())
        .build()
        .map_err(|e| ConnectError::Config(format!("the relay's name or path is no URI of HTTP/2: {e}")))?;
    // Never indexed: a shared connection's table of HPACK never holds it.
    let mut value = http::HeaderValue::from_str(token)
        .map_err(|_| ConnectError::Config("the relay token is no value of an HTTP field".into()))?;
    value.set_sensitive(true);
    let mut request = http::Request::builder()
        .method(http::Method::CONNECT)
        .uri(uri)
        .header("sec-websocket-version", "13")
        .header(http::header::USER_AGENT, config.way.user_agent.as_str())
        .header("x-relay-token", value);
    for (name, value) in extra.iter() {
        // Checked already (`Extra::checked`); never indexed, as the token.
        let name = http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| ConnectError::Config(format!("the caller's header {name} is no name of HTTP/2")))?;
        let mut value = http::HeaderValue::from_bytes(value.as_bytes()).map_err(|_| {
            ConnectError::Config(format!("the value of the caller's header {name} is no value of HTTP/2"))
        })?;
        value.set_sensitive(true);
        request = request.header(name, value);
    }
    let mut request = request.body(()).map_err(|e| ConnectError::Config(format!("the CONNECT: {e}")))?;
    request.extensions_mut().insert(h2::ext::Protocol::from_static("websocket"));
    Ok(request)
}

/// The session of a CONNECT on `conn`, in the place that the pool gave it.
async fn open_on(
    conn: &pool::Conn,
    place: pool::Place,
    config: &WsClientConfig,
    token: &str,
    extra: &Extra,
) -> Result<RelaySession, Carry> {
    let request = request(config, token, extra).map_err(Carry::Answer)?;
    let limit = config.timeout;
    let asked = async {
        let mut sender = conn.sender().ready().await.map_err(Carry::of)?;
        let (response, send) = sender.send_request(request, false).map_err(Carry::of)?;
        let response = response.await.map_err(Carry::of)?;
        Ok::<_, Carry>((response, send))
    };
    let late = |_| Carry::Answer(ConnectError::Timeout { step: "the WebSocket upgrade", after: limit });
    let start = tokio::time::Instant::now();
    tokio::pin!(asked);
    let (response, send) = match tokio::time::timeout(ANSWER_WAIT.min(limit), &mut asked).await {
        Ok(got) => got?,
        Err(_) => {
            let rest = limit.saturating_sub(start.elapsed());
            tokio::select! {
                got = &mut asked => got?,
                speaks = conn.answers(rest) => {
                    if !speaks {
                        let after = start.elapsed().as_millis();
                        return Err(Carry::Connection(format!("no answer to the CONNECT within {after} ms, nor to a PING")));
                    }
                    let rest = limit.saturating_sub(start.elapsed());
                    tokio::time::timeout(rest, asked).await.map_err(late)??
                }
            }
        }
    };
    let status = response.status();
    if status.is_client_error() {
        // A proxy that refuses the extended CONNECT answers 4xx, and the
        // upgrade of HTTP/1.1 is the way past it: the session goes there
        // within the second, not at the end of the CONNECT's wait.
        let _ = refusal_text_within(response.into_body(), REFUSAL_4XX_WAIT).await;
        return Err(Carry::Refused { status: status.as_u16() });
    }
    if !status.is_success() {
        let reason = status.canonical_reason().unwrap_or_default().to_string();
        let body = refusal_text(response.into_body()).await;
        let body = if body.is_empty() { reason } else { body };
        return Err(Carry::Answer(extra.masked(ConnectError::Refused { status: status.as_u16(), body })));
    }
    let stream = Stream { inner: duplex::Stream::from_parts(send, response.into_body()), _place: Some(place) };
    let session = RelaySession::over_h2(stream, conn.peer(), config.idle_timeout, WRITE_TIMEOUT);
    Ok(session.with_limits(config.limits))
}

/// The start of a refusal's body, as the body of a refused upgrade is
/// read: within [`REFUSAL_WAIT`], made one line for a terminal.
async fn refusal_text(body: RecvStream) -> String {
    refusal_text_within(body, REFUSAL_WAIT).await
}

/// [`refusal_text`] within `wait`: a 4xx gives the fallback no more than
/// [`REFUSAL_4XX_WAIT`] to say why.
async fn refusal_text_within(mut body: RecvStream, wait: Duration) -> String {
    let mut got = Vec::new();
    let read = async {
        while let Some(Ok(chunk)) = body.data().await {
            let _ = body.flow_control().release_capacity(chunk.len());
            got.extend_from_slice(&chunk);
            if got.len() >= REFUSAL_BODY {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(wait, read).await;
    got.truncate(REFUSAL_BODY);
    pod_proxy::http::Response { status: 0, reason: String::new(), headers: Vec::new(), body: got, url: String::new() }
        .body_text(200)
}

/// The upgrade of HTTP/1.1 on `tls`, a connection to `peer` whose TLS chose
/// it.
async fn upgrade_on(
    tls: RelayStream,
    peer: Option<std::net::SocketAddr>,
    config: &WsClientConfig,
    token: &str,
    extra: &Extra,
) -> Result<RelaySession, ConnectError> {
    let host_header = pod_proxy::authority(&config.endpoint.host, config.endpoint.port);
    let upgraded = crate::client::upgrade_carrying(tls, &host_header, &config.endpoint.path, token, extra);
    let (tls, pending) = tokio::time::timeout(config.timeout, upgraded)
        .await
        .map_err(|_| ConnectError::Timeout { step: "the WebSocket upgrade", after: config.timeout })??;
    let session = RelaySession::over_tls(tls, peer, pending, config.idle_timeout, WRITE_TIMEOUT);
    Ok(session.with_limits(config.limits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Endpoint;

    fn config(path: &str) -> WsClientConfig {
        WsClientConfig {
            endpoint: Endpoint { host: "relay.example".into(), port: 8443, path: path.into() },
            trust: Trust::Default,
            server_name: "relay.example".into(),
            timeout: Duration::from_secs(5),
            idle_timeout: None,
            way: crate::testing::way(),
            limits: crate::Limits::default(),
        }
    }

    /// RFC 8441 section 4: `:protocol`, a scheme, the authority and the
    /// path of the upgrade, its query among them; the token in a field that
    /// HPACK never indexes, and no key of RFC 6455.
    #[test]
    fn the_connect_names_websocket_the_path_and_hides_the_token_from_hpack() {
        let request = request(&config("/connect/github.com/22?family=6"), "tok.en", &Extra::default()).unwrap();
        assert_eq!(request.method(), http::Method::CONNECT);
        let protocol = request.extensions().get::<h2::ext::Protocol>().map(|p| p.as_str().to_string());
        assert_eq!(protocol.as_deref(), Some("websocket"));
        assert_eq!(request.uri().scheme_str(), Some("https"));
        assert_eq!(request.uri().authority().map(|a| a.as_str()), Some("relay.example:8443"));
        assert_eq!(request.uri().path_and_query().map(|p| p.as_str()), Some("/connect/github.com/22?family=6"));
        assert!(request.headers()["x-relay-token"].is_sensitive());
        assert_eq!(request.headers()["sec-websocket-version"], "13");
        assert!(request.headers().get("sec-websocket-key").is_none());
        let refused = request_err("bad\u{7f}token");
        assert!(!refused.contains("bad"), "the token is not said: {refused}");
    }

    fn request_err(token: &str) -> String {
        request(&config("/connect/h/22"), token, &Extra::default()).err().map(|e| e.to_string()).unwrap_or_default()
    }
}
