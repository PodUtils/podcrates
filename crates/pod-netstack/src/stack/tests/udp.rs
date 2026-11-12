//! UDP over a link (PodUtils/podnet#464): the bound of a socket whose app
//! reads nothing, the count of each lost datagram, the size of a datagram
//! that the stack can carry, and the drop of a socket.

use super::*;

/// The address of the sender.
const SENDER: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 66, 0, 3));

/// A router at 10.66.0.2 and a sender at 10.66.0.3 over one link.
fn linked(router: StackConfig) -> (NetStack, NetStack, Vec<tokio::task::JoinHandle<()>>) {
    let sender = StackConfig { local_v4: Ipv4Addr::new(10, 66, 0, 3), ..StackConfig::default() };
    pair(router, sender, |_| {})
}

async fn stop(mut router: NetStack, mut sender: NetStack, pumps: Vec<tokio::task::JoinHandle<()>>) {
    for pump in pumps {
        pump.abort();
    }
    router.shutdown().await;
    sender.shutdown().await;
}

/// Wait at most 5 s until `check` holds; its value then.
async fn until(check: impl Fn() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !check() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    check()
}

/// The sender sends `count` datagrams (each holds its number) to a socket of
/// the router whose app reads nothing yet. When the router took each one
/// from its link, the app reads: the numbers that it gets, at most 2 s for
/// all, then 300 ms with none.
async fn burst_with_no_reader(router: &NetStack, sender: &NetStack, count: u32) -> Vec<u32> {
    let mut socket = router.bind_udp(at(LOCAL_IP, 5353)).await.unwrap();
    let from = sender.bind_udp(at(SENDER, 40000)).await.unwrap();
    for number in 0..count {
        from.send_to(&number.to_be_bytes(), at(LOCAL_IP, 5353)).await.unwrap();
    }
    // The reactor shares the thread of the test: with the link empty, it
    // ended the pass that took the last datagram.
    let link = router.link_in();
    let taken = until(|| router.device_stats().rx_packets >= u64::from(count) && link.is_empty()).await;
    assert!(taken, "the router took {} of {count} packets", router.device_stats().rx_packets);
    let mut got = Vec::new();
    let mut buffer = [0u8; 16];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let limit = deadline.saturating_duration_since(tokio::time::Instant::now()).max(Duration::from_millis(300));
        match tokio::time::timeout(limit, socket.recv_from(&mut buffer)).await {
            Ok(Ok((4, _))) => got.push(u32::from_be_bytes(buffer[..4].try_into().unwrap())),
            Ok(other) => panic!("a datagram that the sender did not send: {other:?}"),
            Err(_) => break,
        }
    }
    got
}

/// A burst of 500 datagrams to a socket whose app reads nothing: the stack
/// keeps the first 64 in the channel to the app and the next `udp_packets`
/// in the socket, in order, and the app gets each of them at once when it
/// reads (no wait for the tick of the reactor). Before, the reactor took a
/// datagram from smoltcp with no room in the channel and lost it.
#[tokio::test]
async fn a_burst_with_no_reader_keeps_each_datagram_that_fits() {
    let config = StackConfig { max_poll_interval: Duration::from_secs(10), ..StackConfig::default() };
    let kept = 64 + config.udp_packets as u32;
    let (router, sender, pumps) = linked(config);
    let started = tokio::time::Instant::now();
    let got = burst_with_no_reader(&router, &sender, 500).await;
    assert_eq!(got, (0..kept).collect::<Vec<_>>(), "the app got {} datagrams", got.len());
    assert!(started.elapsed() < Duration::from_secs(5), "the backlog waited for the tick: {:?}", started.elapsed());
    stop(router, sender, pumps).await;
}

