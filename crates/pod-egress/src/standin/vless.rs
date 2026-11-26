//! Stand-ins of a VLESS server on the loopback, as Xray-core's behave
//! (`proxy/vless/inbound/inbound.go`, `transport/internet/websocket/hub.go`,
//! `transport/internet/splithttp/hub.go`): over a WebSocket at `/ws`, and over
//! XHTTP at `/xh/` on h2's own HTTP/2 server, which this crate did not write,
//! or on HTTP/1.1. Each reads VLESS's request, ends the session with no
//! answer for an ID that is not [`ID`] (a Close of 1000, or the end of the
//! GET's body), and otherwise dials the target and sends the answer with the
//! target's first bytes. The XHTTP one answers 400 to a padding out of 100 to
//! 1000 and 404 to another path, and puts packet-up's POSTs in order by their
//! numbers.

mod xhttp;
mod xhttp1;

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_ws::session::RelaySession;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub use xhttp1::CLOSE_AFTER;

use super::{Stand, Task, locked};

/// The users' ID of the stand-ins: a made-up value.
pub const ID: &str = "550e8400-e29b-41d4-a716-446655440000";
/// The bytes of [`ID`].
const ID_BYTES: [u8; 16] = [0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0, 0];

/// What a stand-in saw: each request's method, path, `Host` and padding's
/// length, each target that a VLESS request named, and the bytes of each
/// opening write.
#[derive(Debug, Default)]
pub struct Seen {
    pub requests: Mutex<Vec<(String, String, String, usize)>>,
    pub targets: Mutex<Vec<String>>,
    /// The connections that the WebSocket and the HTTP/1.1 stand-ins took.
    pub connections: AtomicUsize,
    /// The bytes of each WebSocket upgrade's head, which is the whole of the
    /// session's first write.
    pub openings: Mutex<Vec<usize>>,
}

/// VLESS's request at the front of `buf`, once whole: its ID, its target
/// and its length. A request of another version or command, or an address
/// of another type, is none.
pub fn parse_request(buf: &[u8]) -> Option<([u8; 16], String, usize)> {
    let addons = usize::from(*buf.get(17)?);
    let at = 18 + addons;
    let command = *buf.get(at)?;
    if (buf[0], command) != (0, 1) {
        return None;
    }
    let port = u16::from_be_bytes([*buf.get(at + 1)?, *buf.get(at + 2)?]);
    let (host, end) = match buf.get(at + 3)? {
        1 => (format!("{}", std::net::Ipv4Addr::from(<[u8; 4]>::try_from(buf.get(at + 4..at + 8)?).ok()?)), at + 8),
        2 => {
            let len = usize::from(*buf.get(at + 4)?);
            (String::from_utf8(buf.get(at + 5..at + 5 + len)?.to_vec()).ok()?, at + 5 + len)
        }
        _ => return None,
    };
    Some((buf[1..17].try_into().ok()?, format!("{host}:{port}"), end))
}

/// `config` with the protocols of `alpn`.
fn acceptor(config: &rustls::ServerConfig, alpn: &[&[u8]]) -> tokio_rustls::TlsAcceptor {
    let mut config = config.clone();
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    tokio_rustls::TlsAcceptor::from(Arc::new(config))
}

/// A VLESS server over a WebSocket at `/ws`, over TLS with `config`, on a
/// free port of the loopback.
pub async fn ws_server(config: Arc<rustls::ServerConfig>) -> io::Result<Stand<Seen>> {
    let (acceptor, listener) = (acceptor(&config, &[]), TcpListener::bind("127.0.0.1:0").await?);
    let port = listener.local_addr()?.port();
    let seen: Arc<Seen> = Arc::default();
    let kept = seen.clone();
    let task = tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, seen) = (acceptor.clone(), kept.clone());
            tokio::spawn(async move {
                seen.connections.fetch_add(1, Ordering::SeqCst);
                let Ok(tls) = acceptor.accept(tcp).await else { return };
                serve_ws(tls, seen).await;
            });
        }
    });
    Ok(Stand { port, seen, task: Task::of(task) })
}

