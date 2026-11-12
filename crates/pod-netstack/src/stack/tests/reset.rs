//! The end of each socket at once: a reset that the consumer asks for, a
//! shutdown, and a panic of the reactor. The app reads the cause as
//! `ConnectionReset`, not a clean end, and does not wait for ever.

use super::*;

const LIMIT: Duration = Duration::from_secs(5);

/// A connected pair of streams in `net` on `port`, after one exchange.
async fn pair_on(net: &NetStack, port: u16) -> (TcpListener, TcpStream, TcpStream) {
    let addr = at(LOCAL_IP, port);
    let mut listener = net.listen(addr).await.unwrap();
    let mut client = net.connect(addr).await.unwrap();
    let mut server = tokio::time::timeout(LIMIT, listener.accept()).await.expect("no accept").unwrap();
    client.write_all(b"ping").await.unwrap();
    let mut got = [0u8; 4];
    tokio::time::timeout(LIMIT, server.read_exact(&mut got)).await.expect("no data").unwrap();
    (listener, client, server)
}

/// The error of `result`, which must be `ConnectionReset` with `why` in it.
fn reset_with<T: std::fmt::Debug>(result: io::Result<T>, why: &str) {
    let err = result.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset, "{err}");
    assert!(err.to_string().contains(why), "{err}");
}

/// A reset of the consumer ends each open stream and each UDP socket at
/// once, with its reason: a read that waits, a write and a UDP receive that
/// waits. The listener stays, and a new connection works.
#[tokio::test]
async fn a_reset_fails_each_stream_and_each_udp_socket() {
    let mut net = NetStack::loopback();
    let (mut listener, mut client, mut server) = pair_on(&net, 8460).await;
    let reading = tokio::spawn(async move {
        let mut byte = [0u8; 1];
        let read = client.read(&mut byte).await;
        (read, client)
    });
    let mut udp = net.bind_udp(at(LOCAL_IP, 40460)).await.unwrap();
    let receiving = tokio::spawn(async move {
        let mut buf = [0u8; 16];
        let got = udp.recv_from(&mut buf).await;
        (got, udp)
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    net.reset_all("the link is dead");
    let (read, mut client) = tokio::time::timeout(LIMIT, reading).await.expect("the read waits").unwrap();
    reset_with(read, "the link is dead");
    reset_with(client.write_all(b"more").await, "the link is dead");
    reset_with(server.read(&mut [0u8; 1]).await, "the link is dead");
    let (got, udp) = tokio::time::timeout(LIMIT, receiving).await.expect("the receive waits").unwrap();
    reset_with(got, "the link is dead");
    reset_with(udp.send_to(b"x", at(LOCAL_IP, 9)).await, "the link is dead");
    // A socket after the reset works.
    let addr = at(LOCAL_IP, 8460);
    let mut again = net.connect(addr).await.unwrap();
    let mut accepted = tokio::time::timeout(LIMIT, listener.accept()).await.expect("no accept").unwrap();
    again.write_all(b"new").await.unwrap();
    let mut got = [0u8; 3];
    tokio::time::timeout(LIMIT, accepted.read_exact(&mut got)).await.expect("no data").unwrap();
    assert_eq!(&got, b"new");
    net.shutdown().await;
}

/// A write that waits for room in the channel from the app fails at a
/// reset: the stack wakes it.
#[tokio::test]
async fn a_write_that_waits_fails_at_a_reset() {
    let mut net = NetStack::loopback();
    let (_listener, mut client, _server) = pair_on(&net, 8461).await;
    // The server reads nothing: its window and the channels fill, and the
    // write waits. A write of the stream is one chunk of the channel.
    let writing = tokio::spawn(async move {
        for _ in 0..2048 {
            client.write_all(&[1u8; 4096]).await?;
        }
        io::Result::Ok(())
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!writing.is_finished(), "the write must wait for the reader");
    net.reset_all("the session is closed");
    let wrote = tokio::time::timeout(LIMIT, writing).await.expect("the write waits for ever").unwrap();
    reset_with(wrote, "the session is closed");
    net.shutdown().await;
}

/// A shutdown ends each open stream and each UDP socket with its cause,
/// and the stack gives it.
#[tokio::test]
async fn a_shutdown_fails_each_socket_with_its_cause() {
    let mut net = NetStack::loopback();
    let (_listener, mut client, _server) = pair_on(&net, 8462).await;
    let mut udp = net.bind_udp(at(LOCAL_IP, 40462)).await.unwrap();
    assert_eq!(net.stop_reason(), None);
    let stopped = net.stopped();
    net.shutdown().await;
    let why = tokio::time::timeout(LIMIT, stopped).await.expect("no stop");
    assert_eq!(why, "the stack is shut down");
    assert_eq!(net.stop_reason().as_deref(), Some("the stack is shut down"));
    reset_with(client.read(&mut [0u8; 1]).await, "shut down");
    reset_with(udp.recv_from(&mut [0u8; 1]).await, "shut down");
}

/// A gateway service that panics: the reactor stops.
struct Panics;

impl GatewayService for Panics {
    fn port(&self) -> u16 {
        53
    }

    fn answer(&self, _from: SocketAddr, _request: &[u8]) -> Option<Vec<u8>> {
        panic!("a test panic of the reactor")
    }
}

/// A panic of the reactor ends each open stream and each UDP socket with a
/// cause; the stack gives the cause, and a new command fails with it.
#[tokio::test]
async fn a_panic_of_the_reactor_fails_each_socket() {
    let config = StackConfig { gateway_service: Some(Arc::new(Panics)), ..StackConfig::default() };
    let net = NetStack::loopback_with(config);
    let (_listener, mut client, _server) = pair_on(&net, 8463).await;
    let mut udp = net.bind_udp(at(LOCAL_IP, 40463)).await.unwrap();
    let stopped = net.stopped();
    udp.send_to(b"boom", at("10.66.0.1".parse().unwrap(), 53)).await.unwrap();
    let why = tokio::time::timeout(LIMIT, stopped).await.expect("no stop");
    assert!(why.contains("a panic"), "{why}");
    assert_eq!(net.stop_reason().as_deref(), Some(why.as_str()));
    reset_with(client.read(&mut [0u8; 1]).await, "a panic");
    reset_with(udp.recv_from(&mut [0u8; 1]).await, "a panic");
    let err = net.connect(at(LOCAL_IP, 8463)).await.unwrap_err();
    assert!(err.to_string().contains("a panic"), "{err}");
}
