//! A forward proxy over TLS on the loopback: TLS with the caller's server
//! configuration and the protocols of its [`Plan`] by ALPN; then HTTP/1.1's
//! CONNECT (RFC 9110, section 9.3.6), its head read a byte at a time, or
//! HTTP/2's (RFC 9113, section 8.5) with h2's own server, which this crate
//! did not write. With credentials in the plan, a CONNECT without their
//! Basic gets `407` with `Proxy-Authenticate: Basic`. It dials the target
//! that the CONNECT names, as it names it, and carries the bytes both ways.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine as _;
use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{Task, locked};

/// What the proxy does with a CONNECT that has the credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Carry the target.
    Carry,
    /// Refuse it with 403.
    Forbid,
    /// On HTTP/2, reset the stream, as a proxy that takes no CONNECT over
    /// HTTP/2 can; HTTP/1.1 carries it.
    ResetHttp2,
}

/// What the proxy serves.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The protocols that it takes by ALPN: `h2` gets HTTP/2, each other
    /// HTTP/1.1.
    pub alpn: Vec<Vec<u8>>,
    pub mode: Mode,
    /// The user and the password of the Basic that it asks for; `None`: it
    /// asks for none.
    pub credentials: Option<(String, String)>,
}

impl Plan {
    /// HTTP/1.1 and HTTP/2, [`Mode::Carry`], no credentials.
    pub fn new() -> Plan {
        Plan { alpn: vec![b"h2".to_vec(), b"http/1.1".to_vec()], mode: Mode::Carry, credentials: None }
    }

    /// The value of `Proxy-Authorization` that the plan takes.
    pub fn basic(&self) -> Option<String> {
        let (user, password) = self.credentials.as_ref()?;
        let pair = format!("{user}:{password}");
        Some(format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(pair.as_bytes())))
    }
}

impl Default for Plan {
    fn default() -> Plan {
        Plan::new()
    }
}

/// One CONNECT as the proxy read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connect {
    /// `HTTP/1.1` or `HTTP/2`.
    pub version: &'static str,
    /// The authority that it names.
    pub target: String,
    /// Its `Proxy-Authorization`.
    pub authorization: Option<String>,
}

/// What the proxy saw: its TLS connections, and each CONNECT.
#[derive(Debug, Default)]
pub struct Seen {
    pub accepted: AtomicUsize,
    pub connects: std::sync::Mutex<Vec<Connect>>,
}

impl Seen {
    /// The count of the TLS connections that it took.
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// Each CONNECT so far.
    pub fn connects(&self) -> Vec<Connect> {
        locked(&self.connects).clone()
    }
}

/// A proxy that runs: its port, and what it saw.
pub type Stand = super::Stand<Seen>;

/// A proxy over TLS with `config` (its certificate; the ALPN of `plan`
/// replaces its own) on a free port of the loopback.
pub async fn start(config: Arc<rustls::ServerConfig>, plan: Plan) -> io::Result<Stand> {
    let mut config = (*config).clone();
    config.alpn_protocols = plan.alpn.clone();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let seen: Arc<Seen> = Arc::default();
    let (kept, plan) = (seen.clone(), Arc::new(plan));
    let task = tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, seen, plan) = (acceptor.clone(), kept.clone(), plan.clone());
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else { return };
                seen.accepted.fetch_add(1, Ordering::SeqCst);
                if tls.get_ref().1.alpn_protocol() == Some(b"h2".as_slice()) {
                    serve_h2(tls, plan, seen).await;
                } else {
                    serve_h1(tls, &plan, &seen).await;
                }
            });
        }
    });
    Ok(Stand { port, seen, task: Task::of(task) })
}

/// The answer to a CONNECT: the target's stream, or a status.
async fn decide(connect: Connect, plan: &Plan, seen: &Seen) -> Result<TcpStream, u16> {
    let target = connect.target.clone();
    let authorization = connect.authorization.clone();
    locked(&seen.connects).push(connect);
    if let Some(basic) = plan.basic()
        && authorization.as_deref() != Some(basic.as_str())
    {
        return Err(407);
    }
    if plan.mode == Mode::Forbid {
        return Err(403);
    }
    TcpStream::connect(target.as_str()).await.map_err(|_| 502)
}

