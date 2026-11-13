//! The close of a stream (T-231, PodUtils/podnet#457): a dropped or closed
//! stream sends each queued byte, then its FIN, and frees its socket, also
//! after the peer closed first; a dropped stream whose peer makes no
//! progress is freed after the linger.

use super::*;

const SIZE: usize = 16 << 20;

/// A server at 10.66.0.2 and a client at 10.66.0.3 over one link.
fn linked(server: StackConfig) -> (NetStack, NetStack, Vec<tokio::task::JoinHandle<()>>) {
    let client = StackConfig { local_v4: Ipv4Addr::new(10, 66, 0, 3), ..StackConfig::default() };
    pair(server, client, |_| {})
}

fn byte(at: usize) -> u8 {
    (at % 65_536 % 251) as u8
}

/// Read to the end; check each byte. The count of the bytes read.
async fn read_all(client: &mut TcpStream) -> usize {
    let mut received = 0;
    let mut buffer = vec![0u8; 64 * 1024];
    let read = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let n = client.read(&mut buffer).await.unwrap();
            if n == 0 {
                break;
            }
            for (k, got) in buffer[..n].iter().enumerate() {
                assert_eq!(*got, byte(received + k), "byte {} is wrong", received + k);
            }
            received += n;
        }
    })
    .await;
    assert!(read.is_ok(), "{received} of {SIZE} bytes and no end in 60 s");
    received
}

/// The server writes 16 MiB and ends its stream at once with `end`, with
/// bytes in the channel to the reactor: the client reads each byte, then the
/// end, and the socket of the server leaves the set.
async fn download(end: fn(TcpStream)) {
    let (mut server_net, mut client_net, pumps) = linked(StackConfig::default());
    let addr = at(LOCAL_IP, 8443);
    let mut listener = server_net.listen(addr).await.unwrap();
    let (resting, _) = server_net.socket_count();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        let block: Vec<u8> = (0..64 * 1024).map(byte).collect();
        for _ in 0..SIZE / block.len() {
            stream.write_all(&block).await.unwrap();
        }
        end(stream);
        listener
    });
    let mut client = client_net.connect(addr).await.unwrap();
    let received = read_all(&mut client).await;
    assert_eq!(received, SIZE, "the end lost {} bytes", SIZE - received);
    let _listener = server.await.unwrap();
    drop(client);
    assert_eq!(settle(&server_net, resting).await, resting, "the socket of the ended stream stays");
    assert_eq!(settle(&client_net, 0).await, 0, "the socket of the client stays");
    for pump in pumps {
        pump.abort();
    }
    server_net.shutdown().await;
    client_net.shutdown().await;
}

#[tokio::test]
async fn a_dropped_stream_sends_each_queued_byte_then_the_end() {
    download(drop).await;
}

#[tokio::test]
async fn a_closed_stream_sends_each_queued_byte_then_the_end() {
    download(TcpStream::close).await;
}

/// The client closes its write side first, and the server reads that end.
/// Then the server writes 16 MiB and closes at once, with bytes in the
/// channel to the reactor: the client reads each byte, then the end. A close
/// in CLOSE-WAIT that does not wait for these bytes loses them.
#[tokio::test]
async fn a_stream_whose_peer_closed_first_sends_each_queued_byte_then_the_end() {
    let (mut server_net, mut client_net, pumps) = linked(StackConfig::default());
    let addr = at(LOCAL_IP, 8447);
    let mut listener = server_net.listen(addr).await.unwrap();
    let (resting, _) = server_net.socket_count();
    let mut client = client_net.connect(addr).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), client.established()).await.expect("no handshake").unwrap();
    let mut stream = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await.expect("no accept").unwrap();
    client.shutdown();
    let mut nothing = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut nothing))
        .await
        .expect("the server reads no end of the client in 3 s")
        .unwrap();
    assert!(nothing.is_empty(), "{} bytes before the end of the client", nothing.len());
    let server = tokio::spawn(async move {
        let block: Vec<u8> = (0..64 * 1024).map(byte).collect();
        for _ in 0..SIZE / block.len() {
            stream.write_all(&block).await.unwrap();
        }
        stream.close();
    });
    let received = read_all(&mut client).await;
    assert_eq!(received, SIZE, "the end after the end of the peer lost {} bytes", SIZE - received);
    server.await.unwrap();
    drop(client);
    assert_eq!(settle(&server_net, resting).await, resting, "the socket of the ended stream stays");
    for pump in pumps {
        pump.abort();
    }
    server_net.shutdown().await;
    client_net.shutdown().await;
}

/// A dropped stream to a dead peer, with bytes queued: the stack aborts it
/// after the linger, and frees its socket.
#[tokio::test]
async fn a_dropped_stream_to_a_dead_peer_is_freed_after_the_linger() {
    let config = StackConfig { linger: Duration::from_millis(300), ..StackConfig::default() };
    let (mut server_net, mut client_net, pumps) = linked(config);
    let addr = at(LOCAL_IP, 8445);
    let mut listener = server_net.listen(addr).await.unwrap();
    let (resting, _) = server_net.socket_count();
    let mut client = client_net.connect(addr).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), client.established()).await.expect("no handshake").unwrap();
    let mut stream = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await.expect("no accept").unwrap();
    // The link goes: the peer is dead.
    for pump in pumps {
        pump.abort();
    }
    stream.write_all(&vec![1u8; 256 * 1024]).await.unwrap();
    drop(stream);
    assert_eq!(settle(&server_net, resting).await, resting, "a dead peer holds the socket");
    assert_eq!(server_net.stats().closed, 1);
    server_net.shutdown().await;
    client_net.shutdown().await;
}

