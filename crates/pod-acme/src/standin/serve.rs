//! The HTTPS of the stand-in: TLS on the provider of the caller, HTTP/1.1,
//! each request answered in turn on its connection until the client closes
//! it or asks for the close. A request body has a `Content-Length` of at
//! most [`MAX_BODY`].

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_proxy::http::request;
use rustls::crypto::CryptoProvider;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::net::TcpListener;

use super::flow::{self, State};

/// The largest request body: a JWS with a CSR is about 2 KiB.
pub const MAX_BODY: usize = 64 << 10;
/// The limit of a TLS handshake and of each wait for a request.
const IDLE: Duration = Duration::from_secs(30);

/// One request, as the flow reads it.
#[derive(Debug, Clone)]
pub struct Asked {
    pub method: String,
    /// The path and the query.
    pub target: String,
    /// The `Content-Type`, when given.
    pub kind: Option<String>,
    pub body: Vec<u8>,
}

/// One answer: its status, its headers past the framing, its body.
#[derive(Debug, Clone)]
pub struct Answer {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn json(status: u16, value: &serde_json::Value) -> Answer {
        let headers = vec![("Content-Type", "application/json".to_string())];
        Answer { status, headers, body: value.to_string().into_bytes() }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Answer {
        self.headers.push((name, value.into()));
        self
    }
}

/// Accept each connection of `listener` until the task ends.
pub async fn run(
    listener: TcpListener,
    acceptor: pod_tls::server::TlsAcceptor,
    state: Arc<Mutex<State>>,
    provider: Arc<CryptoProvider>,
) {
    loop {
        let Ok((tcp, _)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(20)).await;
            continue;
        };
        let (acceptor, state, provider) = (acceptor.clone(), state.clone(), provider.clone());
        tokio::spawn(async move {
            let Ok(Ok(tls)) = tokio::time::timeout(IDLE, acceptor.accept(tcp)).await else { return };
            connection(tls, &state, &provider).await;
        });
    }
}

/// Answer each request of one connection in turn.
async fn connection<S>(mut stream: S, state: &Arc<Mutex<State>>, provider: &Arc<CryptoProvider>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut rest = Vec::new();
    loop {
        let read = tokio::time::timeout(IDLE, read_one(&mut stream, std::mem::take(&mut rest))).await;
        let Ok(Ok((asked, more, close))) = read else { return };
        rest = more;
        let answer = flow::answer(state, provider, &asked);
        let head_only = asked.method == "HEAD";
        if stream.write_all(&encode(&answer, head_only, close)).await.is_err() || close {
            return;
        }
    }
}

/// One request: its head, its body by `Content-Length`, and whether the
/// client asked for the close. Gives the bytes after it too.
async fn read_one<S>(stream: &mut S, buffered: Vec<u8>) -> Result<(Asked, Vec<u8>, bool), String>
where
    S: AsyncRead + Unpin,
{
    let (head, mut rest) = request::read_request(stream, buffered).await.map_err(|e| format!("{e:?}"))?;
    if head.header("transfer-encoding").is_some() {
        return Err("a chunked body: the stand-in takes Content-Length alone".into());
    }
    let length = match head.header("content-length") {
        Some(v) => v.parse::<usize>().map_err(|_| "a Content-Length that is not a number")?,
        None => 0,
    };
    if length > MAX_BODY {
        return Err(format!("a body of {length} bytes"));
    }
    let mut chunk = [0u8; 4096];
    while rest.len() < length {
        let n = stream.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("the connection closed in the body".into());
        }
        rest.extend_from_slice(&chunk[..n]);
    }
    let after = rest.split_off(length);
    let close = head.minor == 0 || head.header("connection").is_some_and(|v| v.eq_ignore_ascii_case("close"));
    let kind = head.header("content-type").map(str::to_string);
    Ok((Asked { method: head.method, target: head.target, kind, body: rest }, after, close))
}

/// The bytes of an answer.
fn encode(answer: &Answer, head_only: bool, close: bool) -> Vec<u8> {
    let phrase = match answer.status {
        200 => "OK",
        status => pod_core::http1::reason(status),
    };
    let mut head = format!("HTTP/1.1 {} {phrase}\r\n", answer.status);
    for (name, value) in &answer.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if answer.status != 204 {
        head.push_str(&format!("Content-Length: {}\r\n", answer.body.len()));
    }
    head.push_str("Cache-Control: no-store\r\n");
    if close {
        head.push_str("Connection: close\r\n");
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    if !head_only && answer.status != 204 {
        bytes.extend_from_slice(&answer.body);
    }
    bytes
}
