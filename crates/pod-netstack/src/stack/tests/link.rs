//! The behaviour of smoltcp 0.14 that the consumers rely on: the close of a
//! socket after its queues are empty, `TimeWait` read as closed, and a large
//! transfer over two bounded links at full speed with buffers of 64 KiB.

use super::*;

/// Two stacks over one link: a pump moves each packet that one sends into
/// the inbound queue of the other.
fn linked(last_a: u8, last_b: u8) -> (NetStack, NetStack, Vec<tokio::task::JoinHandle<()>>) {
    let config = |last| StackConfig { local_v4: Ipv4Addr::new(10, 66, 0, last), ..StackConfig::default() };
    let (a, b) = (NetStack::over_link(config(last_a)), NetStack::over_link(config(last_b)));
    let pump = |from: PacketQueue, to: PacketQueue| {
        tokio::spawn(async move {
            loop {
                let packet = from.recv().await;
                to.push(packet);
            }
        })
    };
    let pumps = vec![pump(a.link_out(), b.link_in()), pump(b.link_out(), a.link_in())];
    (a, b, pumps)
}

/// Each closed connection leaves the socket set. The server drops its
/// handle while its reply is still queued: its close waits for the empty
/// queue, and its socket ends in `TimeWait`, which counts as closed. The
/// client closes after the reply, and its socket ends in `Closed`.
#[tokio::test]
async fn closed_connections_leave_no_socket() {
    let mut net = NetStack::loopback();
    let addr = at(LOCAL_IP, 7000);
    let mut listener = net.listen(addr).await.unwrap();
    let (resting, _) = net.socket_count();
    let server = tokio::spawn(async move {
        for _ in 0..20 {
            let mut stream = listener.accept().await.unwrap();
            let mut request = [0u8; 4];
            stream.read_exact(&mut request).await.unwrap();
            // A reply larger than one segment, then a drop at once: the
            // close waits for the send queue.
            stream.write_all(&[7u8; 8000]).await.unwrap();
        }
    });
    for _ in 0..20 {
        let mut client = net.connect(addr).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        let mut reply = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut reply))
            .await
            .expect("no EOF in 5 s")
            .unwrap();
        assert_eq!(reply.len(), 8000, "the reply of a dropped handle is cut");
    }
    server.await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while net.socket_count().0 != resting && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (live, peak) = net.socket_count();
    assert_eq!(live, resting, "the closed sockets stay in the set (peak {peak})");
    assert_eq!(net.stats().closed, 40);
    net.shutdown().await;
}

/// A download of 16 MiB between two stacks over bounded links (the link
/// queues hold 512 and 4096 packets) completes with the default buffers of
/// 64 KiB. A stack that loses the data of a full link, or that stalls on
/// it, does not finish in the time limit.
#[tokio::test]
async fn a_large_download_crosses_a_bounded_link() {
    const SIZE: usize = 16 << 20;
    let (mut server_net, mut client_net, pumps) = linked(2, 3);
    let addr = at(LOCAL_IP, 8443);
    let mut listener = server_net.listen(addr).await.unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let block: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
        let mut sent = 0;
        while sent < SIZE {
            stream.write_all(&block).await.unwrap();
            sent += block.len();
        }
        stream.shutdown();
        stream
    });
    let mut client = client_net.connect(addr).await.unwrap();
    let start = std::time::Instant::now();
    let mut received = 0usize;
    let mut buffer = vec![0u8; 64 * 1024];
    let read = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let n = client.read(&mut buffer).await.unwrap();
            if n == 0 {
                break;
            }
            for (k, byte) in buffer[..n].iter().enumerate() {
                assert_eq!(*byte, ((received + k) % (64 * 1024) % 251) as u8, "byte {} is wrong", received + k);
            }
            received += n;
        }
    })
    .await;
    assert!(read.is_ok(), "{received} of {SIZE} bytes in 60 s");
    assert_eq!(received, SIZE);
    let elapsed = start.elapsed();
    let _server = server.await.unwrap();
    // A floor far below the rate of a debug build on one core, so that only
    // a stall (a retransmit timeout for each window) fails it.
    let rate = SIZE as f64 / elapsed.as_secs_f64();
    assert!(rate > 1_000_000.0, "{rate:.0} B/s over {elapsed:?}");
    let stats = client_net.device_stats();
    assert!(stats.rx_packets > (SIZE / 1280) as u64, "{stats:?}");
    for pump in pumps {
        pump.abort();
    }
    server_net.shutdown().await;
    client_net.shutdown().await;
}

/// A reader that waits before it reads gets each byte and the end. The
/// stack takes no bytes from a socket while the channel to the app is full:
/// before, the bytes of a receive that found the channel full were lost,
/// and the stream had a hole.
#[tokio::test]
async fn a_slow_reader_gets_each_byte_and_the_end() {
    const SIZE: usize = 1 << 20;
    let mut net = NetStack::loopback();
    let addr = at(LOCAL_IP, 8444);
    let mut listener = net.listen(addr).await.unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let data: Vec<u8> = (0..SIZE).map(|i| (i % 253) as u8).collect();
        stream.write_all(&data).await.unwrap();
        stream.shutdown();
        stream
    });
    let mut client = net.connect(addr).await.unwrap();
    // The channel to the app (64 chunks) and the receive buffer fill.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut got = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), client.read_to_end(&mut got)).await.expect("no EOF in 10 s").unwrap();
    assert_eq!(got.len(), SIZE, "the stream has a hole");
    assert!(got.iter().enumerate().all(|(i, b)| *b == (i % 253) as u8), "the stream has a hole");
    let _server = server.await.unwrap();
    net.shutdown().await;
}