async fn serve_ws<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(mut tls: S, seen: Arc<Seen>) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match tls.read(&mut byte).await {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let path = head.split(' ').nth(1).unwrap_or_default().to_string();
    locked(&seen.openings).push(head.len());
    let field = |name: &str| {
        let mut fields = head.split("\r\n").filter_map(|l| l.split_once(':'));
        fields.find(|(n, _)| n.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string())
    };
    locked(&seen.requests).push(("GET".into(), path.clone(), field("host").unwrap_or_default(), 0));
    let bare = path.split('?').next().unwrap_or("/");
    if bare != "/ws" {
        let _ = tls.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
        return;
    }
    let accept = pod_ws::handshake::accept_key(&field("sec-websocket-key").unwrap_or_default());
    let answer = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: \
         {accept}\r\n\r\n"
    );
    if tls.write_all(answer.as_bytes()).await.is_err() {
        return;
    }
    let session = Arc::new(RelaySession::new(tls, Vec::new(), None, Duration::from_secs(30)).as_server());
    let mut got = Vec::new();
    let (id, target, used) = loop {
        let Ok(frame) = session.read_frame().await else { return };
        got.extend_from_slice(&frame.payload);
        if let Some(request) = parse_request(&got) {
            break request;
        }
    };
    let Some(tcp) = dial(id, &target, &seen).await else {
        // As Xray-core's connection closes: a Close of 1000 and no reason.
        let _ = session.send_close(1000, "").await;
        return;
    };
    let (mut rd, mut wr) = tcp.into_split();
    let _ = wr.write_all(&got[used..]).await;
    let up_session = session.clone();
    let up = async move {
        while let Ok(frame) = up_session.read_frame().await {
            if frame.opcode == pod_ws::frame::OPCODE_CLOSE || wr.write_all(&frame.payload).await.is_err() {
                break;
            }
        }
    };
    let down = async move {
        let (mut buf, mut answer) = (vec![0u8; 16 * 1024], vec![0u8, 0u8]);
        while let Ok(n) = rd.read(&mut buf).await {
            if n == 0 {
                break;
            }
            answer.extend_from_slice(&buf[..n]);
            if session.send_binary(&answer).await.is_err() {
                return;
            }
            answer.clear();
        }
        let _ = session.send_close(1000, "").await;
    };
    tokio::join!(up, down);
}

/// The target of a request whose ID is [`ID`], else none.
async fn dial(id: [u8; 16], target: &str, seen: &Seen) -> Option<TcpStream> {
    if id != ID_BYTES {
        return None;
    }
    locked(&seen.targets).push(target.to_string());
    TcpStream::connect(target).await.ok()
}

/// A VLESS server over XHTTP at `/xh/` on HTTP/2, over TLS with `config`
/// that offers `alpn`, on a free port of the loopback.
pub async fn xhttp_server(config: Arc<rustls::ServerConfig>, alpn: &[&[u8]]) -> io::Result<Stand<Seen>> {
    let (acceptor, listener) = (acceptor(&config, alpn), TcpListener::bind("127.0.0.1:0").await?);
    let port = listener.local_addr()?.port();
    let (seen, sessions): (Arc<Seen>, xhttp::Sessions) = (Arc::default(), Arc::default());
    let kept = seen.clone();
    let task = tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, seen, sessions) = (acceptor.clone(), kept.clone(), sessions.clone());
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else { return };
                let Ok(mut connection) = h2::server::handshake(tls).await else { return };
                while let Some(Ok((request, respond))) = connection.accept().await {
                    tokio::spawn(xhttp::handle(request, respond, seen.clone(), sessions.clone()));
                }
            });
        }
    });
    Ok(Stand { port, seen, task: Task::of(task) })
}

/// A VLESS server over XHTTP at `/xh/` that speaks HTTP/1.1 alone, as
/// Xray-core's with `alpn` `http/1.1`: over TLS with `config` that offers
/// `http/1.1` alone, or with no TLS when `config` is none.
pub async fn xhttp1_server(config: Option<Arc<rustls::ServerConfig>>) -> io::Result<Stand<Seen>> {
    let acceptor = config.map(|config| acceptor(&config, &[b"http/1.1"]));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let (seen, sessions): (Arc<Seen>, xhttp::Sessions) = (Arc::default(), Arc::default());
    let kept = seen.clone();
    let task = tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, seen, sessions) = (acceptor.clone(), kept.clone(), sessions.clone());
            tokio::spawn(async move {
                let Some(acceptor) = acceptor else {
                    return xhttp1::serve(tcp, seen, sessions).await;
                };
                let Ok(tls) = acceptor.accept(tcp).await else { return };
                xhttp1::serve(tls, seen, sessions).await;
            });
        }
    });
    Ok(Stand { port, seen, task: Task::of(task) })
}
