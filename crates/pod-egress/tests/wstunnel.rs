//! wstunnel's protocol against stand-ins on the loopback that read
//! a client as wstunnel's server does, from its source at v11.1.0
//! (`erebe/wstunnel:wstunnel/src/tunnel/server/server.rs`,
//! `handle_tunnel_request`, and `utils.rs`): the path prefix of
//! `/PREFIX/events` and its restriction, the JWT after
//! `authorization.bearer.` in `Sec-WebSocket-Protocol`, its claims
//! `{"id","p":{"Tcp":{"proxy_protocol"}},"r","rp"}`, the `Authorization`
//! that a restrictions file matches, the target dialed before the 101 with
//! the subprotocol `v1`, and a 400 "Invalid request" for each refusal. One
//! stand-in checks the token's signature with the old key, as 9.1 to 10.0
//! do; one answers as 9 does, and one as a server that is no wstunnel. Each
//! takes a frame's payload without its mask, as fastwebsockets 0.10 does
//! under wstunnel's default, unless it is told to apply it
//! (`--websocket-mask-frame`). The target sends a version line and echoes.
//! A pipe's end of input sends no Close until the server's end, so that 1 MiB
//! still on its way comes whole. The stand-ins are the crate's own
//! (`pod_egress::standin::wstunnel`, feature `standin`). The run against
//! wstunnel's own server is podssh's `scripts/interop-wstunnel.sh`, which
//! drives its binary.

#[path = "common/talker.rs"]
mod talker;
#[path = "common/way.rs"]
mod way;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pod_egress::standin::wstunnel::{Answer, Stand, Wst};
use pod_egress::standin::{BANNER, Echo};
use pod_egress::wstunnel::{self, AtEnd, Dial, Ended, Secret, Server, WstunnelError};
use pod_tls::Trust;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// Shared test support; this file uses part of it.
#[allow(dead_code)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

const NAME: &str = "wst.wstunnel.test";
const LIMIT: Duration = Duration::from_secs(20);

/// A server of 11 with its defaults.
fn v11() -> Wst {
    Wst::v11()
}

/// The target: its version line, then an echo.
async fn target() -> Echo {
    pod_egress::standin::echo().await.unwrap()
}

/// A stand-in on the loopback, over TLS for [`NAME`] when `tls`.
async fn stand_in(wst: Wst, tls: bool) -> Stand {
    let config = tls.then(|| Arc::clone(cert::server_for(NAME).unwrap().0.config()));
    pod_egress::standin::wstunnel::start(wst, config).await.unwrap()
}

fn dial() -> Dial {
    Dial {
        way: way::loopback(&[NAME, "other.wstunnel.test"]),
        trust: Trust::Default,
        timeout: LIMIT,
        at_end: AtEnd::Now,
    }
}

async fn open(server: &Server, secret: &Secret, aim: u16) -> Result<wstunnel::Carried, WstunnelError> {
    tokio::time::timeout(LIMIT * 2, wstunnel::open(server, secret, "127.0.0.1", aim, &dial()))
        .await
        .expect("an answer within the limit")
}

/// The version line, then 200 KiB echoed, through `carried`.
async fn carries(mut carried: wstunnel::Carried) {
    let sent: Vec<u8> = (0..200 * 1024).map(|i| (i % 251) as u8).collect();
    let mut banner = vec![0u8; BANNER.len()];
    let mut back = vec![0u8; sent.len()];
    let both = async {
        carried.stream.read_exact(&mut banner).await.unwrap();
        carried.stream.write_all(&sent).await.unwrap();
        carried.stream.read_exact(&mut back).await.unwrap();
    };
    tokio::time::timeout(LIMIT, both).await.expect("the bytes within the limit");
    assert_eq!(banner, BANNER);
    assert!(back == sent, "the bytes came back changed");
}

fn plain(port: u16) -> Server {
    Server::parse(&format!("ws://127.0.0.1:{port}")).unwrap()
}

