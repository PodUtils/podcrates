//! A stream to a target through a Shadowsocks server: TCP to the server,
//! through the caller's way out as each road dials; the salt and
//! the target's address in the first chunk (the salt after a key's prefix,
//! when it gives one); then, where the target speaks
//! first (an SSH server), the server's salt and its first chunk within a
//! limit. That answer tells a server that took the key from one that did
//! not: a server is silent to a stream that it cannot open, and a proxy
//! that waits for TLS on port 443 carries nothing and says nothing either,
//! so silence names both. A server of a key that names a WebSocket is
//! reached in it (`super::ws`), and the stream is the same inside.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::Instant;

use pod_core::{BoxFuture, Class, Connected, Failure, Stage, Target};
use pod_proxy::DialError;
use pod_tls::Trust;
use pod_ws::Way;

use super::codec::{self, ChunkError, Decoder, Encoder};
use super::{Carried, Node, carry};

/// How long the server's first chunk may take, after the salt and the
/// target went: the SSH server behind it speaks at once.
pub const ANSWER_WAIT: Duration = Duration::from_secs(10);

/// What the dial of a server needs, beside the server and the target.
#[derive(Debug, Clone)]
pub struct Dial {
    /// The way out: the dialer, the TLS context, the user agent.
    pub way: Way,
    /// The trust of a WebSocket's TLS, for a server in one.
    pub trust: Trust,
    /// Bound on the TCP dial, and on the send of the first chunk.
    pub timeout: Duration,
    /// How long to wait for the server's first chunk; `None` waits for
    /// nothing.
    pub answer: Option<Duration>,
    /// Whether a server that is silent through the whole wait is taken:
    /// a target whose client speaks first (a pipe's) sends nothing until
    /// it hears. A close, a reset or a chunk that does not open still
    /// fails.
    pub silence_is_ready: bool,
}

/// A stream that the server carries.
#[derive(Debug)]
pub struct Opened {
    pub carried: Carried,
    /// From the dial to the server's first chunk, when one came within the
    /// wait.
    pub answered_in: Option<Duration>,
}

/// Why a server carried no stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// The target cannot go in an address header; no server takes it.
    Target(String),
    /// The TCP connection to the server, directly or through the proxy.
    Dial(DialError),
    /// The WebSocket to a server in one: TLS, a refused upgrade or a wrong
    /// path, in its words, which show no path.
    WebSocket(String),
    /// Sending the salt and the target failed.
    Send(String),
    /// Nothing came back: through the wait (`ended` is `None`), or before
    /// a close or a reset (`ended` says which). `proxy` is the proxy that
    /// carried the connection, if one did.
    Silent { ended: Option<String>, after: Duration, proxy: Option<String> },
    /// Bytes came, and the server's first chunk did not open.
    Chunk(ChunkError),
    /// The server's answer ended inside its salt or its first chunk.
    Cut(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.line("the Shadowsocks server", "the target"))
    }
}

impl std::error::Error for OpenError {}

impl From<OpenError> for Failure {
    fn from(e: OpenError) -> Failure {
        let (stage, class) = e.stage_and_class();
        Failure::caused(stage, class, e.to_string(), e)
    }
}

impl OpenError {
    /// The stage and the class of the failure. The server is the relay of
    /// the road: it is silent to a stream that it cannot open, and an end
    /// with no answer is its refusal of the key.
    pub fn stage_and_class(&self) -> (Stage, Class) {
        match self {
            OpenError::Target(_) => (Stage::Connect, Class::Usage),
            OpenError::Dial(e) => e.stage_and_class(),
            OpenError::WebSocket(_) => (Stage::WebSocket, Class::Unavailable),
            OpenError::Send(_) | OpenError::Cut(_) => (Stage::Relay, Class::Unavailable),
            OpenError::Silent { ended: None, .. } => (Stage::Relay, Class::Timeout),
            OpenError::Silent { ended: Some(_), .. } => (Stage::Relay, Class::Refused),
            OpenError::Chunk(_) => (Stage::Relay, Class::Protocol),
        }
    }