/// The largest datagram for each family at MTU 1280 crosses the link; one
/// byte more is `InvalidInput` for the sender, as the stack cannot carry it.
/// Before, the send was `Ok` and the datagram was lost.
#[tokio::test]
async fn a_datagram_over_what_the_stack_carries_is_refused() {
    let v6 = |last| IpAddr::V6(Ipv6Addr::new(0xfd00, 0x66, 0, 0, 0, 0, 0, last));
    let limit_v4 = if cfg!(feature = "ipv4-fragmentation") { 65_507 } else { 1280 - 28 };
    let sender = StackConfig {
        local_v4: Ipv4Addr::new(10, 66, 0, 3),
        local_v6: Ipv6Addr::new(0xfd00, 0x66, 0, 0, 0, 0, 0, 3),
        ..StackConfig::default()
    };
    let (router, sender, pumps) = pair(StackConfig::default(), sender, |_| {});
    for (to, from, limit) in [(LOCAL_IP, SENDER, limit_v4), (v6(2), v6(3), 1280 - 48)] {
        let mut socket = router.bind_udp(at(to, 5353)).await.unwrap();
        let client = sender.bind_udp(at(from, 40000)).await.unwrap();
        let mut buffer = vec![0u8; 70_000];
        client.send_to(&vec![7u8; limit], at(to, 5353)).await.unwrap();
        let (n, _) = tokio::time::timeout(Duration::from_secs(3), socket.recv_from(&mut buffer))
            .await
            .unwrap_or_else(|_| panic!("{limit} bytes to {to} did not cross"))
            .unwrap();
        assert_eq!(n, limit);
        let err = client.send_to(&vec![7u8; limit + 1], at(to, 5353)).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{} bytes to {to}: {err}", limit + 1);
    }
    stop(router, sender, pumps).await;
}

/// A dropped UDP socket sends the datagrams that it queued, then frees its
/// smoltcp socket and its port: a bind of the port works again.
#[tokio::test]
async fn a_dropped_udp_socket_sends_its_queue_and_frees_its_port() {
    let mut net = NetStack::loopback();
    let mut peer = net.bind_udp(at(LOCAL_IP, 5300)).await.unwrap();
    let socket = net.bind_udp(at(LOCAL_IP, 5301)).await.unwrap();
    for number in 0..10u32 {
        socket.send_to(&number.to_be_bytes(), at(LOCAL_IP, 5300)).await.unwrap();
    }
    drop(socket);
    let mut buffer = [0u8; 16];
    for number in 0..10u32 {
        let (n, _) = tokio::time::timeout(Duration::from_secs(3), peer.recv_from(&mut buffer))
            .await
            .expect("a queued datagram was lost")
            .unwrap();
        assert_eq!(&buffer[..n], &number.to_be_bytes());
    }
    assert_eq!(settle(&net, 1).await, 1, "the socket of the dropped UDP socket stays");
    let again = net.bind_udp(at(LOCAL_IP, 5301)).await;
    assert!(again.is_ok(), "the port of the dropped socket is still held: {again:?}");
    net.shutdown().await;
}

/// The same burst: each datagram that the app does not get is counted in
/// `udp_rx_dropped`. Before, the stack counted none.
#[tokio::test]
async fn a_burst_with_no_reader_counts_each_lost_datagram() {
    let (router, sender, pumps) = linked(StackConfig::default());
    let got = burst_with_no_reader(&router, &sender, 500).await;
    let lost = router.stats().udp_rx_dropped;
    assert_eq!(got.len() as u64 + lost, 500, "{} datagrams reached the app, {lost} counted", got.len());
    assert_eq!(router.device_stats().rx_dropped, 0);
    stop(router, sender, pumps).await;
}

/// `UdpDatagrams::try_send` does not wait: a send that finds the queue to
/// the stack full is lost, and counted in `udp_tx_dropped`. The reactor
/// shares the thread of the test, so it takes nothing during the sends.
#[tokio::test]
async fn a_full_queue_to_the_stack_counts_each_lost_datagram() {
    let mut net = NetStack::over_link(StackConfig::default());
    let datagrams = net.bind_udp(at(LOCAL_IP, 40000)).await.unwrap().into_datagrams();
    let to = at("10.66.0.9".parse().unwrap(), 5353);
    for number in 0..200u32 {
        let contents = number.to_be_bytes();
        let transmit = pod_core::Transmit { destination: to, contents: &contents, segment_size: None, src_ip: None };
        pod_core::Datagrams::try_send(&datagrams, &transmit).unwrap();
    }
    let link = net.link_out();
    assert!(until(|| net.stats().datagrams + net.stats().udp_tx_dropped == 200).await, "{:?}", net.stats());
    assert_eq!(net.stats().udp_tx_dropped, 200 - 64, "the queue to the stack holds 64");
    assert!(until(|| link.len() == 64).await, "{} datagrams on the link", link.len());
    net.shutdown().await;
}

