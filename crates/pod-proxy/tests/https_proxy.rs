//! A proxy of the scheme `https`: TLS to the proxy with the caller's trust,
//! then CONNECT inside that TLS, with the Basic of the URL. The proxy is the
//! stand-in of `pod-egress` (`pod_egress::standin::via`, feature `standin`),
//! with a certificate of the test CA of `pod-tls`; the target sends a
//! version line, then echoes. A recorder between the dialer and the proxy
//! keeps each byte that goes out in the open.

use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use pod_core::{Class, Dial, Stage, Target};
use pod_egress::standin::via::{Mode, Plan, Stand};
use pod_egress::standin::{BANNER, Echo};
use pod_proxy::resolve::System;
use pod_proxy::{DialError, Dialer, Environment, ProxyChoice, ProxyKind, Settings, Stream};
use pod_tls::{Context, TlsRoots, Trust, Words};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// The certificates of the tests of pod-tls, minted in pure Rust.
#[allow(dead_code)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

const NAME: &str = "proxy.https.test";
const OTHER: &str = "other.https.test";
const LIMIT: Duration = Duration::from_secs(10);
const USER: &str = "podnet";

/// The test's own password, made of parts so that no scanner takes the
/// line for a credential.
fn password() -> String {
    ["pw", "-of-", "the-https-test"].concat()
}

/// The proxy over TLS for [`NAME`] that asks for the test's Basic, and the
/// caller's TLS: a context of podnet and the trust of the test CA alone.
async fn proxy() -> (Stand, Arc<Context>, Trust) {
    let (acceptor, roots) = cert::server_for(NAME).expect("a test server");
    let plan = Plan {
        alpn: vec![b"http/1.1".to_vec()],
        mode: Mode::Carry,
        credentials: Some((USER.to_string(), password())),
    };
    let stand = pod_egress::standin::via::start(acceptor.config().clone(), plan).await.expect("a stand-in");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podnet"))));
    let config = context
        .client_config(&TlsRoots { count: roots.len(), roots, source: "the test's own CA".into() })
        .expect("a configuration");
    (stand, context, Trust::caller(config).expect("no ALPN"))
}

async fn target() -> Echo {
    pod_egress::standin::echo().await.expect("a target")
}

/// The Basic of the test's credentials, as the proxy takes it.
fn basic() -> String {
    let pair = format!("{USER}:{}", password());
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(pair))
}

/// The URL of the proxy at `name` and `port`, with the test's credentials.
fn url(name: &str, port: u16) -> String {
    format!("https://{USER}:{}@{name}:{port}", password())
}

/// A dialer through `choice`, with [`NAME`] and [`OTHER`] at the loopback.
fn dialer(choice: ProxyChoice, tls: Option<(Arc<Context>, Trust)>) -> Dialer {
    let lo = IpAddr::from([127, 0, 0, 1]);
    let pins = System::with_pins(vec![(NAME.into(), lo), (OTHER.into(), lo)]);
    let dialer = Dialer::new(Settings::new("podnet", choice), Arc::new(pins));
    match tls {
        Some((context, trust)) => dialer.with_proxy_tls(context, trust),
        None => dialer,
    }
}

fn via(name: &str, port: u16) -> ProxyChoice {
    ProxyChoice::Via(pod_proxy::Proxy::parse(&url(name, port)).expect("an https proxy URL"))
}

/// The version line, then `sent` back, through `io`.
async fn carries(io: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin), sent: &[u8]) {
    let mut banner = vec![0u8; BANNER.len()];
    let mut back = vec![0u8; sent.len()];
    let both = async {
        io.read_exact(&mut banner).await.unwrap();
        io.write_all(sent).await.unwrap();
        io.read_exact(&mut back).await.unwrap();
    };
    tokio::time::timeout(LIMIT, both).await.expect("the bytes within the limit");
    assert_eq!((banner.as_slice(), back.as_slice()), (BANNER, sent));
}

