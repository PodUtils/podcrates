//! Raw streams carried by WebSockets: the bridge of a session and a raw
//! stream (TCP or Unix), both ways, and the client WebSocket of a URL that
//! a bridge dials. A quick tunnel proxies WebSocket upgrades, and that is
//! enough to carry any stream: the origin serves a WebSocket, each of its
//! sessions is bridged to a raw stream, and a client on the far side turns
//! local sockets into WebSockets (the idea of websocat and wstunnel).
//!
//! One WebSocket message maps to one write, so binary framing is kept and
//! interactive protocols (SSH, RDP, databases, raw sockets) work unchanged.
//! A Close of the far end other than 1000 goes to `tracing` with its code,
//! its reason and [`close_hint`].

use std::time::Duration;

use pod_core::{BoxedIo, Failure, Stage};
use pod_tls::Trust;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::client::{ConnectError, DEFAULT_TIMEOUT};
use crate::close::{close_code_and_reason, close_hint, code};
use crate::frame;
use crate::limits::Limits;
use crate::session::RelaySession;
use crate::url::{self, Asking, Stream, WsUrl};
use crate::way::Way;

/// The bound of each step (TCP, TLS, the upgrade) of a client WebSocket.
pub const DIAL_TIMEOUT: Duration = DEFAULT_TIMEOUT;

/// The size of one read of the raw stream, and so of one message.
const CHUNK: usize = 16 * 1024;

/// The counter of the bytes of a bridge: in from the WebSocket to the raw
/// stream, out from the raw stream to the WebSocket.
pub trait Tally: Send + Sync {
    fn add_in(&self, bytes: u64);
    fn add_out(&self, bytes: u64);
}

/// A counter that counts nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoTally;

impl Tally for NoTally {
    fn add_in(&self, _: u64) {}
    fn add_out(&self, _: u64) {}
}

/// A client WebSocket to `url` (`ws://` or `wss://`) through `way`, as a
/// bridge dials it: `Authorization` when `authorization` is given, TLS that
/// offers `http/1.1` alone (a WebSocket upgrade does not go over HTTP/2),
/// and the limits of tokio-tungstenite. The time of each step goes to
/// `tracing`.
pub async fn dial(
    url: &str,
    way: &Way,
    trust: &Trust,
    timeout: Duration,
    authorization: Option<&str>,
) -> Result<RelaySession<Stream>, ConnectError> {
    let parsed = WsUrl::parse(url).map_err(ConnectError::Config)?;
    let asking = Asking { authorization, alpn: &[b"http/1.1"], limits: Limits::TUNGSTENITE, ..Asking::default() };
    url::dial_asking(&parsed, &asking, way, trust, timeout, None).await
}

/// Connect a raw stream to the WebSocket at `url`: [`dial`] within
/// [`DIAL_TIMEOUT`], then [`bridge`].
pub async fn connect_bridge(
    url: &str,
    way: &Way,
    trust: &Trust,
    io: BoxedIo,
    authorization: Option<&str>,
    tally: &dyn Tally,
) -> Result<(), Failure> {
    let session = dial(url, way, trust, DIAL_TIMEOUT, authorization).await?;
    bridge(session, io, tally).await
}

/// Copy bytes both ways between a WebSocket session and a raw stream: each
/// data message to one write of `io`, each read of `io` to one binary
/// message. The end of `io` sends a Close 1000, and the bridge ends when the
/// far end's Close comes; a Close of the far end, or the end of the session,
/// shuts the write side of `io` and ends the bridge, also while `io` has
/// nothing to read. Pings are answered by the session.
pub async fn bridge<S>(session: RelaySession<S>, io: BoxedIo, tally: &dyn Tally) -> Result<(), Failure>
where
    S: AsyncRead + AsyncWrite + Send,
{
    let (mut io_r, mut io_w) = tokio::io::split(io);
    let session = &session;
    // The end of the way from the WebSocket ends the other way too: a raw
    // stream that the far side holds open gives no end of its own.
    let ws_ended = tokio::sync::Notify::new();

    let ws_to_io = async {
        let ended = loop {
            let message = match session.read_frame().await {
                Ok(message) => message,
                Err(e) => break Err(Failure::from(e)),
            };
            if message.opcode == frame::OPCODE_CLOSE {
                closed(&message.payload);
                break Ok(());
            }
            if let Err(e) = io_w.write_all(&message.payload).await {
                break Err(Failure::io(Stage::Connect, e));
            }
            tally.add_in(message.payload.len() as u64);
        };
        let _ = io_w.shutdown().await;
        ws_ended.notify_one();
        ended
    };

    let io_to_ws = async {
        let mut buf = vec![0u8; CHUNK];
        let ended = loop {
            let read = tokio::select! {
                read = io_r.read(&mut buf) => read,
                () = ws_ended.notified() => break Ok(()),
            };
            let n = match read {
                Ok(0) => break Ok(()),
                Ok(n) => n,
                Err(e) => break Err(Failure::io(Stage::Connect, e)),
            };
            tally.add_out(n as u64);
            // A session that ended (a Close came, or the link broke) takes
            // no more: the other way says why.
            if session.send_binary(&buf[..n]).await.is_err() {
                break Ok(());
            }
        };
        let _ = session.send_close(code::NORMAL, "").await;
        ended
    };

    let (to_io, to_ws) = tokio::join!(ws_to_io, io_to_ws);
    to_io?;
    to_ws
}

/// The code and the reason of the far end's Close, in the log: a warning
/// for a code other than 1000 (a relay at its limit: 1009).
fn closed(payload: &[u8]) {
    let (code, reason) = close_code_and_reason(payload);
    match code {
        Some(code) if code != code::NORMAL => {
            tracing::warn!(code, reason = %reason, "the far end closed the WebSocket{}", close_hint(code));
        }
        code => tracing::debug!(code, reason = %reason, "the WebSocket closed"),
    }
}

#[cfg(test)]
mod tests {
    use crate::handshake::accept_key;

    #[test]
    fn accept_key_matches_rfc_example() {
        // RFC 6455 section 1.3.
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }
}
