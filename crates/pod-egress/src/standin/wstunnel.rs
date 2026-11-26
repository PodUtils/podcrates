//! A server of erebe/wstunnel on the loopback that reads a client as
//! wstunnel's server does, from its source at v11.1.0
//! (`erebe/wstunnel:wstunnel/src/tunnel/server/server.rs`,
//! `handle_tunnel_request`, and `utils.rs`): the path prefix of
//! `/PREFIX/events` and its restriction, the JWT after
//! `authorization.bearer.` in `Sec-WebSocket-Protocol`, its claims
//! `{"id","p":{"Tcp":{"proxy_protocol"}},"r","rp"}`, the `Authorization`
//! that a restrictions file matches, the target dialed before the 101 with
//! the subprotocol `v1`, and a 400 "Invalid request" for each refusal. A
//! [`Wst`] can check the token's signature with the old key, as 9.1 to 10.0
//! do, or answer as 9 does, or as a server that is no wstunnel. It takes a
//! frame's payload without its mask, as fastwebsockets 0.10 does under
//! wstunnel's default, unless it is told to apply it
//! (`--websocket-mask-frame`).

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use hmac::{Hmac, KeyInit, Mac};
use pod_ws::frame::{self, Frame, Role};
use sha2::Sha256;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{Task, locked};

/// The key of the signature of wstunnel 9.1 to 10.0.
const OLD_KEY: &[u8] = b"champignonfrais";

/// How the server answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// As wstunnel 10 and 11: the tunnel, or 400 "Invalid request".
    Tunnel,
    /// As wstunnel 9: 400 "Invalid upgrade request".
    Refuse9,
    /// A 101 without the subprotocol `v1`: another server.
    NoSubprotocol,
}

/// The settings of a server.
#[derive(Debug, Clone)]
pub struct Wst {
    /// `-r PREFIX`: the one path prefix that it takes.
    pub restrict: Option<String>,
    /// The `Authorization` that its restrictions match.
    pub authorization: Option<String>,
    /// The token's signature checked with the old key, as 9.1 to 10.0.
    pub checks_signature: bool,
    /// Each frame's mask applied, as `--websocket-mask-frame`.
    pub unmask: bool,
    pub answer: Answer,
    /// A client's Close answered at once, and nothing sent after it, as RFC
    /// 6455 allows a server; else the target's bytes go on.
    pub answers_close: bool,
}

impl Wst {
    /// A server of 11 with its defaults.
    pub fn v11() -> Wst {
        Wst {
            restrict: None,
            authorization: None,
            checks_signature: false,
            unmask: false,
            answer: Answer::Tunnel,
            answers_close: false,
        }
    }
}

impl Default for Wst {
    fn default() -> Wst {
        Wst::v11()
    }
}

/// One upgrade request as the server read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The target of the request line: `/PREFIX/events`.
    pub path: String,
    /// Its `Sec-WebSocket-Protocol`.
    pub protocol: String,
    /// Its `Authorization`.
    pub authorization: Option<String>,
}

/// What the server saw: each request, each target that it dialed, and each
/// frame's masking key.
#[derive(Debug, Default)]
pub struct Seen {
    pub requests: Vec<Asked>,
    pub targets: Vec<(String, u16)>,
    pub keys: Vec<Option<[u8; 4]>>,
}

/// A server that runs: its port, and what it saw.
pub type Stand = super::Stand<Mutex<Seen>>;

/// A server of `wst` on a free port of the loopback, over TLS with `tls`
/// when one is given.
pub async fn start(wst: Wst, tls: Option<Arc<rustls::ServerConfig>>) -> io::Result<Stand> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let seen: Arc<Mutex<Seen>> = Arc::default();
    let kept = seen.clone();
    let acceptor = tls.map(tokio_rustls::TlsAcceptor::from);
    let task = tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (wst, seen, acceptor) = (wst.clone(), kept.clone(), acceptor.clone());
            tokio::spawn(async move {
                match acceptor {
                    Some(acceptor) => {
                        if let Ok(tls) = acceptor.accept(tcp).await {
                            serve(tls, wst, seen).await;
                        }
                    }
                    None => serve(tcp, wst, seen).await,
                }
            });
        }
    });
    Ok(Stand { port, seen, task: Task::of(task) })
}

fn refusal(words: &str) -> String {
    format!("HTTP/1.1 400 Bad Request\r\ncontent-length: {}\r\n\r\n{words}", words.len())
}

/// The target of a request, or the refusal, in the order of
/// `handle_tunnel_request`: the path prefix and its restriction, the JWT,
/// then the restrictions' `Authorization`.
fn admit(head: &pod_proxy::http::Request, wst: &Wst) -> Option<(String, u16)> {
    let rest = head.target.strip_prefix('/')?;
    let (prefix, tail) = rest.split_once('/')?;
    if !tail.ends_with("events") || wst.restrict.as_deref().is_some_and(|r| r != prefix) {
        return None;
    }
    let protocol = head.header("sec-websocket-protocol")?;
    let jwt = protocol.split_once("authorization.bearer.")?.1;
    let parts: Vec<&str> = jwt.split('.').collect();
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header: serde_json::Value = serde_json::from_slice(&b64.decode(parts.first()?).ok()?).ok()?;
    if parts.len() != 3 || header["alg"] != "HS256" {
        return None;
    }
    if wst.checks_signature {
        let mut mac = Hmac::<Sha256>::new_from_slice(OLD_KEY).ok()?;
        mac.update(format!("{}.{}", parts[0], parts[1]).as_bytes());
        mac.verify_slice(&b64.decode(parts[2]).ok()?).ok()?;
    }
    let claims: serde_json::Value = serde_json::from_slice(&b64.decode(parts[1]).ok()?).ok()?;
    claims["id"].as_str()?;
    claims["p"]["Tcp"]["proxy_protocol"].as_bool()?;
    if wst.authorization.as_deref().is_some_and(|a| head.header("authorization") != Some(a)) {
        return None;
    }
    let host = claims["r"].as_str()?.trim_start_matches('[').trim_end_matches(']').to_string();
    Some((host, u16::try_from(claims["rp"].as_u64()?).ok()?))
}

