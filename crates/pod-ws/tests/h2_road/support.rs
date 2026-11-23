//! The stand-ins and the helpers of `tests/h2_road.rs`: the relays of the
//! loopback whose HTTP/2 is h2's own server, which this crate did not write,
//! a stand-in that refuses the extended CONNECT, the way of the tests, and
//! the bytes of a session.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use pod_ws::frame::OPCODE_BINARY;
use pod_ws::h2_road::{self, Road};
use pod_ws::{ConnectError, Endpoint, RelaySession, Trust, WsClientConfig};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpListener;

// Shared test support; this file uses part of it.
#[allow(dead_code)]
mod cert {
    include!("../../../pod-tls/tests/common/cert_for_test.rs");
}

pub const NAME: &str = "relay.h2.test";
pub const TOKEN: &str = "token-of-the-test";
pub const PATH: &str = "/connect/127.0.0.1/22";
pub const LIMIT: Duration = Duration::from_secs(60);
pub const MIB: usize = 1 << 20;
pub const REFUSAL: &str = "forward: missing or wrong token";
const H1: &[&[u8]] = &[b"http/1.1"];
const BOTH: &[&[u8]] = &[b"h2", b"http/1.1"];

/// What a stand-in's TLS and HTTP/2 offer.
#[derive(Clone, Copy, PartialEq)]
pub enum Offer {
    Extended,
    NoSetting,
    Http1Only,
    /// The SETTINGS offer extended CONNECT, and each CONNECT is answered
    /// 405: a proxy that refuses the extended CONNECT, as one refuses
    /// `Upgrade`.
    RefusesConnect,
}

/// A CONNECT of HTTP/2 as the stand-in's application got it.
#[derive(Debug)]
pub struct Asked {
    pub protocol: Option<String>,
    pub scheme: Option<String>,
    pub authority: Option<String>,
    pub path: Option<String>,
    pub version: Option<String>,
    pub key: Option<String>,
}

impl Asked {
    fn of(request: &http::Request<h2::RecvStream>) -> Asked {
        let field = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(String::from);
        Asked {
            protocol: request.extensions().get::<h2::ext::Protocol>().map(|p| p.as_str().to_string()),
            scheme: request.uri().scheme_str().map(String::from),
            authority: request.uri().authority().map(|a| a.to_string()),
            path: request.uri().path_and_query().map(|p| p.to_string()),
            version: field("sec-websocket-version"),
            key: field("sec-websocket-key"),
        }
    }
}

#[derive(Default)]
pub struct Seen {
    pub accepted: AtomicUsize,
    pub upgrades: AtomicUsize,
    pub headers: AtomicUsize,
    pub connects: Mutex<Vec<Asked>>,
    /// Each connection's task, for a cut.
    pub tasks: Mutex<Vec<tokio::task::AbortHandle>>,
}

#[path = "runtimes.rs"]
mod runtimes;

/// The pool of the tests: each test has its own port, so its own hosts.
pub static POOL: h2_road::Pool = h2_road::Pool::new();

/// The way of the tests, which finds the relay's name on the loopback.
pub fn way() -> pod_ws::Way {
    let lo: IpAddr = "127.0.0.1".parse().unwrap();
    crate::common::way_with_pins(&[(NAME, lo)])
}

pub fn root_file(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-ws-h2-road-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("root.pem");
    let der = cert::encoding::self_signed_inner(&cert::encoding::anchor_name(NAME), true).der;
    std::fs::write(&path, cert::encoding::pem(&der)).unwrap();
    path
}

pub fn config(port: u16, root: &Path) -> WsClientConfig {
    WsClientConfig {
        endpoint: Endpoint { host: NAME.into(), port, path: PATH.into() },
        trust: Trust::File(root.to_path_buf()),
        server_name: NAME.into(),
        timeout: LIMIT,
        idle_timeout: Some(LIMIT),
        way: way(),
        limits: pod_ws::Limits::default(),
    }
}

