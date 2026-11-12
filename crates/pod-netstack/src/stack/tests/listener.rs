//! The close of a listener (T-230, PodUtils/podnet#457): its drop or its
//! close frees its listen sockets, its port and its address; the streams
//! that it accepted stay.

use super::*;

/// Listeners on 1,000 ports, one after the other, each dropped or closed:
/// the live count goes back after each, and a listen on the port again
/// works. podnet opens one listener on any address for each port that a
/// program dials.
#[tokio::test]
async fn dropped_listeners_free_their_sockets_and_their_ports() {
    let mut net = NetStack::loopback_with(StackConfig { any_ip: true, ..StackConfig::default() });
    let (resting, _) = net.socket_count();
    for port in 1..=1000u16 {
        let listener = net.listen_any(port).await.unwrap();
        drop(listener);
        let again = net.listen_any(port).await.expect("the port of a dropped listener is still held");
        again.close();
        // The next listen removes the dropped one first; the count shows
        // one listener at most.
        assert!(net.socket_count().0 <= resting + 2, "port {port}: {:?} sockets", net.socket_count());
    }
    assert_eq!(settle(&net, resting).await, resting, "a dropped listener keeps its sockets");
    let (_, peak) = net.socket_count();
    assert!(peak <= resting + 2, "the peak is {peak}");
    net.shutdown().await;
}

/// A stream that a listener accepted outlives the listener, also on an
/// address that the listener made the stack own. A connect to the port
/// after the drop is refused.
#[tokio::test]
async fn an_accepted_stream_outlives_its_listener() {
    let mut net = NetStack::loopback();
    let other = at("10.66.0.3".parse().unwrap(), 80);
    let mut listener = net.listen(other).await.unwrap();
    let mut client = net.connect(other).await.unwrap();
    let mut server = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await.expect("no accept").unwrap();
    drop(listener);
    let mut refused = net.connect(other).await.unwrap();
    let err = tokio::time::timeout(Duration::from_secs(3), refused.established())
        .await
        .expect("no answer in 3 s")
        .expect_err("a dropped listener took a connection");
    assert!(matches!(err.kind(), io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionReset), "{err}");
    client.write_all(b"ping").await.unwrap();
    let mut got = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(3), server.read_exact(&mut got)).await.expect("no data").unwrap();
    server.write_all(b"pong").await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), client.read_exact(&mut got)).await.expect("no answer").unwrap();
    assert_eq!(&got, b"pong");
    net.shutdown().await;
}

/// Listeners on 20 addresses that the stack does not own, one after the
/// other: each drop frees its address. The stack owns at most
/// `MAX_ADDRESSES`, four of them its own and the gateways.
#[tokio::test]
async fn dropped_listeners_free_their_addresses() {
    let mut net = NetStack::loopback();
    for last in 10..30u8 {
        let addr = at(IpAddr::V4(Ipv4Addr::new(10, 66, 0, last)), 80);
        let mut listener = net.listen(addr).await.expect("the address of a dropped listener is still owned");
        let mut client = net.connect(addr).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), client.established()).await.expect("no handshake").unwrap();
        let stream = listener.accept().await.unwrap();
        drop(listener);
        drop(stream);
        drop(client);
        assert_eq!(settle(&net, 0).await, 0, "the sockets of {addr} stay");
    }
    net.shutdown().await;
}
