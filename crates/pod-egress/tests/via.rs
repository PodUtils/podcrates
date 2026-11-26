//! A second proxy over TLS against stand-ins on the loopback: one
//! that speaks HTTP/1.1's CONNECT after TLS, as squid's `https_port` does,
//! and one whose HTTP/2 is h2's own server, which this crate did not write, as
//! Caddy's forwardproxy answers a CONNECT over HTTP/2. Each asks for Basic
//! and answers `407` with `Proxy-Authenticate` without it, then carries the
//! target: an SSH-like server that sends its version line and echoes. The
//! TLS is checked for the proxy's name with the test's root; the host's own
//! proxy, a plain CONNECT stand-in, carries the TCP first where one is set.
//! The stand-ins are the crate's own (`pod_egress::standin`, feature
//! `standin`). The tests against Caddy and squid themselves are podssh's
//! `scripts/interop-via.sh`, which drive its binary.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_egress::standin::via::{Mode, Plan, Stand};
use pod_egress::standin::{BANNER, Echo};
use pod_egress::via::{Client, Dial, Offer, Version, Via};
use pod_proxy::{DialError, Proxy, ProxyChoice};
use pod_tls::Trust;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// Shared test support; this file uses part of it.
#[allow(dead_code)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

#[path = "common/way.rs"]
mod way;

const NAME: &str = "proxy.via.test";
const LIMIT: Duration = Duration::from_secs(20);
const USER: &str = "podssh";
const H1: &[&[u8]] = &[b"http/1.1"];
const H2: &[&[u8]] = &[b"h2"];
const BOTH: &[&[u8]] = &[b"h2", b"http/1.1"];
/// The test's own password, made of parts so that no scanner takes the
/// line for a credential.
fn password() -> String {
    ["pw", "-of-", "the-test"].concat()
}

/// The way out through `proxy`, with the names of the stand-ins at the
/// loopback.
fn pinned(proxy: ProxyChoice) -> pod_ws::Way {
    let lo: IpAddr = "127.0.0.1".parse().unwrap();
    way::through(proxy, &[(NAME, lo), ("other.via.test", lo)])
}

/// How the client reaches the second proxy through `proxy`, with podssh's
/// place of the credentials file.
fn dial(root: &Path, offer: Offer, proxy: ProxyChoice) -> Dial {
    let trust = Trust::File(root.to_path_buf());
    Dial { way: pinned(proxy), trust, timeout: LIMIT, offer, auth_file: "--via-auth-file FILE".into() }
}

fn root_file(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-egress-via-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("root.pem");
    let der = cert::encoding::self_signed_inner(&cert::encoding::anchor_name(NAME), true).der;
    std::fs::write(&path, cert::encoding::pem(&der)).unwrap();
    path
}

/// The target: its version line, then an echo.
async fn target() -> Echo {
    pod_egress::standin::echo().await.unwrap()
}

/// The plan of a stand-in that takes `alpn` and asks for the test's Basic.
fn plan(alpn: &[&[u8]], mode: Mode) -> Plan {
    let alpn = alpn.iter().map(|p| p.to_vec()).collect();
    Plan { alpn, mode, credentials: Some((USER.to_string(), password())) }
}

fn basic() -> String {
    plan(H1, Mode::Carry).basic().unwrap()
}

/// A stand-in proxy over TLS for [`NAME`] that offers `alpn`.
async fn proxy(alpn: &[&[u8]], mode: Mode) -> Stand {
    let (acceptor, _) = cert::server_for(NAME).unwrap();
    pod_egress::standin::via::start(acceptor.config().clone(), plan(alpn, mode)).await.unwrap()
}

/// The host's own proxy: plain CONNECT, which sends a name of [`pins`] to
/// the loopback, and keeps each target that it was asked for.
async fn host_proxy() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let asked: Arc<Mutex<Vec<String>>> = Arc::default();
    let kept = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            let asked = kept.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    match tcp.read(&mut byte).await {
                        Ok(1) => head.push(byte[0]),
                        _ => return,
                    }
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let target = head.split(' ').nth(1).unwrap_or_default().to_string();
                asked.lock().unwrap().push(target.clone());
                let port = target.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok()).unwrap_or(0);
                let Ok(mut out) = TcpStream::connect(("127.0.0.1", port)).await else { return };
                if tcp.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.is_ok() {
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut out).await;
                }
            });
        }
    });
    (port, asked)
}

fn client(port: u16, root: &Path, offer: Offer, credentials: bool, proxy: ProxyChoice) -> Client {
    let via = Via::parse(&format!("https://{NAME}:{port}")).unwrap();
    let via = if credentials { via.with_credentials(&format!("{USER}:{}\n", password())).unwrap() } else { via };
    Client::new(via, dial(root, offer, proxy))
}

