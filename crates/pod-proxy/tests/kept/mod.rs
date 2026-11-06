//! A server on the loopback that keeps each connection for more requests,
//! plain or over TLS, on its own thread and runtime, and counts the
//! connections that it accepts. A script says how it answers each request.

use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use pod_proxy::resolve::System;
use pod_tls::{Context, TlsRoots, Trust, Words};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// The certificates of the tests of pod-tls, minted in pure Rust.
#[allow(dead_code)]
mod cert {
    include!("../../../pod-tls/tests/common/cert_for_test.rs");
}

pub const NAME: &str = "kept.test";

/// The longest wait of a test for an event of the server.
pub const WAIT: Duration = Duration::from_secs(10);

/// What the server does with one request.
pub enum Act {
    /// Send these bytes, and read the next request.
    Answer(Vec<u8>),
    /// Send these bytes, and close the connection.
    AnswerThenClose(Vec<u8>),
    /// Close the connection with no answer.
    Close,
}

/// How the server answers request `nth` (from 1) of connection `conn`
/// (from 1), whose head is `head`.
pub type Script = fn(conn: usize, nth: usize, head: &str) -> Act;

/// A 200 with `text` and its `Content-Length`.
pub fn ok(text: &str) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{text}", text.len()).into_bytes()
}

/// The text of the answer to request `nth` of connection `conn`.
pub fn mark(conn: usize, nth: usize) -> String {
    format!("c{conn}r{nth}")
}

/// The server.
pub struct Kept {
    pub port: u16,
    accepted: Arc<AtomicUsize>,
    /// The head of each request, in the order of arrival.
    pub heads: mpsc::Receiver<String>,
    /// The number of each connection that the server closed.
    pub closed: mpsc::Receiver<usize>,
    pub context: Arc<Context>,
    roots: Option<rustls::RootCertStore>,
}

impl Kept {
    /// The connections accepted so far.
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// A new trust of the server's CA: each call gives its own
    /// configuration.
    pub fn trust(&self) -> Trust {
        let roots = self.roots.clone().expect("a TLS server");
        let roots = TlsRoots { count: roots.len(), roots, source: "the test's own CA".into() };
        Trust::caller(self.context.client_config(&roots).expect("a configuration")).expect("no ALPN")
    }

    /// The next head of a request.
    pub fn head(&self) -> String {
        self.heads.recv_timeout(WAIT).expect("a request")
    }
}

/// A resolver that pins [`NAME`] to the loopback.
pub fn pins() -> System {
    System::with_pins(vec![(NAME.into(), IpAddr::from([127, 0, 0, 1]))])
}

/// A plain HTTP server.
pub fn plain(script: Script) -> Kept {
    start(script, false)
}

/// An HTTPS server for [`NAME`].
pub fn tls(script: Script) -> Kept {
    start(script, true)
}

fn start(script: Script, tls: bool) -> Kept {
    let (acceptor, roots) = match tls {
        true => {
            let (acceptor, roots) = cert::server_for(NAME).expect("a test server");
            (Some(acceptor), Some(roots))
        }
        false => (None, None),
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("its address").port();
    listener.set_nonblocking(true).expect("nonblocking");
    let accepted = Arc::new(AtomicUsize::new(0));
    let (told, heads) = mpsc::channel();
    let (gone, closed) = mpsc::channel();
    let count = accepted.clone();
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("a tokio listener");
            loop {
                let Ok((tcp, _)) = listener.accept().await else { return };
                let conn = count.fetch_add(1, Ordering::SeqCst) + 1;
                let (acceptor, told, gone) = (acceptor.clone(), told.clone(), gone.clone());
                tokio::spawn(async move {
                    let closed = match acceptor {
                        Some(acceptor) => match acceptor.accept(tcp).await {
                            Ok(tls) => serve(tls, conn, script, &told).await,
                            Err(_) => true,
                        },
                        None => serve(tcp, conn, script, &told).await,
                    };
                    if closed {
                        let _ = gone.send(conn);
                    }
                });
            }
        });
    });
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podvm"))));
    Kept { port, accepted, heads, closed, context, roots }
}

/// Answer the requests of one connection. True when the server closed it.
async fn serve<S>(mut io: S, conn: usize, script: Script, told: &mpsc::Sender<String>) -> bool
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    for nth in 1.. {
        let end = loop {
            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
            match io.read(&mut chunk).await {
                Ok(n) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                _ => return false,
            }
        };
        let head = String::from_utf8_lossy(&buf[..end]).into_owned();
        let length = head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while buf.len() < end + length {
            match io.read(&mut chunk).await {
                Ok(n) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                _ => return false,
            }
        }
        buf.drain(..end + length);
        let act = script(conn, nth, &head);
        let _ = told.send(head);
        match act {
            Act::Answer(bytes) => {
                if io.write_all(&bytes).await.is_err() {
                    return false;
                }
            }
            Act::AnswerThenClose(bytes) => {
                let _ = io.write_all(&bytes).await;
                let _ = io.shutdown().await;
                return true;
            }
            Act::Close => {
                let _ = io.shutdown().await;
                return true;
            }
        }
    }
    false
}