async fn serve<S>(mut stream: S, wst: Wst, seen: Arc<Mutex<Seen>>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let Ok((head, pending)) = pod_proxy::http::read_request(&mut stream, Vec::new()).await else { return };
    let protocol = head.header("sec-websocket-protocol").unwrap_or_default().to_string();
    let authorization = head.header("authorization").map(String::from);
    locked(&seen).requests.push(Asked { path: head.target.clone(), protocol, authorization });
    if wst.answer == Answer::Refuse9 {
        let _ = stream.write_all(refusal("Invalid upgrade request").as_bytes()).await;
        return;
    }
    let Some(aim) = admit(&head, &wst) else {
        let _ = stream.write_all(refusal("Invalid request").as_bytes()).await;
        return;
    };
    locked(&seen).targets.push(aim.clone());
    // The target is dialed before the upgrade is answered, as exec_tunnel.
    let conn = TcpStream::connect((aim.0.as_str(), aim.1)).await;
    let (Ok(conn), Ok(key)) = (conn, pod_ws::server::check_upgrade(&head)) else {
        let _ = stream.write_all(refusal("Invalid request").as_bytes()).await;
        return;
    };
    let accept = pod_ws::handshake::accept_key(&key);
    let sub = if wst.answer == Answer::NoSubprotocol { "" } else { "sec-websocket-protocol: v1\r\n" };
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: upgrade\r\n\
         sec-websocket-accept: {accept}\r\n{sub}\r\n"
    );
    if stream.write_all(head.as_bytes()).await.is_ok() {
        pump(stream, pending, conn, &wst, seen).await;
    }
}

/// One frame as fastwebsockets 0.10 reads it: its opcode, its masking key,
/// and its payload as it came, the mask applied only when `unmask`.
async fn read_raw(r: &mut (impl AsyncRead + Unpin), unmask: bool) -> Option<(u8, Option<[u8; 4]>, Vec<u8>)> {
    let mut h = [0u8; 2];
    r.read_exact(&mut h).await.ok()?;
    let len = match h[1] & 0x7f {
        126 => usize::from(r.read_u16().await.ok()?),
        127 => usize::try_from(r.read_u64().await.ok()?).ok()?,
        n => usize::from(n),
    };
    let mut key = [0u8; 4];
    let masked = h[1] & 0x80 != 0;
    if masked {
        r.read_exact(&mut key).await.ok()?;
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload).await.ok()?;
    if masked && unmask {
        frame::apply_mask(&mut payload, key);
    }
    Some((h[0] & 0x0f, masked.then_some(key), payload))
}

async fn send(w: &tokio::sync::Mutex<impl AsyncWrite + Unpin>, opcode: u8, payload: &[u8]) -> bool {
    let bytes = frame::encode(&Frame { fin: true, opcode, payload: payload.to_vec() }, Role::Server, [0; 4]);
    w.lock().await.write_all(&bytes).await.is_ok()
}

/// The copy both ways, as `propagate_local_to_remote` and its reverse: each
/// data frame's payload to the target, a Pong for a Ping, and the target's
/// bytes in binary frames, then a Close at its end. With `answers_close`, a
/// client's Close is answered and ends the target's bytes.
async fn pump<S>(stream: S, pending: Vec<u8>, target: TcpStream, wst: &Wst, seen: Arc<Mutex<Seen>>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (unmask, answers_close) = (wst.unmask, wst.answers_close);
    let (rd, wr) = tokio::io::split(stream);
    let mut rd = std::io::Cursor::new(pending).chain(rd);
    let wr = Arc::new(tokio::sync::Mutex::new(wr));
    let (mut trd, mut twr) = target.into_split();
    let w = wr.clone();
    // This side's Close went: one at most, and no data after it.
    let closed = Arc::new(AtomicBool::new(false));
    let closed_up = closed.clone();
    let up = tokio::spawn(async move {
        while let Some((opcode, key, payload)) = read_raw(&mut rd, unmask).await {
            locked(&seen).keys.push(key);
            match opcode {
                frame::OPCODE_CLOSE if answers_close => {
                    if !closed_up.swap(true, Ordering::SeqCst) {
                        send(&*w, frame::OPCODE_CLOSE, &1000u16.to_be_bytes()).await;
                    }
                    break;
                }
                frame::OPCODE_CLOSE => break,
                frame::OPCODE_PING => {
                    send(&*w, frame::OPCODE_PONG, &payload).await;
                }
                frame::OPCODE_PONG => {}
                // wstunnel writes a text frame's payload as a binary one's.
                _ => {
                    if twr.write_all(&payload).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = twr.shutdown().await;
    });
    let mut buf = vec![0u8; 32 * 1024];
    while let Ok(n) = trd.read(&mut buf).await {
        if n == 0 || closed.load(Ordering::SeqCst) || !send(&*wr, frame::OPCODE_BINARY, &buf[..n]).await {
            break;
        }
    }
    if !closed.swap(true, Ordering::SeqCst) {
        send(&*wr, frame::OPCODE_CLOSE, &1000u16.to_be_bytes()).await;
    }
    let _ = up.await;
}
