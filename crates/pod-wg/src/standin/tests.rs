use std::time::Duration;

use super::*;
use crate::handshake::initiate;

/// One read of `sock`, at most 5 s.
async fn recv(sock: &tokio::net::UdpSocket) -> Vec<u8> {
    let mut buf = vec![0u8; 2048];
    let n = tokio::time::timeout(Duration::from_secs(5), sock.recv(&mut buf)).await.expect("an answer").unwrap();
    buf.truncate(n);
    buf
}

/// The stand-in on the loopback answers a handshake, sends a packet back,
/// answers a TCP SYN with its SYN-ACK, and drops a message with zero
/// reserved bytes when the path needs them.
#[tokio::test]
async fn the_standin_answers_a_handshake_and_carries_packets() {
    pod_testkit::need_loopback!();
    let (keys, endpoint) = start([7, 8, 9], DataPath::NeedsReserved).await;
    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sock.connect(endpoint).await.unwrap();
    let init = initiate(&keys).unwrap();
    sock.send(&init.message).await.unwrap();
    let response = recv(&sock).await;
    let mut session = init.finish(&keys, &response, Instant::now()).unwrap();

    let mut packet = vec![0x45, 0, 0, 40];
    packet.resize(40, 3);
    sock.send(&session.seal(&[0; 3], &packet)).await.unwrap();
    sock.send(&session.seal(&keys.reserved, &packet)).await.unwrap();
    assert_eq!(session.open(&recv(&sock).await).unwrap(), packet, "only the message with the reserved bytes");

    let syn = tcp_packet(Ipv4Addr::new(172, 16, 0, 2), Ipv4Addr::new(1, 1, 1, 1), 40000, 443, 99, 0, 0x02);
    sock.send(&session.seal(&keys.reserved, &syn)).await.unwrap();
    let back = session.open(&recv(&sock).await).unwrap();
    assert_eq!(
        back,
        tcp_packet(Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(172, 16, 0, 2), 443, 40000, 0x0bad_cafe, 100, 0x12)
    );
    assert_eq!(checksum(0, &back[..20]), 0, "the IPv4 checksum holds");
}

/// An empty datagram does not stop the stand-in: it has no type, and the
/// next handshake gets its response.
#[tokio::test]
async fn an_empty_datagram_does_not_stop_the_standin() {
    pod_testkit::need_loopback!();
    let (keys, endpoint) = start([0; 3], DataPath::All).await;
    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sock.connect(endpoint).await.unwrap();
    sock.send(&[]).await.unwrap();
    let init = initiate(&keys).unwrap();
    sock.send(&init.message).await.unwrap();
    let response = recv(&sock).await;
    assert!(init.finish(&keys, &response, Instant::now()).is_ok());
}

/// The stand-in under load answers an initiation with no mac2 with a
/// cookie reply, and the next initiation with the mac2 of the cookie with
/// its response.
#[tokio::test]
async fn the_standin_under_load_takes_the_mac2_of_its_cookie() {
    pod_testkit::need_loopback!();
    let (keys, endpoint) = start_under_load([0; 3], DataPath::All).await;
    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sock.connect(endpoint).await.unwrap();
    let first = initiate(&keys).unwrap();
    sock.send(&first.message).await.unwrap();
    let reply = recv(&sock).await;
    assert_eq!(reply[0], crate::MSG_COOKIE);
    let cookie = crate::Cookie::open(&keys, &first, &reply, Instant::now()).unwrap();
    let mut next = initiate(&keys).unwrap();
    next.add_mac2(&cookie, Instant::now());
    sock.send(&next.message).await.unwrap();
    let response = recv(&sock).await;
    assert!(next.finish(&keys, &response, Instant::now()).is_ok());
}