/// An IPv4 packet with a UDP datagram with valid sums, or a fragment of it:
/// the bytes of the UDP datagram from `start`, `len` of them (None: to the
/// end).
fn ipv4_udp(from: SocketAddr, to: SocketAddr, payload: &[u8], start: usize, len: Option<usize>) -> Vec<u8> {
    use smoltcp::phy::ChecksumCapabilities;
    use smoltcp::wire::{IpProtocol, Ipv4Packet, Ipv4Repr, UdpPacket, UdpRepr};
    let (IpAddr::V4(src), IpAddr::V4(dst)) = (from.ip(), to.ip()) else {
        panic!("IPv4 only");
    };
    let repr = UdpRepr { src_port: from.port(), dst_port: to.port() };
    let mut udp = vec![0u8; 8 + payload.len()];
    let caps = ChecksumCapabilities::default();
    let (source, destination) = (IpAddress::Ipv4(src), IpAddress::Ipv4(dst));
    let mut packet = UdpPacket::new_unchecked(&mut udp);
    repr.emit(&mut packet, &source, &destination, payload.len(), |b| b.copy_from_slice(payload), &caps);
    let end = len.map_or(udp.len(), |len| (start + len).min(udp.len()));
    let chunk = &udp[start..end];
    let ip = Ipv4Repr {
        src_addr: src,
        dst_addr: dst,
        next_header: IpProtocol::Udp,
        payload_len: chunk.len(),
        hop_limit: 64,
    };
    let mut bytes = vec![0u8; 20 + chunk.len()];
    let mut packet = Ipv4Packet::new_unchecked(&mut bytes);
    ip.emit(&mut packet, &caps);
    packet.set_ident(77);
    packet.set_dont_frag(false);
    packet.set_more_frags(end < udp.len());
    packet.set_frag_offset(start as u16);
    packet.fill_checksum();
    packet.payload_mut().copy_from_slice(chunk);
    bytes
}

/// 100 requests to the gateway service in one pass: its send buffer holds
/// 64 answers, and the stack counts each answer that it lost.
#[tokio::test]
async fn a_burst_to_the_gateway_counts_each_lost_answer() {
    let config = StackConfig { gateway_service: Some(Arc::new(super::hooks::Names)), ..StackConfig::default() };
    let mut net = NetStack::over_link(config);
    let gateway = at("10.66.0.1".parse().unwrap(), 53);
    let link = net.link_in();
    for id in 0..100u16 {
        let request = [&id.to_be_bytes()[..], b"web"].concat();
        link.push(ipv4_udp(at(SENDER, 40000), gateway, &request, 0, None));
    }
    let out = net.link_out();
    let done = until(|| out.len() as u64 + net.stats().udp_tx_dropped == 100).await;
    assert!(done, "{} answers, {:?}", out.len(), net.stats());
    assert_eq!(net.stats().udp_tx_dropped, 100 - 64);
    net.shutdown().await;
}