/// A server of 11 (no signature checked) and one of 10.0 (the old key's
/// signature checked) take the token, each frame's payload is read as it
/// came, and the bytes come back as they went; so they do where the server
/// applies the mask. Each frame bears the mask bit with a key of zero.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_banner_and_the_bytes_come_back_through_each_server_of_9_1_to_11() {
    pod_testkit::need_loopback!();
    let echo = target().await;
    let aim = echo.port;
    for wst in [v11(), Wst { checks_signature: true, ..v11() }, Wst { unmask: true, ..v11() }] {
        let stand = stand_in(wst.clone(), false).await;
        carries(open(&plain(stand.port), &Secret::default(), aim).await.unwrap()).await;
        let seen = stand.seen.lock().unwrap();
        let asked = &seen.requests[0];
        assert_eq!((asked.path.as_str(), asked.authorization.as_deref()), ("/v1/events", None));
        assert!(asked.protocol.starts_with("v1, authorization.bearer.ey"), "{}", asked.protocol);
        assert_eq!(seen.targets, vec![("127.0.0.1".to_string(), aim)]);
        assert!(!seen.keys.is_empty() && seen.keys.iter().all(|k| *k == Some([0; 4])), "{:?}", seen.keys);
    }
}

/// The prefix of the secret file goes in the path and its credentials in
/// `Authorization`; a wrong prefix, or none, is refused by the server, and
/// the refusal says so in the words of its version, with no secret in them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_secret_goes_in_the_path_and_a_wrong_one_is_refused_in_its_words() {
    pod_testkit::need_loopback!();
    let echo = target().await;
    let aim = echo.port;
    let restrict = Some("s3cr3t".to_string());
    let wst = Wst { restrict, authorization: Some("Basic YWxpY2U6cHc=".to_string()), ..v11() };
    let stand = stand_in(wst, false).await;
    let port = stand.port;
    let secret = Secret::parse("path-prefix s3cr3t\ncredentials alice:pw\n").unwrap();
    carries(open(&plain(port), &secret, aim).await.unwrap()).await;
    let asked = stand.seen.lock().unwrap().requests[0].clone();
    let want = ("/s3cr3t/events", Some("Basic YWxpY2U6cHc="));
    assert_eq!((asked.path.as_str(), asked.authorization.as_deref()), want);
    for wrong in ["path-prefix other\ncredentials alice:pw", "path-prefix s3cr3t", "v1"] {
        let why = open(&plain(port), &Secret::parse(wrong).unwrap(), aim).await.expect_err("a refusal");
        let line = why.line(&plain(port).shown());
        let words = format!("the wstunnel server ws://127.0.0.1:{port} refused the tunnel: 400 Invalid request; ");
        assert!(line.starts_with(&words) && line.contains("the words of wstunnel 10 and later"), "{line}");
        assert!(!line.contains("s3cr3t") && !line.contains("YWxp") && !format!("{why:?}").contains("s3cr3t"));
    }
}

/// A server of 9 refuses in its own words, which name the versions that
/// cannot read the token; a 101 without `v1` is no wstunnel server.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_version_and_another_server_are_said_in_their_words() {
    pod_testkit::need_loopback!();
    let echo = target().await;
    let aim = echo.port;
    let nine = stand_in(Wst { answer: Answer::Refuse9, ..v11() }, false).await;
    match open(&plain(nine.port), &Secret::default(), aim).await.expect_err("a refusal") {
        WstunnelError::Refused { status: 400, words } => {
            assert!(words.starts_with("Invalid upgrade request; the words of wstunnel 9 and before"), "{words}");
            assert!(words.contains("9.0 or before"), "{words}");
        }
        other => panic!("{other:?}"),
    }
    let other = stand_in(Wst { answer: Answer::NoSubprotocol, ..v11() }, false).await;
    let why = open(&plain(other.port), &Secret::default(), aim).await.expect_err("a refusal");
    let line = why.line(&plain(other.port).shown());
    assert!(line.contains("is no wstunnel server that podssh speaks to") && line.contains("9.1 to 11"), "{line}");
}

