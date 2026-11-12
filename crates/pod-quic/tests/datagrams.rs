//! QUIC over a [`pod_core::Datagrams`] of the caller, against a stand-in
//! server on the loopback (a socket of the system): a stream of 1 MiB each
//! way and a burst of datagrams, over the system's datagrams, over a queued
//! link, and over a queued link whose send queue holds two datagrams, so
//! that each sender of noq parks on a full queue again and again.

mod common;

use pod_testkit::host::scaled;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use common::{LIMIT, NAME, server, tls};
use pod_core::{BoxFuture, SystemDatagrams};
use pod_quic::noq::{Connection, Endpoint};
use pod_quic::queued::{Link, QueuedDatagrams};
use pod_quic::{Settings, connect};
use sha2::{Digest, Sha256};
use tokio::net::UdpSocket;
use tokio::time::timeout;

/// The bytes of the stream each way.
const STREAM: usize = 1 << 20;
/// The datagrams of the burst.
const BURST: usize = 64;

/// A link over a UDP socket of tokio, as a userspace stack gives one: send
/// and receive, and no poll interface.
struct Udp(UdpSocket);

impl Link for Udp {
    fn send_to(&self, datagram: Vec<u8>, to: SocketAddr) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move { self.0.send_to(&datagram, to).await.map(drop) })
    }

    fn recv_from(&self, max: usize) -> BoxFuture<'_, io::Result<(Vec<u8>, SocketAddr)>> {
        Box::pin(async move {
            let mut buf = vec![0u8; max];
            let (n, from) = self.0.recv_from(&mut buf).await?;
            buf.truncate(n);
            Ok((buf, from))
        })
    }

    fn local_addr(&self) -> SocketAddr {
        self.0.local_addr().expect("a bound socket")
    }
}

async fn queued(queue: usize) -> Endpoint {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let datagrams = QueuedDatagrams::with_queue(Arc::new(Udp(socket)), queue);
    pod_quic::over(Arc::new(datagrams), None).expect("an endpoint over the queued link")
}

/// The stand-in echoes each stream and each datagram.
fn echo(connection: Connection) {
    let streams = connection.clone();
    tokio::spawn(async move {
        while let Ok((mut send, mut recv)) = streams.accept_bi().await {
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut recv, &mut send).await;
                let _ = send.finish();
            });
        }
    });
    tokio::spawn(async move {
        while let Ok(datagram) = connection.read_datagram().await {
            let _ = connection.send_datagram(datagram);
        }
    });
}

/// A connection from `client` to the stand-in: a stream of [`STREAM`] bytes
/// comes back whole, and the datagrams of a burst come back.
async fn exchange(client: Endpoint) {
    let (server, addr, roots) = server();
    let accepted = tokio::spawn(async move {
        let incoming = server.accept().await.expect("a connection");
        let connection = incoming.await.expect("the handshake");
        echo(connection.clone());
        (server, connection)
    });
    let connection = timeout(scaled(LIMIT), connect::connect(&client, addr, NAME, tls(&roots), &Settings::default()))
        .await
        .unwrap()
        .expect("a connection over the datagrams");
    let _server = timeout(scaled(LIMIT), accepted).await.unwrap().unwrap();

    let payload: Vec<u8> = (0..STREAM as u32).map(|i| (i.wrapping_mul(7) % 251) as u8).collect();
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    let sent = payload.clone();
    let writer = tokio::spawn(async move {
        send.write_all(&sent).await.unwrap();
        send.finish().unwrap();
    });
    let back = timeout(scaled(LIMIT), recv.read_to_end(STREAM * 2)).await.unwrap().expect("the echo");
    writer.await.unwrap();
    assert_eq!(back.len(), STREAM);
    assert_eq!(Sha256::digest(&back), Sha256::digest(&payload), "each byte back, in order");

    // Datagrams are not reliable: each one sent again until each came back.
    let mut seen = vec![false; BURST];
    let mut rounds = 0;
    while seen.iter().any(|s| !s) {
        rounds += 1;
        assert!(rounds <= 20, "the datagrams did not come back: {seen:?}");
        for (i, _) in seen.iter().enumerate().filter(|(_, s)| !**s) {
            connection.send_datagram(vec![i as u8; 900].into()).unwrap();
        }
        while let Ok(Ok(datagram)) = timeout(std::time::Duration::from_millis(500), connection.read_datagram()).await {
            assert_eq!(datagram.len(), 900);
            seen[datagram[0] as usize] = true;
        }
    }
    connection.close(0u32.into(), b"done");
    client.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_and_datagrams_cross_quic_over_the_system_datagrams() {
    pod_testkit::need_loopback!();
    let datagrams = SystemDatagrams::bind((Ipv4Addr::LOCALHOST, 0).into()).await.unwrap();
    exchange(pod_quic::over(Arc::new(datagrams), None).unwrap()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_and_datagrams_cross_quic_over_a_queued_link() {
    pod_testkit::need_loopback!();
    exchange(queued(pod_quic::queued::SEND_QUEUE).await).await;
}

/// A send queue of two datagrams is full at each burst of QUIC: each sender
/// parks, and each drain must wake it, or the stream stops.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_full_queue_drains_and_the_stream_completes() {
    pod_testkit::need_loopback!();
    exchange(queued(2).await).await;
}