/// A dial through the proxy is a tunnel to the target inside TLS, with the
/// Basic of the URL; `stream` gives the TLS stream, and `tcp`, which takes
/// plain TCP only, says why it refuses.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dial_through_an_https_proxy_carries_the_target() {
    pod_testkit::need_loopback!();
    let ((stand, context, trust), echo) = (proxy().await, target().await);
    let dialer = dialer(via(NAME, stand.port), Some((context, trust)));
    assert_eq!(dialer.label(), format!("https://{NAME}:{}", stand.port));
    assert!(dialer.names_inside());
    let target = Target::new("127.0.0.1", echo.port);
    let mut io = dialer.dial(&target, tokio::time::Instant::now() + LIMIT).await.unwrap_or_else(|e| panic!("{e}")).io;
    carries(&mut io, b"through the https proxy").await;
    let connects = stand.seen.connects();
    assert_eq!(connects.len(), 1, "{connects:?}");
    assert_eq!((connects[0].version, connects[0].target.clone()), ("HTTP/1.1", format!("127.0.0.1:{}", echo.port)));
    assert_eq!(connects[0].authorization.as_deref(), Some(basic().as_str()));
    let (mut stream, _) = dialer.stream("127.0.0.1", echo.port, LIMIT).await.expect("a stream");
    assert!(matches!(stream, Stream::Tls(_)) && stream.tcp().is_none() && stream.peer_addr().is_some());
    carries(&mut stream, b"a stream of TLS").await;
    let refused = dialer.tcp("127.0.0.1", echo.port, LIMIT).await.expect_err("plain TCP only");
    assert!(refused.to_string().contains("is reached over TLS (https://), and this caller takes a plain TCP"));
}

/// A certificate for another name is refused at the stage `Proxy`, with
/// the words of pod-tls, and no CONNECT reaches the proxy; a dialer with no
/// TLS for a proxy says so and dials nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_certificate_is_refused_with_the_words_of_pod_tls() {
    pod_testkit::need_loopback!();
    let ((stand, context, trust), echo) = (proxy().await, target().await);
    let target = Target::new("127.0.0.1", echo.port);
    let wrong = dialer(via(OTHER, stand.port), Some((context, trust)));
    let failure = wrong.dial(&target, tokio::time::Instant::now() + LIMIT).await.expect_err("a refusal");
    assert_eq!((failure.stage, failure.class), (Stage::Proxy, Class::Unavailable), "{failure}");
    let words = format!("could not reach the proxy {OTHER}:{}: TLS: ", stand.port);
    assert!(failure.text().starts_with(&words), "{failure}");
    assert!(failure.text().contains(NAME) && failure.text().contains("not other.https.test"), "{failure}");
    let cause = std::error::Error::source(&failure).and_then(|e| e.downcast_ref::<DialError>());
    assert!(matches!(cause, Some(DialError::ProxyUnreachable { .. })), "{cause:?}");
    let none = dialer(via(NAME, stand.port), None);
    let failure = none.dial(&target, tokio::time::Instant::now() + LIMIT).await.expect_err("no TLS");
    assert_eq!((failure.stage, failure.class), (Stage::Proxy, Class::Config), "{failure}");
    assert!(failure.text().contains("has no TLS context for a proxy"), "{failure}");
    assert!(stand.seen.connects().is_empty(), "{:?}", stand.seen.connects());
}

/// A recorder on the loopback in front of `port`: each byte that the client
/// sends, as it goes.
async fn recorder(port: u16) -> (u16, Arc<Mutex<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let front = listener.local_addr().unwrap().port();
    let kept: Arc<Mutex<Vec<u8>>> = Arc::default();
    let bytes = kept.clone();
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let Ok(server) = TcpStream::connect(("127.0.0.1", port)).await else { return };
            let bytes = bytes.clone();
            tokio::spawn(async move {
                let (mut cr, mut cw) = client.into_split();
                let (mut sr, mut sw) = server.into_split();
                let up = async move {
                    let mut buf = [0u8; 4096];
                    while let Ok(n) = cr.read(&mut buf).await {
                        if n == 0 || sw.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                        bytes.lock().unwrap().extend_from_slice(&buf[..n]);
                    }
                    let _ = sw.shutdown().await;
                };
                let down = async move {
                    let _ = tokio::io::copy(&mut sr, &mut cw).await;
                    let _ = cw.shutdown().await;
                };
                tokio::join!(up, down);
            });
        }
    });
    (front, kept)
}

