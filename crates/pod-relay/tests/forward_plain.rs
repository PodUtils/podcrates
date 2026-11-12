//! The forward opener over plain `ws://` to stand-in relays on the loopback
//! (T-234, feature `plain-ws`), as podnet's tests of its forward road run
//! it: a session through a stand-in relay to an echo server, a refused
//! token, the next host after a dead one, and the refusals of the plain
//! wire. The names of podnet's tests are kept where a test here takes one
//! of them.
#![cfg(feature = "plain-ws")]

mod support;

use std::net::SocketAddr;
use std::time::Duration;

use pod_relay::failover::Mode;
use pod_relay::open::{Failure, OpenError, Opened};
use pod_relay::relay::{RelayList, forward_path, parse_relay_list, select_relays};
use pod_relay::{Context, Relay, Request, Settings, open_on_loopback, stream};
use pod_ws::frame::OPCODE_BINARY;
use pod_ws::{ConnectError, Trust};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The made-up token that the stand-in takes, and another.
const GOOD: &str = "testonly-good-token-0001";
const BAD: &str = "testonly-bad-token-00001";
const LIMIT: Duration = Duration::from_secs(20);

/// A context with `token` as the given relay token.
fn with_token(token: Option<&str>) -> Context {
    support::context().with_settings(Settings { token: token.map(str::to_string), ..Settings::default() })
}

/// A forward session to `target` through `relays` on the plain wire.
async fn open(ctx: &Context, relays: &RelayList, target: SocketAddr) -> Result<Opened, Failure> {
    let path = forward_path(&target.ip().to_string(), target.port()).unwrap();
    let shown = target.to_string();
    let trust = Trust::Default;
    let request =
        Request { ctx, relays, path: &path, trust: &trust, target: &shown, rounds: 1, failover: Mode::default() };
    tokio::time::timeout(LIMIT, open_on_loopback(&request, &mut |_| {})).await.expect("an answer in time")
}

/// A server that echoes each byte, on the loopback.
async fn echo_server() -> SocketAddr {
    let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = echo.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = echo.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = socket.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    at
}

/// A stand-in relay on the loopback: it takes the upgrade at the forward
/// path of `target` with the token [`GOOD`], opens TCP to `target` and copies
/// the bytes both ways, with an empty message after each of the client's, as
/// the relay's keepalive. Any other upgrade gets `403`. Its port.
async fn stand_in(target: SocketAddr) -> u16 {
    let relay = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = relay.local_addr().unwrap().port();
    let expected = format!("/connect/{}/{}", target.ip(), target.port());
    tokio::spawn(async move {
        while let Ok((socket, _)) = relay.accept().await {
            tokio::spawn(serve(socket, expected.clone(), target));
        }
    });
    port
}

async fn serve(mut socket: TcpStream, expected: String, target: SocketAddr) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if socket.read_exact(&mut byte).await.is_err() {
            return;
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).to_string();
    let header = |name: &str| {
        head.lines()
            .find_map(|l| l.split_once(':').filter(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim()))
    };
    let path = head.split(' ').nth(1);
    let (Some(key), true, Some(GOOD)) = (header("sec-websocket-key"), path == Some(&expected), header("x-relay-token"))
    else {
        let body = "missing or wrong token";
        let answer =
            format!("HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let _ = socket.write_all(answer.as_bytes()).await;
        return;
    };
    socket.write_all(&pod_ws::server::switching(key)).await.unwrap();
    let session = pod_ws::server::session(socket, Vec::new(), None);
    let (mut tcp_r, mut tcp_w) = TcpStream::connect(target).await.unwrap().into_split();
    let session = &session;
    let to_target = async {
        while let Ok(message) = session.read_frame().await {
            if message.opcode != OPCODE_BINARY || session.send_binary(&[]).await.is_err() {
                break;
            }
            if tcp_w.write_all(&message.payload).await.is_err() {
                break;
            }
        }
    };
    let from_target = async {
        let mut buf = vec![0u8; 64 * 1024];
        while let Ok(n @ 1..) = tcp_r.read(&mut buf).await {
            if session.send_binary(&buf[..n]).await.is_err() {
                break;
            }
        }
    };
    tokio::select! {
        _ = to_target => {}
        _ = from_target => {}
    }
}

