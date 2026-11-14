//! A UDP bind of port 0 (PodUtils/podnet#552): a free port of the dynamic
//! range, with the rule of `connect`.

use super::*;

fn dynamic(port: u16) -> bool {
    (EPHEMERAL_FIRST..=u16::MAX).contains(&port)
}

/// Two binds of port 0 take two free ports of the dynamic range, and
/// `local_addr` gives each one. A datagram to that port reaches the socket.
#[tokio::test]
async fn a_bind_of_port_zero_takes_a_free_port() {
    let mut net = NetStack::loopback();
    let mut first = net.bind_udp(at(LOCAL_IP, 0)).await.expect("a bind of port 0");
    let second = net.bind_udp(at(LOCAL_IP, 0)).await.expect("a second bind of port 0");
    let (one, two) = (first.local_addr(), second.local_addr());
    assert_eq!((one.ip(), two.ip()), (LOCAL_IP, LOCAL_IP));
    assert!(dynamic(one.port()) && dynamic(two.port()), "{one} {two}");
    assert_ne!(one.port(), two.port(), "two binds of port 0 took one port");
    // The port is held: a bind of it is AddrInUse.
    let again = net.bind_udp(one).await.unwrap_err();
    assert_eq!(again.kind(), io::ErrorKind::AddrInUse);
    second.send_to(b"to the port", one).await.unwrap();
    let mut buffer = [0u8; 32];
    let (n, from) = tokio::time::timeout(Duration::from_secs(3), first.recv_from(&mut buffer))
        .await
        .expect("no datagram to the port of the bind")
        .unwrap();
    assert_eq!((&buffer[..n], from), (&b"to the port"[..], two));
    net.shutdown().await;
}

/// `bind_udp_any(0)` takes a free port too, on each address.
#[tokio::test]
async fn a_bind_of_any_address_and_port_zero_takes_a_free_port() {
    let mut net = NetStack::loopback();
    let any = net.bind_udp_any(0).await.expect("a bind of [::]:0");
    let local = any.local_addr();
    assert_eq!(local.ip(), IpAddr::V6(Ipv6Addr::UNSPECIFIED));
    assert!(dynamic(local.port()), "{local}");
    let one = net.bind_udp(at(LOCAL_IP, 0)).await.unwrap();
    assert_ne!(one.local_addr().port(), local.port(), "a bind of port 0 took the port of the socket on [::]");
    net.shutdown().await;
}

/// The port of the rule of `connect` holds a socket: the bind takes the
/// next port.
#[tokio::test]
async fn a_bind_of_port_zero_skips_a_held_port() {
    let seed = 7;
    // The first socket has the id 1; the bind of port 0 has the id 2.
    let held = ephemeral_port(2, seed);
    let mut net = NetStack::loopback_with(StackConfig { random_seed: seed, ..StackConfig::default() });
    let _holder = net.bind_udp(at(LOCAL_IP, held)).await.unwrap();
    let socket = net.bind_udp(at(LOCAL_IP, 0)).await.expect("a bind of port 0");
    let next = if held == u16::MAX { EPHEMERAL_FIRST } else { held + 1 };
    assert_eq!(socket.local_addr().port(), next);
    net.shutdown().await;
}