/// The frames of HTTP/2 that a client sends, read past the preface by their
/// headers alone: each HEADERS (type 1) is counted.
struct Wire {
    preface: usize,
    head: Vec<u8>,
    skip: usize,
}

impl Wire {
    fn feed(&mut self, mut bytes: &[u8], seen: &Seen) {
        while !bytes.is_empty() {
            let n = if self.preface > 0 {
                let n = self.preface.min(bytes.len());
                self.preface -= n;
                n
            } else if self.skip > 0 {
                let n = self.skip.min(bytes.len());
                self.skip -= n;
                n
            } else {
                let n = (9 - self.head.len()).min(bytes.len());
                self.head.extend_from_slice(&bytes[..n]);
                if self.head.len() == 9 {
                    let h = &self.head;
                    self.skip = (h[0] as usize) << 16 | (h[1] as usize) << 8 | h[2] as usize;
                    if h[3] == 0x1 {
                        seen.headers.fetch_add(1, Ordering::SeqCst);
                    }
                    self.head.clear();
                }
                n
            };
            bytes = &bytes[n..];
        }
    }
}

/// The client's bytes on their way to h2's server, read by [`Wire`].
struct Tap<S> {
    inner: S,
    wire: Wire,
    seen: Arc<Seen>,
}

impl<S: AsyncRead + Unpin> AsyncRead for Tap<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let polled = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &polled {
            this.wire.feed(&buf.filled()[before..], &this.seen);
        }
        polled
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Tap<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, data)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// A stand-in relay for [`NAME`] that offers `offer`.
pub async fn stand_in(offer: Offer) -> (u16, Arc<Seen>) {
    let (acceptor, _) = cert::server_for(NAME).unwrap();
    let mut tls = (**acceptor.config()).clone();
    let alpn = if offer == Offer::Http1Only { H1 } else { BOTH };
    tls.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Seen> = Arc::default();
    let kept = seen.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, seen) = (acceptor.clone(), kept.clone());
            let task = tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else { return };
                seen.accepted.fetch_add(1, Ordering::SeqCst);
                if tls.get_ref().1.alpn_protocol() == Some(b"h2".as_slice()) {
                    let wire = Wire { preface: 24, head: Vec::new(), skip: 0 };
                    serve_h2(Tap { inner: tls, wire, seen: seen.clone() }, offer, seen).await;
                } else {
                    serve_h1(tls, seen).await;
                }
            });
            kept.tasks.lock().unwrap().push(task.abort_handle());
        }
    });
    (port, seen)
}

async fn serve_h2<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(io: S, offer: Offer, seen: Arc<Seen>) {
    let mut builder = h2::server::Builder::new();
    builder.max_concurrent_streams(32);
    if offer == Offer::Extended || offer == Offer::RefusesConnect {
        builder.enable_connect_protocol();
    }
    let Ok(mut connection) = builder.handshake::<_, Bytes>(io).await else { return };
    while let Some(Ok((request, mut respond))) = connection.accept().await {
        if offer == Offer::RefusesConnect {
            seen.connects.lock().unwrap().push(Asked::of(&request));
            let answer = http::Response::builder().status(405).header("allow", "GET").body(()).unwrap();
            let _ = respond.send_response(answer, true);
            continue;
        }
        tokio::spawn(answer_h2(request, respond, seen.clone()));
    }
}

async fn answer_h2(
    request: http::Request<h2::RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
    seen: Arc<Seen>,
) {
    let (asked, token_ok) = {
        let asked = Asked::of(&request);
        let field = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(String::from);
        (asked, field("x-relay-token").as_deref() == Some(TOKEN))
    };
    seen.connects.lock().unwrap().push(asked);
    if !token_ok {
        let answer = http::Response::builder().status(403).header("content-type", "text/plain").body(()).unwrap();
        if let Ok(mut send) = respond.send_response(answer, false) {
            let _ = send.send_data(Bytes::from(format!("{REFUSAL}\n")), true);
        }
        return;
    }
    let Ok(send) = respond.send_response(http::Response::builder().status(200).body(()).unwrap(), false) else {
        return;
    };
    echo(pod_ws::server::session(h2_road::duplex(send, request.into_body()), Vec::new(), None)).await;
}

