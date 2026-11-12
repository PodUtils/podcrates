//! The relay against the stand-in on the loopback: the WebSocket with its
//! subprotocol and the connect token, SSH with the user `tunnel` and no
//! authentication, a `direct-tcpip` channel with an echo, a far end that
//! closes, the host key, a refused port.

mod common;

use pod_core::Class;
use pod_devtunnel::standin::{Keys, Port, Standin};
use pod_devtunnel::{Locator, Relay, Step, step_of};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use common::{connected, plan, report, settings, user, way};

#[tokio::test]
async fn a_stream_to_a_port_of_the_host_echoes_through_the_relay() {
    pod_testkit::need_loopback!();
    let (standin, relay, kept) = connected(plan()).await;
    let mut stream = relay.open(22).await.unwrap_or_else(|e| panic!("open: {}", e.chain()));
    stream.write_all(b"hello through the relay").await.unwrap();
    let mut back = [0u8; 23];
    stream.read_exact(&mut back).await.unwrap();
    assert_eq!(&back, b"hello through the relay");
    // Our EOF: the host ends the channel, and the read ends with no error.
    stream.shutdown().await.unwrap();
    let mut rest = Vec::new();
    assert_eq!(stream.read_to_end(&mut rest).await.unwrap(), 0);

    let seen = standin.seen();
    assert_eq!(seen.subprotocols, ["tunnel-relay-client"]);
    assert_eq!(seen.relay_tokens, ["fresh"]);
    assert_eq!(seen.users, ["tunnel"]);
    assert_eq!(seen.channels, [("127.0.0.1".to_string(), 22, "127.0.0.1".to_string(), 0)]);
    assert_eq!(relay.sessions(), 1);
    assert_eq!(relay.tunnel().name.as_deref(), Some("standin-box"));
    assert_eq!(kept.kinds(), ["devtunnel.connected"]);
    assert!(kept.texts()[0].contains("standin-box"), "{:?}", kept.texts());
}

#[tokio::test]
async fn many_streams_share_one_session() {
    pod_testkit::need_loopback!();
    let (standin, relay, _) = connected(plan()).await;
    let mut streams = Vec::new();
    for n in 0..4u8 {
        let mut stream = relay.open(22).await.unwrap();
        stream.write_all(&[n; 3]).await.unwrap();
        streams.push(stream);
    }
    for (n, stream) in streams.iter_mut().enumerate() {
        let mut back = [0u8; 3];
        stream.read_exact(&mut back).await.unwrap();
        assert_eq!(back, [n as u8; 3]);
    }
    assert_eq!(standin.seen().channels.len(), 4);
    assert_eq!(standin.minted(), 1);
}

#[tokio::test]
async fn a_stream_that_the_far_end_closed_ends_once_and_is_not_opened_again() {
    pod_testkit::need_loopback!();
    let mut plan = plan();
    plan.ports.push((8080, Port::EchoOnce));
    let (standin, relay, _) = connected(plan).await;
    let mut stream = relay.open(8080).await.unwrap();
    stream.write_all(b"one").await.unwrap();
    let mut got = Vec::new();
    stream.read_to_end(&mut got).await.unwrap();
    assert_eq!(got, b"one");
    // The end stays an end.
    assert_eq!(stream.read(&mut [0u8; 8]).await.unwrap(), 0);
    // No reconnect and no new channel: the session is the same, and the
    // next stream is a new one on it.
    assert_eq!((relay.sessions(), standin.minted(), standin.seen().channels.len()), (1, 1, 1));
    let mut again = relay.open(22).await.unwrap();
    again.write_all(b"x").await.unwrap();
    let mut back = [0u8; 1];
    again.read_exact(&mut back).await.unwrap();
    assert_eq!((relay.sessions(), standin.minted()), (1, 1));
}

#[tokio::test]
async fn a_port_that_the_host_refuses_is_a_channel_failure_and_the_session_stays() {
    pod_testkit::need_loopback!();
    let (standin, relay, _) = connected(plan()).await;
    let failure = relay.open(9).await.unwrap_err();
    assert_eq!(step_of(&failure), Some(Step::Channel));
    assert_eq!(failure.class, Class::Unavailable);
    assert!(failure.chain().contains("port 9"), "{}", failure.chain());
    assert!(relay.open(22).await.is_ok());
    assert_eq!((relay.sessions(), standin.minted()), (1, 1));
}

async fn connect_failure(keys: Keys) -> pod_core::Failure {
    let mut plan = plan();
    plan.keys = keys;
    let standin = Standin::start(plan).await.unwrap();
    let (report, _) = report();
    let locator = Locator::parse("standin-box").unwrap();
    Relay::connect(way(), settings(&standin.base()), locator, user(), report).await.unwrap_err()
}

#[tokio::test]
async fn a_host_key_that_is_not_the_tunnels_ends_the_session() {
    pod_testkit::need_loopback!();
    for keys in [Keys::Other, Keys::Empty] {
        let failure = connect_failure(keys).await;
        assert_eq!(step_of(&failure), Some(Step::Ssh), "{keys:?}: {}", failure.chain());
        assert_eq!(failure.class, Class::Refused, "{keys:?}");
    }
}

#[tokio::test]
async fn a_refused_upgrade_is_a_websocket_failure() {
    pod_testkit::need_loopback!();
    let standin = Standin::start(plan()).await.unwrap();
    standin.refuse_upgrades(1);
    let (report, _) = report();
    let locator = Locator::parse("standin-box").unwrap();
    let failure = Relay::connect(way(), settings(&standin.base()), locator, user(), report).await.unwrap_err();
    assert_eq!(step_of(&failure), Some(Step::WebSocket), "{}", failure.chain());
    assert_eq!(failure.class, Class::TempFail);
    assert_eq!(standin.seen().subprotocols, ["tunnel-relay-client"]);
}

#[tokio::test]
async fn a_tunnel_with_no_host_is_unavailable() {
    pod_testkit::need_loopback!();
    let mut plan = plan();
    plan.no_host = true;
    let standin = Standin::start(plan).await.unwrap();
    let (report, _) = report();
    let locator = Locator::parse("standin-box").unwrap();
    let failure = Relay::connect(way(), settings(&standin.base()), locator, user(), report).await.unwrap_err();
    assert_eq!((step_of(&failure), failure.class), (Some(Step::Management), Class::Unavailable));
    assert!(standin.seen().subprotocols.is_empty());
}

#[tokio::test]
async fn a_port_forward_reaches_a_server_of_the_test() {
    pod_testkit::need_loopback!();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let to = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        tcp.write_all(b"SSH-2.0-a-server-of-the-test\r\n").await.unwrap();
    });
    let mut plan = plan();
    plan.ports = vec![(2222, Port::Forward(to))];
    let (_standin, relay, _) = connected(plan).await;
    let mut stream = relay.open(2222).await.unwrap();
    let mut banner = Vec::new();
    stream.read_to_end(&mut banner).await.unwrap();
    assert_eq!(banner, b"SSH-2.0-a-server-of-the-test\r\n");
}
