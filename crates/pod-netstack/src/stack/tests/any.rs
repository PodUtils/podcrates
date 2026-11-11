//! Any IP: a listener and a UDP socket on any address, for a router that
//! answers for each destination of a program (PodUtils/podnet#454).

use super::*;

/// The address of the program.
const PROGRAM_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 66, 0, 4));
const PROGRAM_V6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0x66, 0, 0, 0, 0, 0, 4);

/// The destinations of the program: a real address outside the subnet of
/// the router, an address of its subnet that it does not own, and a real
/// IPv6 address.
const TARGETS: [&str; 3] = ["1.1.1.1", "10.66.0.77", "2606:4700::1111"];

fn target(ip: &str, port: u16) -> SocketAddr {
    at(ip.parse().unwrap(), port)
}

/// The router at 10.66.0.2, and a program at 10.66.0.4 whose subnet holds
/// each address, so that it dials each destination over the link.
fn router_and_program(any_ip: bool) -> (NetStack, NetStack, Vec<tokio::task::JoinHandle<()>>) {
    let router = StackConfig { any_ip, ..StackConfig::default() };
    let everywhere = Subnet { v4_prefix: 0, v6_prefix: 0, ..Subnet::default() };
    let program = StackConfig {
        subnet: everywhere,
        local_v4: Ipv4Addr::new(10, 66, 0, 4),
        local_v6: PROGRAM_V6,
        ..StackConfig::default()
    };
    pair(router, program, |_| {})
}

async fn stop(mut router: NetStack, mut program: NetStack, pumps: Vec<tokio::task::JoinHandle<()>>) {
    for pump in pumps {
        pump.abort();
    }
    router.shutdown().await;
    program.shutdown().await;
}

/// A connect to an address that the router does not own reaches its
/// listener on any address, and the `local_addr` of the accepted stream is
/// the address that the program dialed.
#[tokio::test]
async fn a_listener_on_any_address_takes_each_destination() {
    let (router, program, pumps) = router_and_program(true);
    let mut listener = router.listen_any(443).await.unwrap();
    assert_eq!(listener.local_addr(), target("::", 443));
    for ip in TARGETS {
        let dialed = target(ip, 443);
        let mut client = program.connect(dialed).await.unwrap();
        client.write_all(b"hello").await.unwrap();
        let mut stream =
            tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.expect("no accept in 5 s").unwrap();
        assert_eq!(stream.local_addr(), dialed);
        assert_eq!(stream.peer_addr(), client.local_addr());
        let mut got = [0u8; 5];
        tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut got)).await.expect("no data").unwrap();
        assert_eq!(&got, b"hello");
        stream.write_all(b"world").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut got)).await.expect("no answer").unwrap();
        assert_eq!(&got, b"world", "the answer from {dialed}");
    }
    assert_eq!(router.stats().accepted, 3);
    stop(router, program, pumps).await;
}

/// With any IP off (the default), a listener on any address takes a
/// connect to an address of the router only. The SYN to 1.1.1.1 goes first
/// over a link that keeps the order: had the router answered it, the
/// program would have its handshake before the answer of the second.
#[tokio::test]
async fn with_any_ip_off_a_listener_on_any_address_takes_only_the_own_addresses() {
    let (router, program, pumps) = router_and_program(false);
    let mut listener = router.listen_any(443).await.unwrap();
    let mut flows = router.bind_udp_any(5353).await.unwrap();
    let mut lost = program.connect(target("1.1.1.1", 443)).await.unwrap();
    let mut client = program.connect(at(LOCAL_IP, 443)).await.unwrap();
    client.write_all(b"ping").await.unwrap();
    let mut stream =
        tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.expect("no accept in 5 s").unwrap();
    assert_eq!(stream.local_addr(), at(LOCAL_IP, 443));
    let mut got = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut got)).await.expect("no data").unwrap();
    stream.write_all(b"pong").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut got)).await.expect("no answer").unwrap();
    assert_eq!(&got, b"pong");
    let handshake = tokio::time::timeout(Duration::ZERO, lost.established()).await;
    assert!(handshake.is_err(), "the router answered a SYN to an address that it does not own");
    assert_eq!(router.stats().accepted, 1);
    // UDP: the first datagram goes to 1.1.1.1 and is dropped.
    let udp = program.bind_udp(at(PROGRAM_IP, 40000)).await.unwrap();
    udp.send_to(b"lost", target("1.1.1.1", 5353)).await.unwrap();
    udp.send_to(b"kept", at(LOCAL_IP, 5353)).await.unwrap();
    let mut buffer = [0u8; 16];
    let (n, from, to) = tokio::time::timeout(Duration::from_secs(5), flows.recv_from_to(&mut buffer))
        .await
        .expect("no datagram")
        .unwrap();
    assert_eq!((&buffer[..n], from, to), (&b"kept"[..], at(PROGRAM_IP, 40000), at(LOCAL_IP, 5353)));
    stop(router, program, pumps).await;
}

