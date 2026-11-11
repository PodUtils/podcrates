//! The checksums of the link: a stack with `Checksum::Tx` takes a packet
//! whose TCP or UDP sum is partial, as the end of a veth pair with its
//! offload on sends it (PodUtils/podnet#454).

use smoltcp::phy::Checksum;

use super::*;

const PROGRAM_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 66, 0, 4));

/// Compute each sum, verify none: podnet's veth wire.
fn tx_only() -> ChecksumCapabilities {
    let mut checksum = ChecksumCapabilities::default();
    checksum.ipv4 = Checksum::Tx;
    checksum.tcp = Checksum::Tx;
    checksum.udp = Checksum::Tx;
    checksum.icmpv4 = Checksum::Tx;
    checksum
}

/// Put the sum of the pseudo-header only into the checksum of each TCP
/// segment and of each UDP datagram that starts with `partial`: what an
/// offload leaves for the hardware to complete.
fn make_partial(packet: &mut [u8]) {
    if packet.first().map(|b| b >> 4) != Some(4) {
        return;
    }
    let header = usize::from(packet[0] & 0x0f) * 4;
    let total = usize::from(u16::from_be_bytes([packet[2], packet[3]])).min(packet.len());
    let (protocol, length) = (packet[9], total.saturating_sub(header));
    let at = match protocol {
        6 => header + 16,
        17 if packet.get(header + 8..).is_some_and(|payload| payload.starts_with(b"partial")) => header + 6,
        _ => return,
    };
    let mut sum: u32 = u32::from(protocol) + length as u32;
    for word in packet[12..20].chunks(2) {
        sum += u32::from(u16::from_be_bytes([word[0], word[1]]));
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    packet[at..at + 2].copy_from_slice(&(sum as u16).to_be_bytes());
}

/// The router at 10.66.0.2 with `checksum`, and a program at 10.66.0.4
/// whose packets get partial sums on their way.
fn router_and_program(checksum: ChecksumCapabilities) -> (NetStack, NetStack, Vec<tokio::task::JoinHandle<()>>) {
    let router = StackConfig { checksum, ..StackConfig::default() };
    let program = StackConfig { local_v4: Ipv4Addr::new(10, 66, 0, 4), ..StackConfig::default() };
    pair(router, program, make_partial)
}

/// A datagram with a partial UDP sum passes with `Checksum::Tx` and is
/// dropped with the default. The program sends it first over a link that
/// keeps the order, then a datagram with a whole sum: the first that the
/// router reads tells which.
#[tokio::test]
async fn a_partial_checksum_passes_with_tx_and_is_dropped_by_default() {
    for (checksum, first) in [(ChecksumCapabilities::default(), &b"whole"[..]), (tx_only(), &b"partial"[..])] {
        let (mut router, mut program, pumps) = router_and_program(checksum);
        let mut server = router.bind_udp(at(LOCAL_IP, 5353)).await.unwrap();
        let client = program.bind_udp(at(PROGRAM_IP, 40000)).await.unwrap();
        client.send_to(b"partial", at(LOCAL_IP, 5353)).await.unwrap();
        client.send_to(b"whole", at(LOCAL_IP, 5353)).await.unwrap();
        let mut buffer = [0u8; 16];
        let (n, _) = tokio::time::timeout(Duration::from_secs(5), server.recv_from(&mut buffer))
            .await
            .expect("no datagram")
            .unwrap();
        assert_eq!(&buffer[..n], first);
        for pump in pumps {
            pump.abort();
        }
        router.shutdown().await;
        program.shutdown().await;
    }
}

/// A TCP exchange whose each segment from the program has a partial sum
/// works with `Checksum::Tx`, and the router's own segments have whole
/// sums (the program verifies them).
#[tokio::test]
async fn a_tcp_exchange_with_partial_checksums_works_with_tx() {
    let (mut router, mut program, pumps) = router_and_program(tx_only());
    let mut listener = router.listen(at(LOCAL_IP, 8080)).await.unwrap();
    let mut client = program.connect(at(LOCAL_IP, 8080)).await.unwrap();
    client.write_all(b"hello").await.unwrap();
    let mut stream =
        tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.expect("no accept in 5 s").unwrap();
    let mut got = [0u8; 5];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut got)).await.expect("no data").unwrap();
    assert_eq!(&got, b"hello");
    stream.write_all(b"world").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut got)).await.expect("no answer").unwrap();
    assert_eq!(&got, b"world");
    for pump in pumps {
        pump.abort();
    }
    router.shutdown().await;
    program.shutdown().await;
}