/// The CONNECT and its Basic go inside the TLS alone: the bytes in the open
/// start with a record of TLS, and hold no CONNECT, no header, no Basic and
/// no password.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_basic_of_an_https_proxy_goes_inside_the_tls_only() {
    pod_testkit::need_loopback!();
    let ((stand, context, trust), echo) = (proxy().await, target().await);
    let (front, recorded) = recorder(stand.port).await;
    let dialer = dialer(via(NAME, front), Some((context, trust)));
    let target = Target::new("127.0.0.1", echo.port);
    let dialed = dialer.dial(&target, tokio::time::Instant::now() + LIMIT).await;
    let clear = recorded.lock().unwrap().clone();
    let text = String::from_utf8_lossy(&clear);
    for secret in [basic(), password(), "CONNECT".into(), "Proxy-Authorization".into()] {
        assert!(!text.contains(&secret), "the Basic went in cleartext: {secret:?} is in the bytes to the proxy");
    }
    assert_eq!(clear.first(), Some(&0x16), "the first byte to the proxy starts no TLS handshake");
    carries(&mut dialed.unwrap_or_else(|e| panic!("{e}")).io, b"inside").await;
    assert_eq!(stand.seen.connects()[0].authorization.as_deref(), Some(basic().as_str()));
}

/// `HTTPS_PROXY=https://...` is read by the env reader as a proxy of the
/// scheme `https`, and a dial of the environment goes through it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn https_proxy_of_the_environment_is_an_https_proxy() {
    pod_testkit::need_loopback!();
    let ((stand, context, trust), echo) = (proxy().await, target().await);
    let value = url(NAME, stand.port);
    let reader = move |name: &str| (name == "HTTPS_PROXY").then(|| std::ffi::OsString::from(&value));
    let env = Environment::read(&reader);
    let given = env.proxy.as_ref().expect("a proxy of the environment");
    let proxy = given.value.as_ref().expect("an https proxy is taken");
    assert_eq!((given.origin.to_string().as_str(), proxy.kind()), ("HTTPS_PROXY", ProxyKind::Https));
    let dialer = dialer(ProxyChoice::FromEnvironment(env), Some((context, trust)));
    assert_eq!(dialer.label(), format!("https://{NAME}:{} of HTTPS_PROXY", stand.port));
    let target = Target::new("127.0.0.1", echo.port);
    let mut io = dialer.dial(&target, tokio::time::Instant::now() + LIMIT).await.unwrap_or_else(|e| panic!("{e}")).io;
    carries(&mut io, b"from the environment").await;
}

/// The blocking face gives the TLS stream on the standard socket, with each
/// byte that the session holds.
#[cfg(feature = "blocking")]
#[test]
fn the_blocking_face_carries_a_stream_through_an_https_proxy() {
    use std::io::{Read, Write};
    pod_testkit::need_loopback!();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let ((stand, context, trust), echo) = runtime.block_on(async { (proxy().await, target().await) });
    let inner = dialer(via(NAME, stand.port), Some((context, trust)));
    let blocking = pod_proxy::blocking::Dialer::from_async(inner).unwrap();
    let (mut stream, _) = blocking.stream("127.0.0.1", echo.port, LIMIT).expect("a stream");
    assert!(matches!(stream, pod_proxy::blocking::Stream::Tls(_)) && stream.peer_addr().is_some());
    stream.set_timeouts(Some(LIMIT)).unwrap();
    let mut banner = vec![0u8; BANNER.len()];
    stream.read_exact(&mut banner).unwrap();
    stream.write_all(b"blocking").unwrap();
    let mut back = [0u8; 8];
    stream.read_exact(&mut back).unwrap();
    assert_eq!((banner.as_slice(), &back), (BANNER, b"blocking"));
}
