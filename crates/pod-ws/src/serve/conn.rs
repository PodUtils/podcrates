//! One connection of the loop: its socket options, its stages (TLS, the
//! head, the 101 and the first frame), each within its deadline, then the
//! wait for `ready` in the hands of the caller.

use std::net::SocketAddr;
use std::sync::Arc;

use pod_identity::PublicKey;
use pod_proxy::http::request::{self, Request, RequestError};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until, timeout_at};
use tokio_rustls::TlsAcceptor;

use super::gate::{Gate, PreAuth, Slot, Stage};
use super::{Accepted, Options, Serve, ServedStream};
use crate::frame::{self, Role};
use crate::server;
use crate::session::RelaySession;

/// The bytes of one read before the session.
const CHUNK: usize = 4096;

/// What the task of one connection holds.
pub(super) struct Conn {
    pub(super) gate: Arc<Gate>,
    pub(super) slot: Slot,
    pub(super) tcp: TcpStream,
    pub(super) from: SocketAddr,
    pub(super) acceptor: TlsAcceptor,
    pub(super) cfg: Arc<Serve>,
    pub(super) out: mpsc::UnboundedSender<Accepted>,
    /// The loop's channel of the TLS handshakes that failed.
    pub(super) tls_failed: mpsc::UnboundedSender<(SocketAddr, std::io::Error)>,
    /// The accept.
    pub(super) start: Instant,
}

/// What the stages give: the session, the key of the peer, the head.
type Shaken = (RelaySession<ServedStream>, Option<PublicKey>, Request);

