//! A TLS server for [`NAME`] on the loopback that answers each request with
//! one large body, on its own thread and runtime: the async tests and the
//! blocking ones both use it.

use std::net::IpAddr;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use pod_proxy::resolve::System;
use pod_tls::{Context, TlsRoots, Trust, Words};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// The certificates of the tests of pod-tls, minted in pure Rust.
#[allow(dead_code)]
mod cert {
    include!("../../../pod-tls/tests/common/cert_for_test.rs");
}

pub const NAME: &str = "relay.test";

/// More than the bound of a buffered body (`client::MAX_BODY`).
pub const LARGE: usize = pod_proxy::client::MAX_BODY + (1 << 20);

/// How the server answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// The body with its `Content-Length`.
    Length,
    /// The body in chunks of many sizes, and a trailer.
    Chunked,
    /// A `Content-Length` of the body, half of it, and the close.
    Cut,
    /// A `Content-Length` of the body, half of it, and no more bytes.
    Stall,
    /// A `Content-Length` and no body (the answer to a `HEAD`), and no
    /// more bytes.
    HeadOnly,
    /// A 302 to this location; a `GET` of this path gets the body with its
    /// `Content-Length`.
    #[allow(dead_code, reason = "only the tests of redirects use it")]
    Redirect(&'static str),
}

/// The server: its port, the trust of its CA, a TLS context, and the heads
/// of the requests that it got.
pub struct Served {
    pub port: u16,
    pub trust: Trust,
    pub context: Arc<Context>,
    pub heads: mpsc::Receiver<String>,
}

/// `size` bytes that do not repeat in a short period.
pub fn body(size: usize) -> Arc<Vec<u8>> {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let bytes = (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    Arc::new(bytes)
}

pub fn sha256(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

/// A resolver that pins [`NAME`] to the loopback.
pub fn pins() -> System {
    System::with_pins(vec![(NAME.into(), IpAddr::from([127, 0, 0, 1]))])
}

/// Start a server that answers each request with `body` as `answer` says.
pub fn serve(answer: Answer, body: Arc<Vec<u8>>) -> Served {
    let (acceptor, roots) = cert::server_for(NAME).expect("a test server");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("its address").port();
    listener.set_nonblocking(true).expect("nonblocking");
    let (told, heads) = mpsc::channel();
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("a tokio listener");
            loop {
                let Ok((tcp, _)) = listener.accept().await else { return };
                let (acceptor, body, told) = (acceptor.clone(), body.clone(), told.clone());
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(tcp).await else { return };
                    let mut head = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !head.ends_with(b"\r\n\r\n") {
                        match tls.read(&mut chunk).await {
                            Ok(n) if n > 0 => head.extend_from_slice(&chunk[..n]),
                            _ => return,
                        }
                    }
                    let head = String::from_utf8_lossy(&head).into_owned();
                    let answer = match answer {
                        Answer::Redirect(to) if head.starts_with(&format!("GET {to} ")) => Answer::Length,
                        answer => answer,
                    };
                    let _ = told.send(head);
                    let _ = answer_with(&mut tls, answer, &body).await;
                });
            }
        });
    });
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podvm"))));
    let config = context
        .client_config(&TlsRoots { count: roots.len(), roots, source: "the test's own CA".into() })
        .expect("a configuration");
    Served { port, trust: Trust::caller(config).expect("no ALPN"), context, heads }
}

async fn answer_with<S>(tls: &mut S, answer: Answer, body: &[u8]) -> std::io::Result<()>
where
    S: AsyncWriteExt + Unpin,
{
    let length = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
    match answer {
        Answer::Length => {
            tls.write_all(length.as_bytes()).await?;
            for part in body.chunks(64 << 10) {
                tls.write_all(part).await?;
            }
        }
        Answer::Chunked => {
            tls.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").await?;
            let (mut at, mut size) = (0, 1);
            while at < body.len() {
                let part = &body[at..body.len().min(at + size)];
                tls.write_all(format!("{:x};x=y\r\n", part.len()).as_bytes()).await?;
                tls.write_all(part).await?;
                tls.write_all(b"\r\n").await?;
                at += part.len();
                size = (size * 7 + 13) % (200 << 10) + 1;
            }
            tls.write_all(b"0\r\nX-Trailer: t\r\n\r\n").await?;
        }
        Answer::Cut => {
            tls.write_all(length.as_bytes()).await?;
            tls.write_all(&body[..body.len() / 2]).await?;
        }
        Answer::Stall | Answer::HeadOnly => {
            tls.write_all(length.as_bytes()).await?;
            if answer == Answer::Stall {
                tls.write_all(&body[..body.len() / 2]).await?;
            }
            tls.flush().await?;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        Answer::Redirect(to) => {
            let head = format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\n\r\n");
            tls.write_all(head.as_bytes()).await?;
        }
    }
    tls.shutdown().await
}