/// A port on the loopback where nothing listens.
async fn dead_port() -> u16 {
    let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
    dead.local_addr().unwrap().port()
}

/// podnet's test: a relay on the loopback takes the WebSocket at
/// /connect/IP/PORT with the right token, opens TCP to an echo server, and
/// copies the bytes. 1 MiB goes there and back. A wrong token is refused
/// with 403 first, and the client mints no token (it is given). A list
/// whose first host does not answer goes to the next one, and a host named
/// twice is one host.
#[tokio::test]
async fn bytes_go_through_a_local_relay() {
    pod_testkit::need_loopback!();
    let echo = echo_server().await;
    let url = format!("http://127.0.0.1:{}", stand_in(echo).await);
    let relays = select_relays(Some(&url), None, &[]).unwrap();

    let failure = open(&with_token(Some(BAD)), &relays, echo).await.expect_err("a wrong token is refused");
    assert!(matches!(failure.last(), OpenError::Connect { error: ConnectError::Refused { status: 403, .. }, .. }));
    let lines = failure.lines(&echo.to_string()).join("\n");
    assert!(lines.contains("403") && !lines.contains(BAD), "{lines}");

    let opened = open(&with_token(Some(GOOD)), &relays, echo).await.expect("a session");
    assert_eq!(opened.relay, relays.hosts[0]);
    let (mut r, mut w) = tokio::io::split(stream::of(opened.session));
    let data: Vec<u8> = (0..(1u32 << 20)).map(|i| (i * 7 % 251) as u8).collect();
    let sent = data.clone();
    let writer = tokio::spawn(async move {
        w.write_all(&sent).await.unwrap();
        w
    });
    let mut got = vec![0u8; data.len()];
    tokio::time::timeout(LIMIT, r.read_exact(&mut got)).await.expect("the echo in time").unwrap();
    assert!(got == data, "the bytes changed on the way");
    drop(writer.await.unwrap());

    // A list: the first host does not answer, the next one carries the
    // session.
    let list = select_relays(Some(&format!("http://127.0.0.1:{},{url}", dead_port().await)), None, &[]).unwrap();
    assert_eq!(list.hosts.len(), 2);
    let opened = open(&with_token(Some(GOOD)), &list, echo).await.expect("the second host carries the session");
    assert_eq!(opened.relay, list.hosts[1]);
    let mut io = stream::of(opened.session);
    io.write_all(b"ping").await.unwrap();
    let mut back = [0u8; 4];
    tokio::time::timeout(LIMIT, io.read_exact(&mut back)).await.expect("the echo in time").unwrap();
    assert_eq!(&back, b"ping");

    assert_eq!(parse_relay_list(&format!("{url},{url}/")).unwrap().len(), 1, "the same host once");
}

/// The plain wire goes to the loopback only: a host elsewhere is refused
/// before any connection, with the token of the settings, so no token leaves
/// this host in clear.
#[tokio::test]
async fn a_relay_off_the_loopback_is_refused_before_any_connection() {
    let relays = RelayList { hosts: vec![Relay { host: "relay.example".into(), port: 80 }], explicit: true };
    let target: SocketAddr = "192.0.2.1:22".parse().unwrap();
    let failure = open(&with_token(Some(GOOD)), &relays, target).await.expect_err("refused");
    let OpenError::Connect { error: ConnectError::Config(why), .. } = failure.last() else {
        panic!("{:?}", failure.last());
    };
    assert!(why.contains("loopback"), "{why}");
}

/// A stand-in relay mints no token: with none given, kept by the run or in
/// the cache, the attempt ends at its token, before any connection.
#[tokio::test]
async fn the_plain_wire_takes_a_given_token_and_mints_none() {
    let relays = RelayList { hosts: vec![Relay { host: "127.0.0.1".into(), port: 9 }], explicit: true };
    let target: SocketAddr = "127.0.0.1:22".parse().unwrap();
    let failure = open(&with_token(None), &relays, target).await.expect_err("no token");
    let OpenError::Token(pod_relay::token::TokenError::Connect(ConnectError::Config(why))) = failure.last() else {
        panic!("{:?}", failure.last());
    };
    assert!(why.contains("token of the settings"), "{why}");
}