async fn serve_h1<S: AsyncRead + AsyncWrite + Unpin>(mut tls: S, plan: &Plan, seen: &Seen) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match tls.read(&mut byte).await {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let target = head.split(' ').nth(1).unwrap_or_default().to_string();
    let authorization = head
        .split("\r\n")
        .filter_map(|l| l.split_once(':'))
        .find(|(n, _)| n.trim().eq_ignore_ascii_case("proxy-authorization"))
        .map(|(_, v)| v.trim().to_string());
    match decide(Connect { version: "HTTP/1.1", target, authorization }, plan, seen).await {
        Ok(mut tcp) => {
            if tls.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await.is_ok() {
                let _ = tokio::io::copy_bidirectional(&mut tls, &mut tcp).await;
            }
        }
        Err(407) => {
            let answer = "HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic \
                          realm=\"stand-in\"\r\nContent-Length: 0\r\n\r\n";
            let _ = tls.write_all(answer.as_bytes()).await;
        }
        Err(status) => {
            let _ = tls.write_all(format!("HTTP/1.1 {status} No\r\nContent-Length: 0\r\n\r\n").as_bytes()).await;
        }
    }
}

async fn serve_h2<S>(tls: S, plan: Arc<Plan>, seen: Arc<Seen>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let Ok(mut connection) = h2::server::handshake(tls).await else { return };
    while let Some(Ok((request, mut respond))) = connection.accept().await {
        let (seen, plan) = (seen.clone(), plan.clone());
        tokio::spawn(async move {
            let target = request.uri().authority().map(|a| a.to_string()).unwrap_or_default();
            let authorization =
                request.headers().get("proxy-authorization").and_then(|v| v.to_str().ok()).map(String::from);
            // RFC 9113, section 8.5: a CONNECT has an authority alone.
            let connect = Connect { version: "HTTP/2", target, authorization };
            let formed = request.method() == http::Method::CONNECT
                && request.uri().scheme().is_none()
                && request.uri().path_and_query().is_none();
            if !formed || plan.mode == Mode::ResetHttp2 {
                locked(&seen.connects).push(connect);
                let reason = if formed { h2::Reason::REFUSED_STREAM } else { h2::Reason::PROTOCOL_ERROR };
                respond.send_reset(reason);
                return;
            }
            let status = match decide(connect, &plan, &seen).await {
                Ok(tcp) => {
                    let answer = http::Response::builder().status(200).body(()).expect("a response of 200");
                    let Ok(send) = respond.send_response(answer, false) else { return };
                    return bridge(request.into_body(), send, tcp).await;
                }
                Err(status) => status,
            };
            let mut answer = http::Response::builder().status(status);
            if status == 407 {
                answer = answer.header("proxy-authenticate", "Basic realm=\"stand-in\"");
            }
            if let Ok(answer) = answer.body(()) {
                let _ = respond.send_response(answer, true);
            }
        });
    }
}

/// A stream of HTTP/2 and the target's TCP, both ways.
async fn bridge(mut recv: h2::RecvStream, mut send: h2::SendStream<Bytes>, tcp: TcpStream) {
    let (mut rd, mut wr) = tcp.into_split();
    let up = async move {
        while let Some(Ok(chunk)) = recv.data().await {
            let _ = recv.flow_control().release_capacity(chunk.len());
            if wr.write_all(&chunk).await.is_err() {
                return;
            }
        }
        let _ = wr.shutdown().await;
    };
    let down = async move {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = match rd.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let mut rest = &buf[..n];
            while !rest.is_empty() {
                send.reserve_capacity(rest.len());
                match std::future::poll_fn(|cx| send.poll_capacity(cx)).await {
                    Some(Ok(0)) => continue,
                    Some(Ok(room)) => {
                        let take = room.min(rest.len());
                        if send.send_data(Bytes::copy_from_slice(&rest[..take]), false).is_err() {
                            return;
                        }
                        rest = &rest[take..];
                    }
                    _ => return,
                }
            }
        }
        let _ = send.send_data(Bytes::new(), true);
    };
    tokio::join!(up, down);
}
