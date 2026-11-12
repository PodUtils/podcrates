//! The settings of the TCP of a stack that a consumer tunes (T-335, from
//! podnet's WARP stack): the congestion controller, the burst limit of the
//! link and the counters that stacks share.

use smoltcp::phy::Device as _;

use super::*;

/// The TCP window field of an IPv4 packet with a TCP segment, and its SYN
/// flag.
fn window_of(packet: &[u8]) -> Option<(u16, bool)> {
    let header = usize::from(packet.first()? & 0x0f) * 4;
    if packet[0] >> 4 != 4 || *packet.get(9)? != 6 || packet.len() < header + 20 {
        return None;
    }
    let window = u16::from_be_bytes([packet[header + 14], packet[header + 15]]);
    Some((window, packet[header + 13] & 0x02 != 0))
}

/// The window of the SYN that a stack with `config` sends for a connect.
async fn syn_window(config: StackConfig) -> u16 {
    let net = NetStack::over_link(config);
    let out = net.link_out();
    // The stream comes at once; the SYN goes to the link, and no peer answers.
    let _stream = net.connect(at(IpAddr::V4(Ipv4Addr::new(10, 66, 0, 9)), 80)).await.unwrap();
    let wait = async {
        loop {
            if let Some((window, true)) = window_of(&out.recv().await) {
                return window;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait).await.expect("a SYN in 5 s")
}

/// podnet issue 134: each TCP socket takes the controller of the settings;
/// none by default.
#[test]
fn each_tcp_socket_takes_the_congestion_controller_of_the_config() {
    use tcp::CongestionControl;
    assert_eq!(StackConfig::default().congestion_control, CongestionControl::None);
    for cc in [CongestionControl::None, CongestionControl::Reno, CongestionControl::Cubic] {
        let config = StackConfig { congestion_control: cc, ..StackConfig::default() };
        assert_eq!(new_tcp_socket(&config).congestion_control(), cc);
    }
}

/// A download over a bounded link with Reno and with CUBIC: each byte comes,
/// in order, in the time limit.
#[tokio::test]
async fn a_download_with_each_controller_crosses_a_bounded_link() {
    const SIZE: usize = 2 << 20;
    for (port, cc) in [(8501, tcp::CongestionControl::Reno), (8502, tcp::CongestionControl::Cubic)] {
        let config = |last| StackConfig {
            local_v4: Ipv4Addr::new(10, 66, 0, last),
            congestion_control: cc,
            ..StackConfig::default()
        };
        let (mut server_net, mut client_net, pumps) = pair(config(2), config(3), |_| {});
        let addr = at(LOCAL_IP, port);
        let mut listener = server_net.listen(addr).await.unwrap();
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            let data: Vec<u8> = (0..SIZE).map(|i| (i % 249) as u8).collect();
            stream.write_all(&data).await.unwrap();
            stream.shutdown();
            stream
        });
        let mut client = client_net.connect(addr).await.unwrap();
        let mut got = Vec::new();
        let limit = pod_testkit::host::scaled(Duration::from_secs(60));
        tokio::time::timeout(limit, client.read_to_end(&mut got)).await.expect("the end in the limit").unwrap();
        assert_eq!(got.len(), SIZE, "{cc:?}");
        assert!(got.iter().enumerate().all(|(i, b)| *b == (i % 249) as u8), "{cc:?}: the stream has a hole");
        let _server = server.await.unwrap();
        for pump in pumps {
            pump.abort();
        }
        server_net.shutdown().await;
        client_net.shutdown().await;
    }
}

/// podnet issue 100: a burst limit of 2 segments clamps the window that the
/// stack advertises to 2 segments; with no limit, the window of the SYN is
/// the buffer (at most 65535 in a SYN).
#[tokio::test]
async fn the_burst_limit_clamps_the_window_that_the_stack_advertises() {
    let segment = 1280 - 20 - 20;
    let clamped = syn_window(StackConfig { max_burst: Some(2), ..StackConfig::default() }).await;
    assert!(usize::from(clamped) <= 2 * segment, "a window of {clamped} with 2 segments");
    let open = syn_window(StackConfig::default()).await;
    assert_eq!(open, u16::MAX, "with no limit");
    assert_eq!(
        link_device(&StackConfig { max_burst: Some(3), ..StackConfig::default() }, false).capabilities().max_burst_size,
        Some(3)
    );
}

/// Two stacks with the counters of one link: each counts into them, and
/// `device_stats` of each reads them.
#[tokio::test]
async fn stacks_with_shared_counters_add_to_one_count() {
    let shared = Arc::new(DeviceStats::default());
    let config = |last| StackConfig {
        local_v4: Ipv4Addr::new(10, 66, 0, last),
        link_stats: Some(shared.clone()),
        ..StackConfig::default()
    };
    let (mut a, mut b, pumps) = pair(config(2), config(3), |_| {});
    let addr = at(LOCAL_IP, 8503);
    let mut listener = a.listen(addr).await.unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let mut ping = [0u8; 4];
        stream.read_exact(&mut ping).await.unwrap();
        stream.write_all(b"pong").await.unwrap();
        stream
    });
    let mut client = b.connect(addr).await.unwrap();
    client.write_all(b"ping").await.unwrap();
    let mut pong = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut pong)).await.expect("a pong").unwrap();
    let _server = server.await.unwrap();
    let counted = shared.snapshot();
    // The SYN, the SYN-ACK, the ACK, the ping, the pong: from both stacks.
    assert!(counted.tx_packets >= 5 && counted.rx_packets >= 5, "{counted:?}");
    assert!(a.device_stats().tx_packets >= counted.tx_packets, "the stack reads the shared counters");
    assert!(b.device_stats().rx_packets >= counted.rx_packets, "the stack reads the shared counters");
    for pump in pumps {
        pump.abort();
    }
    a.shutdown().await;
    b.shutdown().await;
}