    /// The line that says it, the server named as `shown` and the target as
    /// `target`. Never a password.
    pub fn line(&self, shown: &str, target: &str) -> String {
        match self {
            OpenError::Target(why) => format!("{target} cannot go in a Shadowsocks address: {why}"),
            OpenError::Dial(e) => format!("the Shadowsocks server {shown}: {e}"),
            OpenError::WebSocket(why) => format!("the Shadowsocks server {shown}: {why}"),
            OpenError::Send(why) => {
                format!("the Shadowsocks server {shown}: sending the salt and {target} failed: {why}")
            }
            OpenError::Silent { ended, after, proxy } => {
                let what = match ended {
                    None => format!("no answer within {} of the salt and {target}", seconds(*after)),
                    Some(how) => format!("{how}, with no answer to the salt and {target}"),
                };
                let cause = match proxy {
                    Some(proxy) => format!(
                        "the proxy {proxy} took the CONNECT, and no byte of the Shadowsocks server came back: it may \
                         expect TLS and carry no Shadowsocks; else the password or the method is not the server's, \
                         or the server cannot reach {target}"
                    ),
                    None => format!(
                        "the password or the method may not be the server's, as a server is silent to a stream that \
                         it cannot open, or the server cannot reach {target}"
                    ),
                };
                format!("the Shadowsocks server {shown}: {what}: {cause}")
            }
            OpenError::Chunk(
                e @ (ChunkError::Header | ChunkError::Kind(_) | ChunkError::Clock { .. } | ChunkError::Salt),
            ) => format!("the Shadowsocks server {shown}: its answer was refused: {e}"),
            OpenError::Chunk(e) => format!("the Shadowsocks server {shown}: its first chunk did not open: {e}"),
            OpenError::Cut(why) => format!("the Shadowsocks server {shown}: {why}"),
        }
    }
}

/// `10 s`, or `0.4 s` for a wait of a part of a second.
fn seconds(wait: Duration) -> String {
    match wait.subsec_millis() {
        0 => format!("{} s", wait.as_secs()),
        _ => format!("{:.1} s", wait.as_secs_f64()),
    }
}

/// A stream to `host:port` through `node`.
pub async fn open(node: &Node, host: &str, port: u16, dial: &Dial) -> Result<Opened, OpenError> {
    let header = codec::address(host, port).map_err(OpenError::Target)?;
    // A proxy is named in a silence's words only for raw TCP: a WebSocket's
    // upgrade proved that the bytes pass. The way's dialer names it.
    let proxy = match &node.ws {
        Some(_) => None,
        None => dial.way.dial.proxy_label(&Target::new(&node.host, node.port)),
    };
    let started = Instant::now();
    let mut tcp: Box<dyn Io> = match &node.ws {
        Some(url) => {
            let carried = super::ws::open(url, &dial.way, &dial.trust, dial.timeout).await;
            Box::new(carried.map_err(OpenError::WebSocket)?)
        }
        None => Box::new(dial.way.tcp(&node.host, node.port, dial.timeout).await.map_err(OpenError::Dial)?.io),
    };
    let master = node.master_key();
    let salt = super::prefix::salt(node.method, node.prefix().unwrap_or_default());
    let encoder = salt.and_then(|salt| Encoder::with_salt(node.method, &master, &salt));
    let mut encoder = encoder.map_err(|e| OpenError::Send(e.to_string()))?;
    let mut first = Vec::new();
    // The address in the first chunk, or in SIP022's headers.
    encoder.first(&header, &mut first).map_err(|e| OpenError::Send(e.to_string()))?;
    let sent = async {
        tcp.write_all(&first).await?;
        tcp.flush().await
    };
    match tokio::time::timeout(dial.timeout, sent).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(OpenError::Send(e.to_string())),
        Err(_) => return Err(OpenError::Send(format!("no progress in {} s", dial.timeout.as_secs()))),
    }
    // A response of SIP022 names this request's salt.
    let mut decoder = Decoder::answer(node.method, &master, encoder.request_salt());
    let (early, answered_in) = match dial.answer {
        Some(wait) => match first_chunk(&mut tcp, &mut decoder, wait, dial.silence_is_ready).await {
            Ok(Some(plain)) => (plain, Some(started.elapsed())),
            Ok(None) => (Vec::new(), None),
            Err(Waited::Silent(ended)) => return Err(OpenError::Silent { ended, after: wait, proxy }),
            Err(Waited::Chunk(e)) => return Err(OpenError::Chunk(e)),
            Err(Waited::Cut(why)) => return Err(OpenError::Cut(why)),
        },
        None => (Vec::new(), None),
    };
    Ok(Opened { carried: carry::spawn(tcp, encoder, decoder, early), answered_in })
}