async fn serve_h1<S: AsyncRead + AsyncWrite + Unpin>(mut tls: S, seen: Arc<Seen>) {
    let Ok((head, pending)) = pod_proxy::http::request::read_request(&mut tls, Vec::new()).await else { return };
    if head.header("x-relay-token") != Some(TOKEN) {
        let _ = tls.write_all(&pod_proxy::http::request::refusal(403, REFUSAL, &[])).await;
        return;
    }
    let key = match pod_ws::server::check_upgrade(&head) {
        Ok(key) => key,
        Err(refused) => {
            let _ = tls.write_all(&refused.answer()).await;
            return;
        }
    };
    seen.upgrades.fetch_add(1, Ordering::SeqCst);
    if tls.write_all(&pod_ws::server::switching(&key)).await.is_ok() {
        echo(pod_ws::server::session(tls, pending, None)).await;
    }
}

/// Each binary message back as it came, until the Close, which the
/// session's reader answers.
async fn echo<S: AsyncRead + AsyncWrite>(session: RelaySession<S>) {
    while let Ok(frame) = session.read_frame().await {
        if frame.opcode != OPCODE_BINARY || session.send_binary(&frame.payload).await.is_err() {
            return;
        }
    }
}

/// Bytes that a seed fixes (xorshift64*).
fn bytes(size: usize, mut seed: u64) -> Vec<u8> {
    (0..size)
        .map(|_| {
            seed ^= seed >> 12;
            seed ^= seed << 25;
            seed ^= seed >> 27;
            (seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 56) as u8
        })
        .collect()
}

/// `size` bytes out through `session` in messages of 64 KiB while the echo
/// is read: each byte back once, in order, by digest; then the Close.
pub async fn each_way(session: RelaySession, size: usize, seed: u64) {
    let session = Arc::new(session);
    let out = bytes(size, seed);
    let sent = Sha256::digest(&out);
    let writer = {
        let session = session.clone();
        tokio::spawn(async move {
            for chunk in out.chunks(64 * 1024) {
                session.send_binary(chunk).await.expect("a message out");
            }
        })
    };
    let (mut back, mut got) = (Sha256::new(), 0);
    let read = async {
        while got < size {
            let frame = session.read_frame().await.expect("a message back");
            assert_eq!(frame.opcode, OPCODE_BINARY);
            got += frame.payload.len();
            back.update(&frame.payload);
        }
    };
    tokio::time::timeout(echo_limit(), read).await.expect("the echo within the limit");
    writer.await.unwrap();
    assert_eq!(got, size, "each byte came back once");
    assert_eq!(back.finalize(), sent, "the digests are equal");
    let _ = session.send_close(1000, "").await;
}

/// The bytes of each of the eight sessions: 4 MiB each way on a kernel,
/// 1 MiB under an emulator, which carries TLS and HTTP/2 too slowly for the
/// limit. The test of the shared connection stays the same.
pub fn per_session() -> usize {
    if pod_testkit::host::emulated() { MIB } else { 4 * MIB }
}

/// The wait for the echo of one session: 60 s on a kernel, five times that
/// under an emulator.
pub fn echo_limit() -> Duration {
    pod_testkit::host::scaled(LIMIT)
}

pub async fn connect(config: &WsClientConfig, token: &str) -> Result<(RelaySession, Road), ConnectError> {
    tokio::time::timeout(LIMIT * 2, h2_road::connect(&POOL, config, token)).await.expect("an answer within the limit")
}