/// With any IP, a listener on an address that the stack does not own still
/// owns it: the stack's own connect to it stays in the stack and does not
/// go out on the link (the program drops it there).
#[tokio::test]
async fn with_any_ip_a_listener_on_another_address_stays_in_the_stack() {
    let (router, program, pumps) = router_and_program(true);
    let other = at("10.66.0.3".parse().unwrap(), 80);
    let mut listener = router.listen(other).await.unwrap();
    let mut client = router.connect(other).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), client.established()).await.expect("no handshake in 3 s").unwrap();
    let stream = listener.accept().await.unwrap();
    assert_eq!(stream.local_addr(), other);
    assert_eq!(program.device_stats().rx_packets, 0, "a packet left on the link");
    stop(router, program, pumps).await;
}

/// A datagram to each destination reaches a UDP socket on any address,
/// with its destination; the answer leaves from that destination.
#[tokio::test]
async fn a_udp_socket_on_any_address_answers_from_each_destination() {
    let (router, program, pumps) = router_and_program(true);
    let mut flows = router.bind_udp_any(5353).await.unwrap();
    assert_eq!(flows.local_addr(), target("::", 5353));
    let mut v4 = program.bind_udp(at(PROGRAM_IP, 40000)).await.unwrap();
    let mut v6 = program.bind_udp(at(IpAddr::V6(PROGRAM_V6), 40000)).await.unwrap();
    let mut buffer = [0u8; 16];
    for ip in TARGETS {
        let sent_to = target(ip, 5353);
        let socket = if sent_to.is_ipv4() { &mut v4 } else { &mut v6 };
        socket.send_to(b"ping", sent_to).await.unwrap();
        let (n, from, to) = tokio::time::timeout(Duration::from_secs(5), flows.recv_from_to(&mut buffer))
            .await
            .expect("no datagram")
            .unwrap();
        assert_eq!((&buffer[..n], from, to), (&b"ping"[..], socket.local_addr(), sent_to));
        flows.send_to_from(b"pong", from, to.ip()).await.unwrap();
        let (n, answer) = tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buffer))
            .await
            .expect("no answer")
            .unwrap();
        assert_eq!((&buffer[..n], answer), (&b"pong"[..], sent_to));
    }
    let mixed = flows.send_to_from(b"x", v4.local_addr(), "2606:4700::1111".parse().unwrap()).await;
    assert_eq!(mixed.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    stop(router, program, pumps).await;
}

/// A socket on any address and one on an address do not share a port; the
/// admit sees `[::]:port`. The gateway service keeps its address, and a UDP
/// socket on any address takes the other destinations of its port.
#[tokio::test]
async fn a_socket_on_any_address_shares_no_port_and_the_gateway_service_keeps_its_address() {
    let admit = |target: SocketAddr| !(target.ip().is_unspecified() && target.port() == 7);
    let config = StackConfig {
        any_ip: true,
        admit: Some(Arc::new(admit)),
        gateway_service: Some(Arc::new(super::hooks::Names)),
        ..StackConfig::default()
    };
    let mut net = NetStack::loopback_with(config);
    let in_use = io::ErrorKind::AddrInUse;
    let _any = net.listen_any(80).await.unwrap();
    assert_eq!(net.listen(at(LOCAL_IP, 80)).await.unwrap_err().kind(), in_use);
    let _one = net.listen(at(LOCAL_IP, 81)).await.unwrap();
    assert_eq!(net.listen_any(81).await.unwrap_err().kind(), in_use);
    assert_eq!(net.listen_any(80).await.unwrap_err().kind(), in_use);
    assert_eq!(net.listen_any(7).await.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    let _one = net.bind_udp(at(LOCAL_IP, 81)).await.unwrap();
    assert_eq!(net.bind_udp_any(81).await.unwrap_err().kind(), in_use);
    assert_eq!(net.bind_udp_any(7).await.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    let gateway = at("10.66.0.1".parse().unwrap(), 53);
    assert_eq!(net.bind_udp(gateway).await.unwrap_err().kind(), in_use);
    let mut flows = net.bind_udp_any(53).await.unwrap();
    let mut client = net.bind_udp(at(LOCAL_IP, 40053)).await.unwrap();
    let mut buffer = [0u8; 16];
    client.send_to(b"\x12\x34web", gateway).await.unwrap();
    let (n, from) =
        tokio::time::timeout(Duration::from_secs(2), client.recv_from(&mut buffer)).await.expect("no answer").unwrap();
    assert_eq!((&buffer[..n], from), (&[0x12, 0x34, 10, 66, 0, 2][..], gateway));
    let other = at("10.66.0.77".parse().unwrap(), 53);
    client.send_to(b"query", other).await.unwrap();
    let (n, _, to) =
        tokio::time::timeout(Duration::from_secs(2), flows.recv_from_to(&mut buffer)).await.expect("no query").unwrap();
    assert_eq!((&buffer[..n], to), (&b"query"[..], other));
    net.shutdown().await;
}