/// A datagram over what the stack carries is counted in `udp_oversize`;
/// `UdpDatagrams::try_send` refuses it too, also as a segment. The limits
/// of the build.
#[tokio::test]
async fn the_stack_counts_each_datagram_over_its_limit() {
    let config = StackConfig::default();
    let v4 = if crate::IPV4_FRAGMENTATION { 65_507 } else { 1252 };
    assert_eq!(config.max_datagram(LOCAL_IP), v4);
    assert_eq!(config.max_datagram("fd00:66::9".parse().unwrap()), 1232);
    let mut net = NetStack::over_link(config);
    let socket = net.bind_udp(at(LOCAL_IP, 40000)).await.unwrap();
    assert_eq!(socket.max_datagram(LOCAL_IP), v4);
    let to = at("10.66.0.9".parse().unwrap(), 5353);
    let big = vec![1u8; v4 + 1];
    assert_eq!(socket.send_to(&big, to).await.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    let datagrams = socket.into_datagrams();
    assert_eq!(datagrams.max_datagram(LOCAL_IP), v4);
    let transmit = pod_core::Transmit { destination: to, contents: &big, segment_size: None, src_ip: None };
    let err = pod_core::Datagrams::try_send(&datagrams, &transmit).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    let segments = pod_core::Transmit { destination: to, contents: &big, segment_size: Some(v4), src_ip: None };
    pod_core::Datagrams::try_send(&datagrams, &segments).expect("each segment fits");
    assert_eq!(pod_core::Datagrams::may_fragment(&datagrams), crate::IPV4_FRAGMENTATION);
    assert_eq!(net.stats().udp_oversize, 2);
    net.shutdown().await;
}

/// A UDP datagram of 3000 bytes in two IPv4 fragments to a socket of the
/// stack: with the feature `ipv4-fragmentation`, the app gets it; with
/// none, the stack drops each fragment and counts the datagram in
/// `udp_oversize`. A first IPv6 fragment is counted in each build.
#[tokio::test]
async fn an_inbound_datagram_in_fragments() {
    let mut net = NetStack::over_link(StackConfig::default());
    let mut socket = net.bind_udp_any(5353).await.unwrap();
    let payload: Vec<u8> = (0..3000).map(|k| k as u8).collect();
    let to = at(LOCAL_IP, 5353);
    let link = net.link_in();
    link.push(ipv4_udp(at(SENDER, 40000), to, &payload, 0, Some(1008)));
    link.push(ipv4_udp(at(SENDER, 40000), to, &payload, 1008, None));
    let mut buffer = vec![0u8; 4000];
    let got = tokio::time::timeout(Duration::from_millis(500), socket.recv_from(&mut buffer)).await;
    if crate::IPV4_FRAGMENTATION {
        let (n, from) = got.expect("no datagram from the fragments").unwrap();
        assert_eq!((&buffer[..n], from), (&payload[..], at(SENDER, 40000)));
        assert_eq!(net.stats().udp_oversize, 0);
    } else {
        assert!(got.is_err(), "a datagram from the fragments: {got:?}");
        assert_eq!(net.stats().udp_oversize, 1);
    }
    // IPv6: the fixed header, a fragment header (more fragments, offset 0)
    // and the head of a UDP datagram of 3000 bytes to port 5353.
    let mut v6 = vec![0x60, 0, 0, 0, 0, 24, 44, 64];
    v6.extend_from_slice(&"fd00:66::3".parse::<Ipv6Addr>().unwrap().octets());
    v6.extend_from_slice(&"fd00:66::2".parse::<Ipv6Addr>().unwrap().octets());
    v6.extend_from_slice(&[17, 0, 0, 1, 0, 0, 0, 9]);
    v6.extend_from_slice(&[0x9c, 0x40, 0x14, 0xe9, 0x0b, 0xc0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]);
    let before = net.stats().udp_oversize;
    link.push(v6);
    assert!(until(|| net.stats().udp_oversize == before + 1).await, "{:?}", net.stats());
    // The stack runs on.
    link.push(ipv4_udp(at(SENDER, 40000), to, b"after", 0, None));
    let got = tokio::time::timeout(Duration::from_secs(2), socket.recv_from(&mut buffer)).await;
    let (n, _) = got.expect("the stack stopped").unwrap();
    assert_eq!(&buffer[..n], b"after");
    net.shutdown().await;
}

/// The limits for each MTU, with and with no IPv4 fragmentation.
#[test]
fn the_limits_follow_the_mtu_and_the_buffer() {
    let limits = |mtu, payload, fragmentation| {
        let limits = Limits::compute(mtu, payload, fragmentation);
        (limits.v4, limits.v6)
    };
    assert_eq!(limits(1280, 65_536, false), (1252, 1232));
    assert_eq!(limits(1500, 65_536, false), (1472, 1452));
    assert_eq!(limits(1280, 1000, false), (1000, 1000));
    let fragmented = 65_507.min(smoltcp::config::FRAGMENTATION_BUFFER_SIZE - 28);
    assert_eq!(limits(1280, 65_536, true), (fragmented, 1232));
}