/// A Shadowsocks server as a dialer: each dial is a stream to its target.
#[derive(Debug, Clone)]
pub struct Road {
    pub node: Node,
    pub dial: Dial,
}

impl pod_core::Dial for Road {
    fn label(&self) -> String {
        format!("Shadowsocks {}", self.node.shown())
    }

    fn proxy_label(&self, _target: &Target) -> Option<String> {
        Some(self.node.shown())
    }

    /// A stream to `target`, open before `deadline`.
    fn dial<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let left = crate::road::left(deadline, target)?;
            let dial = Dial { timeout: self.dial.timeout.min(left), ..self.dial.clone() };
            let shown = self.node.shown();
            let host = target.host();
            let opening = open(&self.node, &host, target.port(), &dial);
            let opened = tokio::time::timeout_at(deadline, opening)
                .await
                .map_err(|_| crate::road::late(target, &shown))?
                .map_err(|e| {
                    let (stage, class) = e.stage_and_class();
                    Failure::caused(stage, class, e.line(&shown, &target.to_string()), e)
                })?;
            Ok(Connected::new(Box::new(opened.carried.stream)))
        })
    }

    fn names_inside(&self) -> bool {
        true
    }
}

/// The server's connection: TCP, or the bytes of a WebSocket.
trait Io: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// How the wait for the first chunk failed.
enum Waited {
    Silent(Option<String>),
    Chunk(ChunkError),
    Cut(String),
}

/// The plaintext of the server's first chunk within `wait`; `None` when it
/// was silent and `silence_is_ready`.
async fn first_chunk<S: AsyncRead + Unpin>(
    stream: &mut S,
    decoder: &mut Decoder,
    wait: Duration,
    silence_is_ready: bool,
) -> Result<Option<Vec<u8>>, Waited> {
    let deadline = Instant::now() + wait;
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        if let Some(plain) = decoder.chunk().map_err(Waited::Chunk)? {
            return Ok(Some(plain));
        }
        let read = tokio::time::timeout_at(deadline, stream.read(&mut buf)).await;
        let untouched = decoder.untouched();
        match read {
            Err(_) if untouched && silence_is_ready => return Ok(None),
            Err(_) if untouched => return Err(Waited::Silent(None)),
            Err(_) => {
                return Err(Waited::Cut(format!("its answer stopped inside its first chunk ({})", seconds(wait))));
            }
            Ok(Ok(0)) if untouched => return Err(Waited::Silent(Some("it closed the connection".into()))),
            Ok(Ok(0)) => return Err(Waited::Cut("its answer ended inside its first chunk".into())),
            Ok(Ok(n)) => decoder.feed(&buf[..n]),
            Ok(Err(e)) if untouched => return Err(Waited::Silent(Some(format!("the connection failed ({e})")))),
            Ok(Err(e)) => return Err(Waited::Cut(format!("the connection failed inside its first chunk: {e}"))),
        }
    }
}