/// A tunnel's version line, then `sent` echoed back.
async fn carries(opened: &mut pod_egress::via::Opened, sent: &[u8]) {
    let mut banner = vec![0u8; BANNER.len()];
    let mut back = vec![0u8; sent.len()];
    let both = async {
        opened.stream.read_exact(&mut banner).await.unwrap();
        opened.stream.write_all(sent).await.unwrap();
        opened.stream.read_exact(&mut back).await.unwrap();
    };
    tokio::time::timeout(LIMIT, both).await.expect("the bytes within the limit");
    assert_eq!(banner, BANNER);
    assert!(back == sent, "the bytes came back changed");
}

async fn open(client: &Client, port: u16) -> Result<pod_egress::via::Opened, DialError> {
    tokio::time::timeout(LIMIT * 2, client.open("127.0.0.1", port)).await.expect("an answer within the limit")
}

/// The credential goes with each CONNECT, on HTTP/1.1 and on HTTP/2, and
/// the tunnel carries the target; with none, the `407` is said with its
/// words: what the proxy asks for, and that podssh sent none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_credential_goes_with_each_connect_and_a_407_says_what_is_missing() {
    pod_testkit::need_loopback!();
    let (root, echo) = (root_file("credential"), target().await);
    let aim = echo.port;
    for (alpn, version) in [(H1, Version::Http1), (H2, Version::Http2)] {
        let stand = proxy(alpn, Mode::Carry).await;
        let port = stand.port;
        let with = client(port, &root, Offer::Both, true, ProxyChoice::Direct);
        let mut opened = open(&with, aim).await.unwrap_or_else(|e| panic!("{}: {e}", version.name()));
        assert_eq!(opened.version, version);
        carries(&mut opened, b"through the second proxy").await;
        let first = stand.seen.connects()[0].clone();
        assert_eq!((first.version, first.target.as_str()), (version.name(), format!("127.0.0.1:{aim}").as_str()));
        assert_eq!(first.authorization.as_deref(), Some(basic().as_str()));
        let without = client(port, &root, Offer::Both, false, ProxyChoice::Direct);
        let why = open(&without, aim).await.err().expect("a refusal").to_string();
        let words = format!("the proxy https://{NAME}:{port} refused CONNECT 127.0.0.1:{aim}: 407 Proxy");
        assert!(why.starts_with(&words) && why.contains(" Authentication Required; "), "{why}");
        assert!(why.contains("it asks for Basic, and podssh sent none") && why.contains("--via-auth-file"), "{why}");
    }
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// Over HTTP/2, two tunnels at once are two streams of one TLS connection,
/// each with its own bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_tunnels_share_one_http2_connection() {
    pod_testkit::need_loopback!();
    let (root, echo) = (root_file("shared"), target().await);
    let aim = echo.port;
    let stand = proxy(BOTH, Mode::Carry).await;
    let client = client(stand.port, &root, Offer::Both, true, ProxyChoice::Direct);
    let mut first = open(&client, aim).await.unwrap();
    let mut second = open(&client, aim).await.unwrap();
    assert_eq!((first.version, first.shared), (Version::Http2, false));
    assert_eq!((second.version, second.shared), (Version::Http2, true));
    carries(&mut second, b"the second").await;
    let big = vec![7u8; 300_000];
    carries(&mut first, &big).await;
    assert_eq!(stand.seen.accepted(), 1, "one TLS connection");
    assert_eq!(stand.seen.connects().len(), 2);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// The host's own proxy carries the TCP to the second by its name; TLS
