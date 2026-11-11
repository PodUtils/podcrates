use std::net::{IpAddr, SocketAddr};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

mod hooks;
mod link;

/// The address of the default stack.
const LOCAL_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 66, 0, 2));

fn at(ip: IpAddr, port: u16) -> SocketAddr {
    SocketAddr::new(ip, port)
}

#[test]
fn clock_is_monotonic() {
    let clock = Clock::starting_at(100);
    let a = clock.now_ms();
    std::thread::sleep(Duration::from_millis(2));
    let b = clock.now_ms();
    assert!(b > a);
}

/// The request and the response of one TCP exchange in a loopback stack.
/// podnet keeps the same exchange as `loopback_round_trip` of its proof.
#[tokio::test]
async fn async_loopback_round_trip() {
    let mut net = NetStack::loopback();
    let addr = at(LOCAL_IP, 8080);
    let mut listener = net.listen(addr).await.unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let mut buffer = vec![0u8; 1024];
        let n = stream.read(&mut buffer).await.unwrap();
        let body =
            format!("HTTP/1.0 200 OK\r\n\r\nuserspace tcp says: {}", String::from_utf8_lossy(&buffer[..n]).trim());
        stream.write_all(body.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
        n
    });
    let mut client = net.connect(addr).await.unwrap();
    client.write_all(b"GET /hello HTTP/1.0\r\n\r\n").await.unwrap();
    client.flush().await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut response))
        .await
        .expect("no EOF in 5 s")
        .unwrap();
    let request_len = server.await.unwrap();
    net.shutdown().await;
    assert_eq!(request_len, 23);
    let text = String::from_utf8_lossy(&response);
    assert!(text.contains("200 OK"), "{text:?}");
    assert!(text.contains("GET /hello"), "{text:?}");
}

#[tokio::test]
async fn udp_loopback_round_trip() {
    let mut net = NetStack::loopback();
    let server_addr = at(LOCAL_IP, 5353);
    let client_addr = at(LOCAL_IP, 40000);
    let mut server = net.bind_udp(server_addr).await.unwrap();
    let client = net.bind_udp(client_addr).await.unwrap();
    client.send_to(b"ping", server_addr).await.unwrap();
    let mut buffer = [0u8; 64];
    let (n, from) = tokio::time::timeout(Duration::from_secs(2), server.recv_from(&mut buffer))
        .await
        .expect("udp timed out")
        .unwrap();
    assert_eq!(&buffer[..n], b"ping");
    assert_eq!(from, client_addr);
    net.shutdown().await;
}

/// A listener on an address other than the stack's own (a forward to
/// 10.66.0.3) accepts a connection (PodUtils/podnet#41). A connection to a
/// closed port is refused, and `established` says so.
#[tokio::test]
async fn a_listener_on_another_address_accepts() {
    let mut net = NetStack::loopback();
    let other = at("10.66.0.3".parse().unwrap(), 80);
    let mut listener = net.listen(other).await.unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        stream.write_all(b"hi").await.unwrap();
        stream.shutdown();
    });
    let mut client = net.connect(other).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), client.established()).await.expect("no handshake in 3 s").unwrap();
    let mut got = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut got)).await.expect("no EOF in 3 s").unwrap();
    assert_eq!(got, b"hi");
    server.await.unwrap();
    let mut closed = net.connect(at(LOCAL_IP, 81)).await.unwrap();
    let err = tokio::time::timeout(Duration::from_secs(3), closed.established())
        .await
        .expect("no answer in 3 s")
        .unwrap_err();
    assert!(matches!(err.kind(), io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionReset), "{err}");
    net.shutdown().await;
}

#[tokio::test]
async fn outside_subnet_is_refused() {
    let mut net = NetStack::loopback();
    let err = net.connect(at("10.99.0.2".parse().unwrap(), 80)).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AddrNotAvailable);
    net.shutdown().await;
}

#[tokio::test]
async fn tcp_nodelay_is_recorded() {
    let mut net = NetStack::loopback();
    let mut client = net.connect(at(LOCAL_IP, 9)).await.unwrap();
    client.set_nodelay(true);
    assert!(client.nodelay());
    net.shutdown().await;
}

/// A write of the app wakes the reactor; it does not wait for the poll tick.
/// The time limit is below `max_poll_interval`, so a missing wake fails.
#[tokio::test]
async fn application_writes_wake_the_reactor() {
    let mut net = NetStack::loopback();
    let addr = at(LOCAL_IP, 8123);
    let mut listener = net.listen(addr).await.unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let mut buffer = [0u8; 16];
        stream.read(&mut buffer).await.unwrap()
    });
    let mut client = net.connect(addr).await.unwrap();
    client.write_all(b"hello").await.unwrap();
    let read = tokio::time::timeout(Duration::from_millis(60), server).await;
    assert!(read.is_ok(), "the reactor did not serve the write within 60ms");
    assert_eq!(read.unwrap().unwrap(), 5);
    net.shutdown().await;
}

#[tokio::test]
async fn ipv6_loopback_round_trip() {
    let mut net = NetStack::loopback();
    let addr = at("fd00:66::2".parse().unwrap(), 8090);
    let mut listener = net.listen(addr).await.unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let mut buffer = [0u8; 32];
        let n = stream.read(&mut buffer).await.unwrap();
        stream.write_all(b"ok").await.unwrap();
        n
    });
    let mut client = net.connect(addr).await.unwrap();
    assert!(client.local_addr().is_ipv6(), "local is {:?}", client.local_addr());
    client.write_all(b"v6 hello").await.unwrap();
    let mut reply = [0u8; 2];
    tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut reply))
        .await
        .expect("v6 read timed out")
        .unwrap();
    assert_eq!(&reply, b"ok");
    assert_eq!(server.await.unwrap(), 8);
    net.shutdown().await;
}

/// A listen, a connect or a bind that smoltcp refuses (port 0) frees the
/// socket that it added to the set.
#[tokio::test]
async fn a_refused_bind_frees_its_socket() {
    let mut net = NetStack::loopback();
    assert!(net.listen(at(LOCAL_IP, 0)).await.is_err());
    assert!(net.bind_udp(at(LOCAL_IP, 0)).await.is_err());
    assert!(net.connect(at(LOCAL_IP, 0)).await.is_err());
    assert_eq!(net.socket_count().0, 0, "a refused socket stays in the set");
    // A refused listen leaves the address free for a later listen.
    assert!(net.listen(at(LOCAL_IP, 8081)).await.is_ok());
    net.shutdown().await;
}
