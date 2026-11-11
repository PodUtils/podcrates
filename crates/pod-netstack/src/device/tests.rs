use std::sync::Mutex;

use super::*;

#[test]
fn loopback_device_feeds_itself() {
    let mut device = LinkDevice::loopback(1500);
    assert_eq!(device.capabilities().max_transmission_unit, 1500);
    assert_eq!(device.capabilities().medium, Medium::Ip);
    // Send one packet; it comes back on the receive side.
    let token = device.transmit(Instant::ZERO).unwrap();
    token.consume(4, |buf| buf.copy_from_slice(b"ping"));
    let (rx, _tx) = device.receive(Instant::ZERO).expect("loopback frame");
    let mut seen = Vec::new();
    rx.consume(|buf| seen.extend_from_slice(buf));
    assert_eq!(seen, b"ping");
    let stats = device.stats().snapshot();
    assert_eq!(stats.rx_packets, 1);
    assert_eq!(stats.tx_packets, 1);
    assert_eq!(stats.rx_bytes, 4);
    assert_eq!(stats.tx_bytes, 4);
}

#[test]
fn tunnel_device_queues_outbound() {
    let mut device = LinkDevice::tunnel(1280);
    let outbound = device.outbound();
    let token = device.transmit(Instant::ZERO).unwrap();
    token.consume(3, |buf| buf.copy_from_slice(b"tx!"));
    assert_eq!(outbound.pop().unwrap(), b"tx!");
    assert!(device.receive(Instant::ZERO).is_none());
}

#[test]
fn inbound_frames_arrive() {
    let mut device = LinkDevice::tunnel(1280);
    let inbound = device.inbound();
    assert!(inbound.push(b"hello".to_vec()));
    let (rx, _) = device.receive(Instant::ZERO).unwrap();
    let mut seen = Vec::new();
    rx.consume(|buf| seen.extend_from_slice(buf));
    assert_eq!(seen, b"hello");
}

#[test]
fn a_link_device_is_bounded_and_counts_its_inbound_drops() {
    let device = LinkDevice::link(1280);
    let inbound = device.inbound();
    for _ in 0..LINK_INBOUND {
        assert!(inbound.push(vec![0x45]));
    }
    assert!(!inbound.push(vec![0x45]));
    assert_eq!(device.stats().snapshot().rx_dropped, 1);
    assert!(device.outbound().is_empty());
}

#[test]
fn a_full_tunnel_queue_holds_the_stack() {
    let mut device = LinkDevice::tunnel_bounded(1280, 1);
    let token = device.transmit(Instant::ZERO).expect("room");
    token.consume(20, |b| b[0] = 0x45);
    assert!(device.transmit(Instant::ZERO).is_none());
    assert_eq!(device.stats().snapshot().tx_held, 1);
    assert_eq!(device.stats().snapshot().tx_dropped, 0);
    device.outbound().pop().unwrap();
    assert!(device.transmit(Instant::ZERO).is_some());
    // An unbounded tunnel never holds.
    let mut device = LinkDevice::tunnel(1280);
    assert!(device.transmit(Instant::ZERO).is_some());
}

#[test]
fn bounded_queue_drops_and_counts() {
    let queue = PacketQueue::bounded(2);
    assert!(queue.push(b"1".to_vec()));
    assert!(queue.push(b"2".to_vec()));
    assert!(!queue.push(b"3".to_vec()));
    assert_eq!(queue.dropped(), 1);
    assert_eq!(queue.len(), 2);
}

/// PodUtils/podnet#278: the inbound queue of a tunnel has a limit.
#[test]
fn the_inbound_queue_of_a_tunnel_has_a_limit() {
    let device = LinkDevice::tunnel_bounded(1280, 4).with_inbound_limit(2);
    let inbound = device.inbound();
    assert!(inbound.push(b"1".to_vec()));
    assert!(inbound.push(b"2".to_vec()));
    assert!(!inbound.push(b"3".to_vec()), "the third packet is dropped");
    assert_eq!(inbound.dropped(), 1);
}

#[tokio::test]
async fn queue_wakes_a_waiter() {
    let queue = PacketQueue::unbounded();
    let waiter = {
        let queue = queue.clone();
        tokio::spawn(async move { queue.recv().await })
    };
    // Let the waiter park.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    queue.push(b"wake".to_vec());
    let packet =
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter).await.expect("waiter timed out").unwrap();
    assert_eq!(packet, b"wake");
}

/// The tap of a recording device sees each packet of each direction, with
/// its bytes, and the packet still goes through the device.
#[test]
fn a_recording_device_taps_each_packet() {
    let seen: Mutex<Vec<(Direction, Vec<u8>)>> = Mutex::new(Vec::new());
    let tap = |direction: Direction, packet: &[u8]| seen.lock().unwrap().push((direction, packet.to_vec()));
    let mut device = RecordingDevice::new(LinkDevice::loopback(1280), tap);
    let token = device.transmit(Instant::ZERO).unwrap();
    token.consume(3, |buf| buf.copy_from_slice(b"out"));
    let (rx, _tx) = device.receive(Instant::ZERO).expect("the loopback gives the packet back");
    let mut got = Vec::new();
    rx.consume(|buf| got.extend_from_slice(buf));
    assert_eq!(got, b"out");
    assert_eq!(device.capabilities().max_transmission_unit, 1280);
    let inner = device.into_inner();
    assert_eq!(inner.stats().snapshot().tx_packets, 1);
    assert_eq!(
        seen.into_inner().unwrap(),
        vec![(Direction::Outbound, b"out".to_vec()), (Direction::Inbound, b"out".to_vec())]
    );
}

/// `with_checksum` sets the checksums of the capabilities; the default
/// computes and verifies each.
#[test]
fn with_checksum_sets_the_capabilities() {
    let device = LinkDevice::link(1280);
    let checksum = device.capabilities().checksum;
    assert!(checksum.tcp.rx() && checksum.tcp.tx() && checksum.udp.rx() && checksum.ipv4.rx());
    let mut tx_only = ChecksumCapabilities::default();
    tx_only.ipv4 = smoltcp::phy::Checksum::Tx;
    tx_only.tcp = smoltcp::phy::Checksum::Tx;
    let checksum = device.with_checksum(tx_only).capabilities().checksum;
    assert!(!checksum.tcp.rx() && checksum.tcp.tx() && !checksum.ipv4.rx() && checksum.ipv4.tx());
    assert!(checksum.udp.rx(), "a field that the caller did not set keeps its default");
}