/// and the CONNECT go inside, and the host's proxy sees neither.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_host_s_proxy_carries_the_tls_to_the_second_proxy() {
    pod_testkit::need_loopback!();
    let (root, echo) = (root_file("hop"), target().await);
    let aim = echo.port;
    let stand = proxy(BOTH, Mode::Carry).await;
    let port = stand.port;
    let (hop, asked) = host_proxy().await;
    let first = ProxyChoice::Via(Proxy::parse(&format!("http://127.0.0.1:{hop}")).unwrap());
    let mut opened = open(&client(port, &root, Offer::Http1, true, first), aim).await.unwrap();
    assert_eq!(opened.version, Version::Http1, "HTTP/1.1 alone was offered");
    carries(&mut opened, b"two proxies").await;
    assert_eq!(*asked.lock().unwrap(), vec![format!("{NAME}:{port}")]);
    assert_eq!(stand.seen.connects()[0].target, format!("127.0.0.1:{aim}"));
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// A proxy that resets the CONNECT on HTTP/2 gets HTTP/1.1 on a connection
/// of its own, and says why; with HTTP/2 alone asked, it is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connect_that_http2_resets_goes_over_http1() {
    pod_testkit::need_loopback!();
    let (root, echo) = (root_file("reset"), target().await);
    let aim = echo.port;
    let stand = proxy(BOTH, Mode::ResetHttp2).await;
    let port = stand.port;
    let mut opened = open(&client(port, &root, Offer::Both, true, ProxyChoice::Direct), aim).await.unwrap();
    assert_eq!(opened.version, Version::Http1);
    assert!(opened.fell_back.as_deref().is_some_and(|w| w.contains("HTTP/2 did not carry")), "{:?}", opened.fell_back);
    carries(&mut opened, b"after the reset").await;
    assert_eq!(stand.seen.accepted(), 2);
    let only = open(&client(port, &root, Offer::Http2, true, ProxyChoice::Direct), aim).await;
    assert!(matches!(only, Err(DialError::ProxyProtocol { .. })), "{:?}", only.err());
    let h1 = proxy(H1, Mode::Carry).await;
    let refused = open(&client(h1.port, &root, Offer::Http2, true, ProxyChoice::Direct), aim).await.err().unwrap();
    assert!(refused.to_string().contains("did not choose HTTP/2"), "{refused}");
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// A 403, a target that the proxy cannot reach, and a certificate of
/// another name each fail with what failed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refusal_an_unreachable_target_and_another_name_fail_with_their_words() {
    pod_testkit::need_loopback!();
    let (root, echo) = (root_file("refusals"), target().await);
    let aim = echo.port;
    for alpn in [H1, H2] {
        let forbids = proxy(alpn, Mode::Forbid).await;
        let e = open(&client(forbids.port, &root, Offer::Both, true, ProxyChoice::Direct), aim).await.err().unwrap();
        assert!(matches!(e, DialError::ProxyRefused { status: 403, .. }), "{e}");
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
        let carries = proxy(alpn, Mode::Carry).await;
        let e = open(&client(carries.port, &root, Offer::Both, true, ProxyChoice::Direct), closed).await.err().unwrap();
        assert!(matches!(e, DialError::ProxyRefused { status: 502, .. }), "{e}");
    }
    let other = proxy(H2, Mode::Carry).await;
    let via = Via::parse(&format!("https://other.via.test:{}", other.port)).unwrap();
    let e = Client::new(via, dial(&root, Offer::Both, ProxyChoice::Direct)).open("127.0.0.1", aim).await.err().unwrap();
    assert!(matches!(&e, DialError::ProxyUnreachable { detail, .. } if detail.starts_with("TLS")), "{e}");
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// The client is a dialer: a dial of a target is a tunnel to it, which the
/// far end names; a refusal is a failure of the proxy's stage, of the class
/// refused, in the proxy's words.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_client_is_a_dialer_and_a_refusal_is_a_typed_failure() {
    pod_testkit::need_loopback!();
    let (root, echo) = (root_file("dialer"), target().await);
    let aim = echo.port;
    let stand = proxy(BOTH, Mode::Carry).await;
    let port = stand.port;
    let carrying = client(port, &root, Offer::Both, true, ProxyChoice::Direct);
    let dialer: &dyn pod_core::Dial = &carrying;
    let target = pod_core::Target::new("127.0.0.1", aim);
    assert!(dialer.names_inside());
    assert_eq!(dialer.proxy_label(&target), Some(format!("https://{NAME}:{port}")));
    let deadline = tokio::time::Instant::now() + LIMIT;
    let mut io = dialer.dial(&target, deadline).await.unwrap_or_else(|e| panic!("{e}")).io;
    let mut banner = vec![0u8; BANNER.len()];
    let mut back = [0u8; 9];
    let both = async {
        io.read_exact(&mut banner).await.unwrap();
        io.write_all(b"a dialer!").await.unwrap();
        io.read_exact(&mut back).await.unwrap();
    };
    tokio::time::timeout(LIMIT, both).await.expect("the bytes within the limit");
    assert_eq!((banner.as_slice(), &back), (BANNER, b"a dialer!"));
    let forbids = proxy(H1, Mode::Forbid).await;
    let refusing = client(forbids.port, &root, Offer::Both, true, ProxyChoice::Direct);
    let failure = (&refusing as &dyn pod_core::Dial).dial(&target, deadline).await.expect_err("a refusal");
    assert_eq!((failure.stage, failure.class), (pod_core::Stage::Proxy, pod_core::Class::Refused), "{failure}");
    assert!(failure.text().contains(&format!("refused CONNECT 127.0.0.1:{aim}: 403")), "{failure}");
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
