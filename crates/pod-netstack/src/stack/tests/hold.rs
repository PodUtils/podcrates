//! The hold of a full link (PodUtils/podnet#134, PodUtils/podssh#231): while
//! the outbound queue is full, the stack keeps its packets in its sockets;
//! when the queue drains, each packet goes, and none is lost.

use std::sync::atomic::AtomicU64;

use tokio::sync::watch;

use super::*;
use crate::device::LINK_OUTBOUND;

const SIZE: usize = 4 << 20;

fn byte(at: usize) -> u8 {
    (at % 65_536 % 251) as u8
}

/// A server at 10.66.0.2 and a client at 10.66.0.3 over one link. The pump
/// from the server moves a packet only while `open` holds true, and counts
/// each packet that it moves.
fn held_link(
    config: StackConfig,
) -> (NetStack, NetStack, watch::Sender<bool>, Arc<AtomicU64>, Vec<tokio::task::JoinHandle<()>>) {
    let client = StackConfig { local_v4: Ipv4Addr::new(10, 66, 0, 3), ..config.clone() };
    let (server, client) = (NetStack::over_link(config), NetStack::over_link(client));
    let (open, mut gate) = watch::channel(true);
    let moved = Arc::new(AtomicU64::new(0));
    let (from, to) = (client.link_out(), server.link_in());
    let up = tokio::spawn(async move {
        loop {
            to.push(from.recv().await);
        }
    });
    let (from, to, count) = (server.link_out(), client.link_in(), moved.clone());
    let down = tokio::spawn(async move {
        loop {
            if gate.wait_for(|open| *open).await.is_err() {
                return;
            }
            let packet = from.recv().await;
            count.fetch_add(1, Ordering::Relaxed);
            to.push(packet);
        }
    });
    (server, client, open, moved, vec![up, down])
}

/// The server writes 4 MiB while the pump from it stands, with a window
/// larger than its outbound queue: the queue fills and the stack holds.
/// Then the pump moves again: the client reads each byte and the end, the
/// device dropped no packet, and each packet that the stack sent crossed
/// the link.
#[tokio::test]
async fn the_packets_held_at_a_full_link_each_go_when_it_drains() {
    // A window of 2 MiB: more than LINK_OUTBOUND packets in flight.
    let config = StackConfig { tcp_buffer: 2 << 20, ..StackConfig::default() };
    let (mut server_net, mut client_net, open, moved, pumps) = held_link(config);
    let addr = at(LOCAL_IP, 8449);
    let mut listener = server_net.listen(addr).await.unwrap();
    let mut client = client_net.connect(addr).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), client.established()).await.expect("no handshake").unwrap();
    let mut stream = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await.expect("no accept").unwrap();
    open.send_replace(false);
    let writer = tokio::spawn(async move {
        let block: Vec<u8> = (0..64 * 1024).map(byte).collect();
        for _ in 0..SIZE / block.len() {
            stream.write_all(&block).await.unwrap();
        }
        stream.close();
    });
    let out = server_net.link_out();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while out.len() < LINK_OUTBOUND && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(out.len() >= LINK_OUTBOUND, "the queue did not fill: {} packets", out.len());
    // More passes of the reactor while the queue is full.
    tokio::time::sleep(Duration::from_millis(50)).await;
    open.send_replace(true);
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
    let wrote = tokio::time::timeout(Duration::from_secs(5), writer).await;
    // No packet goes after the stop of the reactor: the count is final.
    server_net.shutdown().await;
    let stats = server_net.device_stats();
    assert_eq!(stats.tx_dropped, 0, "{} packets dropped at the full queue, {stats:?}", stats.tx_dropped);
    assert!(stats.tx_held > 0, "the stack never held: {stats:?}");
    assert!(read.is_ok(), "{received} of {SIZE} bytes and no end in 60 s");
    assert!(matches!(wrote, Ok(Ok(()))), "the writer did not end: {wrote:?}");
    assert_eq!(received, SIZE, "the end lost {} bytes", SIZE - received);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while moved.load(Ordering::Relaxed) < stats.tx_packets && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let moved = moved.load(Ordering::Relaxed);
    assert_eq!(moved, stats.tx_packets, "{moved} of {} packets crossed the link", stats.tx_packets);
    for pump in pumps {
        pump.abort();
    }
    client_net.shutdown().await;
}
