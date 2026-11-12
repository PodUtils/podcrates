//! The TCP timeout, the keepalive and no Nagle of each socket (T-393, from
//! podnet's WARP stack, issue 125 and podssh): the settings on a socket, a
//! connect and a stream to a peer that is gone end with `TimedOut`, an idle
//! stream to a live peer stays, and a RST of the peer stays a reset.

use super::*;

const LIMIT: Duration = Duration::from_secs(5);

/// The settings of a stack at `10.66.0.LAST` with a TCP timeout and a
/// keepalive.
fn timed(last: u8, timeout: Duration, keepalive: Duration) -> StackConfig {
    StackConfig {
        local_v4: Ipv4Addr::new(10, 66, 0, last),
        tcp_timeout: Some(timeout),
        tcp_keepalive: Some(keepalive),
        ..StackConfig::default()
    }
}

/// podnet issue 125: each TCP socket takes the timeout and the keepalive of
/// the settings; none by default.
#[test]
fn a_tcp_socket_has_a_timeout_and_a_keepalive() {
    let config = timed(2, Duration::from_secs(60), Duration::from_secs(20));
    let socket = new_tcp_socket(&config);
    assert_eq!(socket.timeout(), Some(SmolDuration::from_secs(60)));
    assert_eq!(socket.keep_alive(), Some(SmolDuration::from_secs(20)));
    let socket = new_tcp_socket(&StackConfig::default());
    assert_eq!((socket.timeout(), socket.keep_alive()), (None, None));
}

/// podssh: with `nodelay`, the TCP sockets of the stack have no Nagle, and
/// each connected and each accepted stream says so; Nagle is on by default.
#[tokio::test]
async fn the_tcp_sockets_have_no_nagle() {
    let config = StackConfig { nodelay: true, ..StackConfig::default() };
    assert!(!new_tcp_socket(&config).nagle_enabled());
    assert!(new_tcp_socket(&StackConfig::default()).nagle_enabled());
    let mut net = NetStack::loopback_with(config);
    let addr = at(LOCAL_IP, 8470);
    let mut listener = net.listen(addr).await.unwrap();
    let client = net.connect(addr).await.unwrap();
    let server = tokio::time::timeout(LIMIT, listener.accept()).await.expect("no accept").unwrap();
    assert!(client.nodelay() && server.nodelay());
    net.shutdown().await;
}

/// The error of a read or a handshake that must end with `TimedOut`.
fn timed_out<T: std::fmt::Debug>(result: io::Result<T>) {
    let err = result.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}");
    assert!(err.to_string().contains("TCP timeout"), "{err}");
}

/// A connect to a peer that never answers ends with `TimedOut` after the
/// timeout, not a wait for ever, and the stack frees its socket.
#[tokio::test]
async fn a_connect_with_no_answer_ends_at_the_timeout() {
    let timeout = Duration::from_secs(1);
    // The link goes nowhere: no packet comes back.
    let mut net = NetStack::over_link(timed(2, timeout, Duration::from_millis(250)));
    let start = std::time::Instant::now();
    let mut stream = net.connect(at(IpAddr::V4(Ipv4Addr::new(10, 66, 0, 9)), 80)).await.unwrap();
    let limit = pod_testkit::host::scaled(LIMIT);
    timed_out(tokio::time::timeout(limit, stream.established()).await.expect("the connect waits for ever"));
    assert!(start.elapsed() >= timeout - Duration::from_millis(100), "{:?}", start.elapsed());
    assert_eq!(settle(&net, 0).await, 0, "the socket stays");
    net.shutdown().await;
}

/// A connected pair of streams over two stacks with `config`, after one
/// exchange: the client at `10.66.0.3`, the server at `10.66.0.2`.
async fn linked(
    port: u16,
    config: fn(u8) -> StackConfig,
) -> (NetStack, NetStack, Vec<tokio::task::JoinHandle<()>>, TcpStream, TcpStream) {
    let (server_net, client_net, pumps) = pair(config(2), config(3), |_| {});
    let addr = at(LOCAL_IP, port);
    let mut listener = server_net.listen(addr).await.unwrap();
    let mut client = client_net.connect(addr).await.unwrap();
    let mut server = tokio::time::timeout(LIMIT, listener.accept()).await.expect("no accept").unwrap();
    client.write_all(b"ping").await.unwrap();
    let mut got = [0u8; 4];
    tokio::time::timeout(LIMIT, server.read_exact(&mut got)).await.expect("no data").unwrap();
    (server_net, client_net, pumps, client, server)
}

/// A stream whose peer is gone (the link carries nothing) ends with
/// `TimedOut` at each end after the keepalive and the timeout: its reads do
/// not wait for ever.
#[tokio::test]
async fn a_stream_whose_peer_is_gone_ends_at_the_timeout() {
    let config = |last| timed(last, Duration::from_secs(1), Duration::from_millis(250));
    let (mut server_net, mut client_net, pumps, mut client, mut server) = linked(8471, config).await;
    for pump in pumps {
        pump.abort();
    }
    let limit = pod_testkit::host::scaled(LIMIT);
    let read = tokio::time::timeout(limit, client.read(&mut [0u8; 1])).await;
    timed_out(read.expect("the read of the client waits for ever"));
    let read = tokio::time::timeout(limit, server.read(&mut [0u8; 1])).await;
    timed_out(read.expect("the read of the server waits for ever"));
    assert_eq!(settle(&client_net, 0).await, 0, "the socket of the client stays");
    server_net.shutdown().await;
    client_net.shutdown().await;
}

/// The keepalive holds an idle stream to a live peer past the timeout:
/// each answer of the peer starts the timeout again.
#[tokio::test]
async fn an_idle_stream_to_a_live_peer_outlives_the_timeout() {
    let config = |last| timed(last, Duration::from_millis(600), Duration::from_millis(150));
    let (mut server_net, mut client_net, pumps, mut client, mut server) = linked(8472, config).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    client.write_all(b"late").await.unwrap();
    let mut got = [0u8; 4];
    tokio::time::timeout(LIMIT, server.read_exact(&mut got)).await.expect("no data").unwrap();
    assert_eq!(&got, b"late");
    for pump in pumps {
        pump.abort();
    }
    server_net.shutdown().await;
    client_net.shutdown().await;
}

/// With a timeout, a RST of the peer stays a reset: an abort of the server
/// is `ConnectionReset`, and a connect to a port with no listener is
/// refused at once.
#[tokio::test]
async fn a_reset_of_the_peer_is_no_timeout() {
    let config = |last| timed(last, Duration::from_secs(30), Duration::from_secs(10));
    let (mut server_net, mut client_net, pumps, mut client, server) = linked(8473, config).await;
    server.abort();
    let read = tokio::time::timeout(LIMIT, client.read(&mut [0u8; 1])).await.expect("no reset");
    let err = read.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset, "{err}");
    let mut refused = client_net.connect(at(LOCAL_IP, 81)).await.unwrap();
    let err = tokio::time::timeout(LIMIT, refused.established()).await.expect("no answer").unwrap_err();
    assert!(matches!(err.kind(), io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionReset), "{err}");
    for pump in pumps {
        pump.abort();
    }
    server_net.shutdown().await;
    client_net.shutdown().await;
}