/// A dropped stream whose peer reads the end but never closes: the stack
/// aborts it after the linger. The peer gets a RST, and its socket goes too.
#[tokio::test]
async fn a_dropped_stream_whose_peer_never_closes_is_freed_after_the_linger() {
    let config = StackConfig { linger: Duration::from_millis(300), ..StackConfig::default() };
    let mut net = NetStack::loopback_with(config);
    let addr = at(LOCAL_IP, 8446);
    let mut listener = net.listen(addr).await.unwrap();
    let (resting, _) = net.socket_count();
    let mut client = net.connect(addr).await.unwrap();
    let mut server = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await.expect("no accept").unwrap();
    server.write_all(b"bye").await.unwrap();
    drop(server);
    let mut got = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut got)).await.expect("no end").unwrap();
    assert_eq!(got, b"bye");
    // The client keeps its stream: the server waits in FIN-WAIT-2.
    assert_eq!(settle(&net, resting).await, resting, "a peer that never closes holds the socket");
    drop(client);
    net.shutdown().await;
}

/// An app that aborts its stream sends a RST: the peer reads a reset.
#[tokio::test]
async fn an_abort_sends_a_reset() {
    let mut net = NetStack::loopback();
    let addr = at(LOCAL_IP, 8444);
    let mut listener = net.listen(addr).await.unwrap();
    let mut client = net.connect(addr).await.unwrap();
    let server = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await.expect("no accept").unwrap();
    server.abort();
    let mut buffer = [0u8; 16];
    let read = tokio::time::timeout(Duration::from_secs(3), client.read(&mut buffer)).await.expect("no RST in 3 s");
    assert_eq!(read.unwrap_err().kind(), io::ErrorKind::ConnectionReset);
    net.shutdown().await;
}

/// A router on any address accepts a connection, and its app aborts the
/// stream at once (podnet's guest wire, PodUtils/podnet#464): a RST crosses
/// the link, the peer reads a reset, and its next write fails.
#[tokio::test]
async fn an_abort_after_the_handshake_sends_a_reset_on_the_link() {
    let resets = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let router = StackConfig { any_ip: true, ..StackConfig::default() };
    let (server, client, gate, pumps) = super::burst::gated(router, super::burst::counting(resets.clone()));
    gate.notify_one();
    let mut listener = server.listen_any(443).await.unwrap();
    let mut stream = client.connect(at("10.66.0.77".parse().unwrap(), 443)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), stream.established()).await.expect("no handshake").unwrap();
    let accepted = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await.expect("no accept").unwrap();
    accepted.abort();
    let mut byte = [0u8; 1];
    let read = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut byte)).await.expect("no RST in 3 s");
    assert_eq!(read.map_err(|err| err.kind()), Err(io::ErrorKind::ConnectionReset));
    assert!(resets.load(Ordering::Relaxed) >= 1, "no RST crossed the link");
    assert!(stream.write_all(b"zz").await.is_err(), "the peer writes to a flow that is gone");
    super::burst::stop(server, client, pumps).await;
}

/// A RST of the peer while the channel to the app is full: the app reads
/// each byte that the stack took, then the reset. With no place for the
/// reset, the app would read a clean end: a cut stream that looks whole.
#[tokio::test]
async fn a_reset_after_a_full_channel_reads_as_a_reset() {
    let (mut server_net, mut client_net, pumps) = linked(StackConfig::default());
    let addr = at(LOCAL_IP, 8448);
    let mut listener = server_net.listen(addr).await.unwrap();
    let mut client = client_net.connect(addr).await.unwrap();
    let mut server = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await.expect("no accept").unwrap();
    // More than the window of the client: its channel to the app (64 chunks
    // of 4 KiB at most) fills, the rest stays in its socket, and the app
    // reads nothing yet.
    server.write_all(&vec![7u8; 512 * 1024]).await.unwrap();
    let channel = 64 * 4096;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    // The payload, with at most 60 bytes of headers in each packet.
    let payload = |stats: crate::DeviceStatsSnapshot| stats.rx_bytes.saturating_sub(60 * stats.rx_packets);
    while payload(client_net.device_stats()) <= channel + 4096 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let packets = client_net.device_stats().rx_packets;
    server.abort();
    // The RST comes in; the reactor takes it in the same pass.
    while client_net.device_stats().rx_packets == packets && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let mut data = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut data)).await.expect("no end");
    assert_eq!(read.map_err(|err| err.kind()), Err(io::ErrorKind::ConnectionReset), "{} bytes", data.len());
    // The bytes that stayed in the socket came before the reset.
    assert!(data.len() as u64 > channel + 4096, "{} bytes", data.len());
    for pump in pumps {
        pump.abort();
    }
    server_net.shutdown().await;
    client_net.shutdown().await;
}
