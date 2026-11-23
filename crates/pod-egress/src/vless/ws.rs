//! VLESS over a WebSocket (Xray-core, `transport/internet/websocket/`). The
//! upgrade goes to the link's path, which the server checks whole
//! (`hub.go`: another path is a 404), with the link's `host` in `Host`; then
//! each binary message is a piece of the stream, both ways (`connection.go`:
//! a write is one binary message, and the reader takes them in turn). The
//! server ends a session with a Close of 1000 (`connection.go`, `Close`),
//! the same when it refuses the ID: the answer's reader says which came.
//! No early data goes in `Sec-WebSocket-Protocol`: the request rides the
//! first message.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::Notify;

use super::link::Server;
use super::{Dial, VlessError};
use crate::coalesce;
use crate::end::{AtEnd, end_of_input};
use pod_ws::client::{self, ConnectError};
use pod_ws::session::RelaySession;
use pod_ws::{frame, handshake, url};

/// The pipe in memory, each way.
const PIPE: usize = 256 * 1024;
/// The largest message sent, as a CDN in front of the server may cut a
/// larger one, as on the forwarders' road.
const CHUNK: usize = frame::FORWARD_SEND_PAYLOAD;
/// The largest message taken: gorilla's writer sends what each write gave.
const RECEIVE_CAP: usize = 1024 * 1024;

/// The upgrade, then VLESS's request in the first binary message; the
/// stream is one end of a pipe that two tasks copy to and from the session.
/// The upgrade goes in one write of [`coalesce::TARGET`] bytes, padded where
/// the request is short. A frame cannot follow the head in that write: the
/// server of RFC 6455 that Xray-core uses closes a connection that carries
/// bytes after the handshake (gorilla/websocket v1.5.3, `server.go`: a
/// buffered reader with bytes in it is closed, "client sent data before
/// handshake is complete"), so the first application data goes in the first
/// message, as its own write.
pub(super) async fn open(
    server: &Server,
    first: Vec<u8>,
    dial: &Dial,
) -> Result<(DuplexStream, coalesce::Report), VlessError> {
    let key = handshake::generate_key().map_err(|e| VlessError::Carry(e.to_string()))?;
    let head = request(server, &key, &dial.way.user_agent);
    let mut opening = coalesce::First::new();
    let (frame, _) = opening.write(&head, &[]);
    let report = opening.report();
    let stream =
        connect(&server.host, server.port, &server.sni, server.tls, dial).await.map_err(|e| failed(e, &server.path))?;
    let upgraded = tokio::time::timeout(dial.timeout, client::upgrade_with(stream, &frame, &key))
        .await
        .map_err(|_| VlessError::Carry(format!("no answer to the upgrade within {} s", dial.timeout.as_secs())))?;
    let (stream, pending) = upgraded.map_err(|e| failed(e, &server.path))?;
    let idle = Some(pod_ws::DEFAULT_IDLE_TIMEOUT);
    let session = RelaySession::new(stream, pending, idle, client::WRITE_TIMEOUT).naming("the VLESS server");
    session.set_receive_cap(RECEIVE_CAP);
    session.send_binary(&first).await.map_err(|e| VlessError::Carry(format!("sending VLESS's request failed: {e}")))?;
    Ok((spawn(session, dial.at_end), report))
}

/// TCP to `host:port` through the way out of `dial`, and TLS for `sni` where
/// `tls` says so: the stream of an upgrade of the VLESS, Trojan and VMess
/// roads.
pub(crate) async fn connect(
    host: &str,
    port: u16,
    sni: &str,
    tls: bool,
    dial: &Dial,
) -> Result<url::Stream, ConnectError> {
    let way = &dial.way;
    if tls {
        let (tls, _) = way.open_tls(host, port, sni, &dial.trust, dial.timeout, &[]).await?;
        return Ok(url::Stream::Tls(Box::new(tls)));
    }
    Ok(url::Stream::Plain(way.tcp(host, port, dial.timeout).await.map_err(ConnectError::Dial)?.io))
}

/// The upgrade request: the link's path and `Host`, no subprotocol, no
/// extension and no `Origin`, as Xray-core's client sends it.
fn request(server: &Server, key: &str, user_agent: &str) -> Vec<u8> {
    format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n",
        server.path, server.authority, user_agent
    )
    .into_bytes()
}