/// Run the stages of one connection, give its session to the loop, and
/// cut it at an eviction, at its deadline of `ready`, or at a drop of its
/// gate with no `ready`.
pub(super) async fn run(conn: Conn) {
    let Conn { gate, slot: Slot { id, mut evicted }, tcp, from, acceptor, cfg, out, tls_failed, start } = conn;
    let options = tune(&tcp, cfg.lowat);
    // A second handle of the socket: its shutdown cuts the session in the
    // hands of the caller.
    let cutter = socket2::SockRef::from(&tcp).try_clone().ok();
    let end = start + cfg.deadlines.ready;
    let shaken = tokio::select! {
        biased;
        _ = &mut evicted => Err(Why::Other("evicted".to_string())),
        shaken = shake(&gate, id, tcp, &acceptor, &cfg, start, end) => shaken,
    };
    let (session, key, request) = match shaken {
        Ok(shaken) => shaken,
        Err(Why::Tls(e)) => {
            tracing::debug!(%from, error = %e, "a connection before ready is cut: TLS");
            gate.release(id);
            let _ = tls_failed.send((from, e));
            return;
        }
        Err(Why::Other(why)) => {
            tracing::debug!(%from, why, "a connection before ready is cut");
            gate.release(id);
            return;
        }
    };
    if !gate.advance(id, Stage::Ready) {
        tracing::debug!(%from, "a connection before ready is cut: evicted");
        return;
    }
    let (ready, readied) = oneshot::channel();
    let gate_of = PreAuth::new(gate.clone(), id, ready);
    let accepted = Accepted { session, peer: from, key, request, options, gate: gate_of };
    if out.send(accepted).is_err() {
        return;
    }
    // Of a ready and the deadline, the one that frees the slot acts. A
    // ready or a drop of the gate frees the slot, which closes the signal
    // of an eviction: then the gate's own signal tells which.
    let mut readied = readied;
    let cut = tokio::select! {
        biased;
        readied = &mut readied => readied.err().map(|_| "dropped with no ready"),
        evicted = &mut evicted => match evicted {
            Ok(()) => Some("evicted"),
            Err(_) => (&mut readied).await.err().map(|_| "dropped with no ready"),
        },
        () = sleep_until(end) => gate.release(id).then_some("the deadline of ready"),
    };
    gate.release(id);
    if let Some(why) = cut {
        tracing::debug!(%from, why, "a session before ready is cut");
        if let Some(socket) = cutter {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// `TCP_NODELAY`, and `TCP_NOTSENT_LOWAT` of `lowat` where the system has
/// it; then the options as `getsockopt` reads them.
fn tune(tcp: &TcpStream, lowat: Option<u32>) -> Options {
    // The frames of a session are small; Nagle would hold one behind an
    // acknowledgement.
    let _ = tcp.set_nodelay(true);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(lowat) = lowat {
        let _ = socket2::SockRef::from(tcp).set_tcp_notsent_lowat(lowat);
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let _ = lowat;
    Options { nodelay: tcp.nodelay().unwrap_or(false), notsent_lowat: notsent_lowat(tcp) }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn notsent_lowat(tcp: &TcpStream) -> Option<u32> {
    socket2::SockRef::from(tcp).tcp_notsent_lowat().ok()
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn notsent_lowat(_: &TcpStream) -> Option<u32> {
    None
}

/// Why the stages cut a connection: its TLS handshake failed (the caller
/// sees the error), or another stage ended it.
enum Why {
    Tls(std::io::Error),
    Other(String),
}

impl From<String> for Why {
    fn from(why: String) -> Why {
        Why::Other(why)
    }
}

/// The stages of one connection, each within its deadline and within
/// `end`, the deadline of `ready`.
async fn shake(
    gate: &Gate,
    id: u64,
    tcp: TcpStream,
    acceptor: &TlsAcceptor,
    cfg: &Serve,
    start: Instant,
    end: Instant,
) -> Result<Shaken, Why> {
    let d = &cfg.deadlines;
    let at = (start + d.tls).min(end);
    let mut tls =
        timeout_at(at, acceptor.accept(tcp)).await.map_err(|_| "the deadline of TLS".to_string())?.map_err(Why::Tls)?;
    let key = pod_tls::peer::peer_key(tls.get_ref().1.peer_certificates());
    gate.advance(id, Stage::Head);

    let at = (Instant::now() + d.head).min(end);
    let head = timeout_at(at, read_head(&mut tls, cfg.caps.head_bytes, cfg.caps.head_lines)).await;
    let (request, rest) = match head.map_err(|_| "the deadline of the head".to_string())? {
        Ok(read) => read,
        Err(e) => {
            if let Some(status) = e.status() {
                let answer = request::refusal(status, &e.to_string(), &[]);
                let _ = timeout_at(at, tls.write_all(&answer)).await;
            }
            return Err(format!("the head: {e}").into());
        }
    };
    let checked = server::check_upgrade(&request)
        .and_then(|key_ws| cfg.protocol.map_or(Ok(()), |p| server::check_protocol(&request, p)).map(|()| key_ws));
    let key_ws = match checked {
        Ok(key_ws) => key_ws,
        Err(refused) => {
            let _ = timeout_at(at, tls.write_all(&refused.answer())).await;
            return Err(format!("the upgrade: {} {}", refused.status, refused.why).into());
        }
    };

    // The stage moves before the 101: a client that reads the 101 knows
    // that its connection waits for its first frame.
    gate.advance(id, Stage::First);
    let at = (Instant::now() + d.first).min(end);
    let first = async {
        tls.write_all(&server::switching_to(&key_ws, cfg.protocol)).await.map_err(|e| format!("the 101: {e}"))?;
        tls.flush().await.map_err(|e| format!("the 101: {e}"))?;
        read_first(&mut tls, rest, cfg.receive_cap()).await
    };
    let pending = timeout_at(at, first).await.map_err(|_| "the deadline of the first frame".to_string())??;
    let session = server::session(tls, pending, cfg.idle).with_limits(cfg.limits);
    Ok((session, key, request))
}

/// The head of the request: at most `bytes` bytes and `lines` lines. It
/// gives the request and the bytes after its head.
async fn read_head(tls: &mut ServedStream, bytes: usize, lines: usize) -> Result<(Request, Vec<u8>), RequestError> {
    let mut buf = Vec::with_capacity(CHUNK.min(bytes));
    let mut chunk = [0u8; CHUNK];
    loop {
        if let Some(end) = head_end(&buf) {
            if end > bytes || buf[..end].iter().filter(|&&b| b == b'\n').count() > lines + 1 {
                return Err(RequestError::TooLarge);
            }
            let rest = buf.split_off(end);
            return Ok((request::parse_request(&buf)?, rest));
        }
        let lines_so_far = buf.iter().filter(|&&b| b == b'\n').count();
        if buf.len() > bytes || lines_so_far > lines {
            return Err(RequestError::TooLarge);
        }
        let n = tls.read(&mut chunk).await.map_err(|e| RequestError::Gone(e.to_string()))?;
        if n == 0 {
            return Err(RequestError::Gone("the connection closed".into()));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// The end of a head: after its empty line, ended by CRLF or a bare LF.
fn head_end(buf: &[u8]) -> Option<usize> {
    let mut at = 0;
    while let Some(lf) = buf[at..].iter().position(|&b| b == b'\n') {
        let next = at + lf + 1;
        if buf[next..].starts_with(b"\n") {
            return Some(next + 1);
        }
        if buf[next..].starts_with(b"\r\n") {
            return Some(next + 2);
        }
        at = next;
    }
    None
}

/// Read until one whole frame of the client is in `buf`, which keeps it
/// for the session. A frame that breaks RFC 6455 or the cap cuts it.
async fn read_first(tls: &mut ServedStream, mut buf: Vec<u8>, cap: usize) -> Result<Vec<u8>, String> {
    let mut chunk = [0u8; CHUNK];
    loop {
        match frame::decode_capped(&buf, Role::Client, cap) {
            Ok(Some(_)) => return Ok(buf),
            Ok(None) => {}
            Err(e) => return Err(format!("the first frame: {e}")),
        }
        let n = tls.read(&mut chunk).await.map_err(|e| format!("the first frame: {e}"))?;
        if n == 0 {
            return Err("the connection closed before its first frame".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_of_a_head_is_after_its_empty_line() {
        assert_eq!(head_end(b"GET / HTTP/1.1\r\nHost: h\r\n\r\nrest"), Some(27));
        assert_eq!(head_end(b"GET / HTTP/1.1\nHost: h\n\nrest"), Some(24));
        assert_eq!(head_end(b"GET / HTTP/1.1\r\nHost: h\r\n"), None);
    }
}