/// A pipe's end of input: this side's input ends at once, while the
/// target's 1 MiB is still on its way. A Close is no half-close, and this
/// stand-in answers one at once and sends nothing after it; so the pipe
/// sends its Close only after the server's, and each byte comes. Planted, a
/// Close at the first end, the rest is lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pipe_s_end_of_input_waits_for_the_server_s_end_and_loses_no_byte() {
    pod_testkit::need_loopback!();
    let aim = talker::talker().await;
    let stand = stand_in(Wst { answers_close: true, ..v11() }, false).await;
    let pipe = Dial { at_end: AtEnd::AfterPeer, ..dial() };
    let (server, secret) = (plain(stand.port), Secret::default());
    let opened = wstunnel::open(&server, &secret, "127.0.0.1", aim, &pipe);
    let carried = tokio::time::timeout(LIMIT * 2, opened).await.expect("an answer within the limit").unwrap();
    let mut stream = carried.stream;
    stream.shutdown().await.unwrap();
    let got = talker::to_end(&mut stream).await;
    talker::assert_whole(&got, "wstunnel");
    assert_eq!(carried.status.get(), Some(Ended::Closed { code: Some(1000), reason: String::new() }));
}

fn root_file(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-egress-wstunnel-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("root.pem");
    let der = cert::encoding::self_signed_inner(&cert::encoding::anchor_name(NAME), true).der;
    std::fs::write(&path, cert::encoding::pem(&der)).unwrap();
    path
}

/// Over `wss://`, TLS is checked for the server's name with the test's
/// root, and the tunnel carries the session; another name is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wss_checks_the_server_s_name_and_carries_the_session() {
    pod_testkit::need_loopback!();
    let (root, echo) = (root_file("tls"), target().await);
    let aim = echo.port;
    let stand = stand_in(Wst { restrict: Some("p".to_string()), ..v11() }, true).await;
    let port = stand.port;
    let trust = Trust::File(root.clone());
    let secret = Secret::parse("p").unwrap();
    let dial = Dial { trust, ..dial() };
    let server = Server::parse(&format!("wss://{NAME}:{port}")).unwrap();
    let opened = tokio::time::timeout(LIMIT * 2, wstunnel::open(&server, &secret, "127.0.0.1", aim, &dial)).await;
    carries(opened.expect("an answer within the limit").unwrap()).await;
    let other = Server::parse(&format!("wss://other.wstunnel.test:{port}")).unwrap();
    let why = wstunnel::open(&other, &secret, "127.0.0.1", aim, &dial).await.expect_err("a refusal");
    assert!(why.line(&other.shown()).starts_with("TLS with the wstunnel server wss://other"), "{why:?}");
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// A tunnel is a dialer: a dial of a target is a tunnel to it; a refusal is
/// a failure of the WebSocket's stage, of the class refused, in the line
/// that names the server, with no secret in it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tunnel_is_a_dialer_and_a_refusal_is_a_typed_failure() {
    pod_testkit::need_loopback!();
    let echo = target().await;
    let aim = echo.port;
    let stand = stand_in(Wst { restrict: Some("s3cr3t".to_string()), ..v11() }, false).await;
    let port = stand.port;
    let secret = Secret::parse("s3cr3t").unwrap();
    let tunnel = wstunnel::Tunnel { server: plain(port), secret, dial: dial() };
    let dialer: &dyn pod_core::Dial = &tunnel;
    assert!(dialer.names_inside());
    let deadline = tokio::time::Instant::now() + LIMIT;
    let target = pod_core::Target::new("127.0.0.1", aim);
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
    let wrong = wstunnel::Tunnel { secret: Secret::default(), ..tunnel.clone() };
    let failure = (&wrong as &dyn pod_core::Dial).dial(&target, deadline).await.expect_err("a refusal");
    assert_eq!((failure.stage, failure.class), (pod_core::Stage::WebSocket, pod_core::Class::Refused), "{failure}");
    let words = format!("the wstunnel server ws://127.0.0.1:{port} refused the tunnel: 400 Invalid request");
    assert!(failure.text().starts_with(&words) && !failure.text().contains("s3cr3t"), "{failure}");
}