/// A failure of the dial or the upgrade, in this road's words.
fn failed(e: ConnectError, path: &str) -> VlessError {
    let path = path.split('?').next().unwrap_or("/");
    match e {
        ConnectError::Dial(e) => VlessError::Dial(e),
        ConnectError::Refused { status, body } => {
            let said = if body.is_empty() { String::new() } else { format!(" ({body})") };
            let why = format!(
                "the WebSocket upgrade at {path}{said}; a path that it does not serve, or a proxy or a front end \
                 that does not pass the upgrade, answers so: type=xhttp passes where the upgrade does not"
            );
            VlessError::Refused { status, why }
        }
        ConnectError::Tls(why) => VlessError::Carry(format!("TLS: {why}")),
        ConnectError::Upgrade(why) => VlessError::Carry(format!("the WebSocket upgrade: {why}")),
        other => VlessError::Carry(other.to_string()),
    }
}

/// Two tasks between the session and one end of a pipe: the bytes out in
/// binary messages, the binary messages in as bytes. A text message is no
/// part of VLESS's stream and is passed over. The server's Close, or a
/// failure, ends the pipe's reading side, which the answer's reader sees.
pub(crate) fn spawn<S>(session: RelaySession<S>, at: AtEnd) -> DuplexStream
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + 'static,
{
    let (ours, theirs) = tokio::io::duplex(PIPE);
    let session = Arc::new(session);
    let (mut from_local, mut to_local) = tokio::io::split(theirs);
    // Each task's end stops the other: a write of the reader's could wait
    // on a pipe that only the sending task reads.
    let up_failed = Arc::new(Notify::new());
    let down_ended = Arc::new(Notify::new());
    let (up_session, up_failed_tx, down_ended_rx) = (session.clone(), up_failed.clone(), down_ended.clone());
    tokio::spawn(async move {
        let mut buf = vec![0u8; CHUNK];
        loop {
            let read = tokio::select! {
                read = from_local.read(&mut buf) => read,
                () = down_ended_rx.notified() => return,
            };
            match read {
                Ok(0) | Err(_) => {
                    // A WebSocket has no half-close: a pipe waits for the
                    // server's end, as the target may still answer.
                    let close = async {
                        let _ = up_session.send_close(1000, "").await;
                    };
                    end_of_input(at, down_ended_rx.notified(), close).await;
                    return;
                }
                Ok(n) => {
                    if up_session.send_binary(&buf[..n]).await.is_err() {
                        up_failed_tx.notify_one();
                        return;
                    }
                }
            }
        }
    });
    tokio::spawn(async move {
        let liveness = session.watch_liveness(pod_ws::client::LIVENESS_EVERY, pod_ws::client::LIVENESS_ALLOWED);
        tokio::pin!(liveness);
        loop {
            let read = tokio::select! {
                read = session.read_frame() => read,
                _ = &mut liveness => break,
                () = up_failed.notified() => break,
            };
            let Ok(f) = read else { break };
            match f.opcode {
                frame::OPCODE_CLOSE => break,
                frame::OPCODE_BINARY if !f.payload.is_empty() => {
                    if to_local.write_all(&f.payload).await.is_err() {
                        break;
                    }
                }
                frame::OPCODE_BINARY | frame::OPCODE_TEXT => {}
                _ => {
                    let _ = session.send_close(1002, "").await;
                    break;
                }
            }
        }
        down_ended.notify_one();
        let _ = to_local.shutdown().await;
    });
    ours
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_upgrade_names_the_link_s_path_and_host_and_no_subprotocol() {
        let link = ["vless://550e8400-e29b-41d4-a716-446655440000", "@203.0.113.9:443?type=ws&security=tls"].concat();
        let server = Server::parse(&format!("{link}&sni=v.example&host=cdn.example&path=%2Fws%3Fed%3D2048")).unwrap();
        let text = String::from_utf8(request(&server, "dGhlIHNhbXBsZSBub25jZQ==", "pod-egress/test")).unwrap();
        assert!(text.starts_with("GET /ws HTTP/1.1\r\nHost: cdn.example\r\n"), "{text}");
        assert!(!text.to_ascii_lowercase().contains("sec-websocket-protocol"), "{text}");
        let refused = failed(ConnectError::Refused { status: 404, body: String::new() }, "/ws?x=1");
        let line = refused.line("vless://203.0.113.9:443/ws", "h:22");
        assert!(line.contains("refused the WebSocket upgrade at /ws;") && line.ends_with("HTTP 404"), "{line}");
    }
}
