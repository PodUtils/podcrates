//! The NTPv4 carrier (PodUtils/podnet#484): 300 bytes each way over 100
//! exchanges with a new extension field type for each packet and the clock
//! inside 1 ms of the server; a truncated reply is a carrier limit and a
//! smaller payload, and the session stays open; the dial carries a byte
//! stream over a real UDP socket.

#![cfg(feature = "ntp")]

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::ntp::{MockNtp, mock};
use pod_link::ntp::wire::{self, Timestamp};
use pod_link::ntp::{Ntp, Session};
use pod_link::{Carrier, LimitKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::time::timeout;

/// 300 bytes that tell their own place.
fn three_hundred() -> Vec<u8> {
    (0..300u32).map(|at| (at % 251) as u8).collect()
}

/// One exchange of the session against the server, with the bytes that it
/// delivers.
async fn exchange(session: &mut Session, socket: &UdpSocket, up: &[u8]) -> Vec<u8> {
    session.write(up);
    let request = session.request(Timestamp::now()).expect("a request");
    socket.send(&request).await.expect("the request to the server");
    let mut buffer = vec![0u8; 4096];
    let bytes =
        timeout(Duration::from_secs(2), socket.recv(&mut buffer)).await.expect("a reply in time").expect("a reply");
    session.reply(&buffer[..bytes], Timestamp::now()).expect("a reply").expect("a reply to this request");
    let mut out = vec![0u8; 4096];
    let read = session.read(&mut out);
    out.truncate(read);
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn three_hundred_bytes_each_way_over_one_hundred_exchanges() {
    pod_testkit::need_loopback!();
    // The server's clock stands 2 s ahead: a host that far off is the case
    // of NTP-2.
    let offset = 2_000_000_000i128;
    let server: MockNtp = mock(offset).await;
    server.down.lock().unwrap_or_else(|e| e.into_inner()).extend(three_hundred());
    let mut session = Session::new(3, Duration::from_secs(16), Duration::from_millis(1), 0x4242);
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("a UDP socket");
    socket.connect(server.addr).await.expect("a connection to the server");
    let mut down = Vec::new();
    let mut first = None;
    for at in 0..100u32 {
        let up = [at as u8, (at + 1) as u8, (at + 2) as u8];
        session.write(&up);
        let request = session.request(Timestamp::now()).expect("a request");
        if first.is_none() {
            first = Some(wire::parse(&request).expect("a request packet"));
        }
        socket.send(&request).await.expect("the request to the server");
        let mut buffer = vec![0u8; 4096];
        let bytes =
            timeout(Duration::from_secs(2), socket.recv(&mut buffer)).await.expect("a reply in time").expect("a reply");
        let delivered =
            session.reply(&buffer[..bytes], Timestamp::now()).expect("a reply").expect("a reply to this request");
        let mut out = vec![0u8; 4096];
        let read = session.read(&mut out);
        assert_eq!(read, delivered, "the reply gave what it said");
        down.extend_from_slice(&out[..read]);
    }
    assert_eq!(server.up.lock().unwrap_or_else(|e| e.into_inner()).len(), 300, "300 bytes up");
    assert_eq!(down.len(), 300, "300 bytes down");
    assert_eq!(server.requests.load(Ordering::SeqCst), 100, "100 exchanges");
    let types = server.types.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert_eq!(types.len(), 100);
    assert!(types.iter().all(|kind| wire::unassigned(*kind)), "each type is unassigned: {types:?}");
    assert!(types.windows(2).all(|pair| pair[0] != pair[1]), "a constant type is a signature");
    // The clock of the carrier is inside the tolerance of the server.
    assert!(session.discipline().disciplined(), "the discipline holds");
    let error = (session.discipline().offset() - offset).abs();
    assert!(error < 1_000_000, "the offset error is under 1 ms: {error} ns");
    let ahead = session.clock(Timestamp::now()).nanos() - Timestamp::now().shifted(offset).nanos();
    assert!(ahead.abs() < 1_000_000, "the timestamps stand within 1 ms of the server: {ahead} ns");
    // The packet is plausible: version 4, a client, a poll of log2 16.
    let first = first.expect("a first request");
    assert_eq!((first.version, first.mode, first.stratum, first.poll), (4, 3, 0, 4));
    assert_ne!(first.transmit, Timestamp::ZERO, "a transmit timestamp");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_truncated_reply_is_a_limit_and_a_smaller_payload() {
    pod_testkit::need_loopback!();
    let server = mock(0).await;
    server.down.lock().unwrap_or_else(|e| e.into_inner()).extend([1u8, 2, 3]);
    server.cut.store(2, Ordering::SeqCst);
    let mut session = Session::new(8, Duration::from_secs(16), Duration::from_millis(1), 9);
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("a UDP socket");
    socket.connect(server.addr).await.expect("a connection");
    let delivered = exchange(&mut session, &socket, b"abcdef").await;
    assert_eq!(delivered, vec![1], "the bytes that the cut reply carried");
    let limit = session.limit().expect("a reported limit");
    assert_eq!(limit.kind, LimitKind::Truncated, "a cut reply is a carrier limit");
    assert_eq!(limit.bytes, Some(1));
    assert_eq!(session.payload(), 1, "the payload is smaller now");
    // The session stays open: the next exchange goes through.
    server.cut.store(0, Ordering::SeqCst);
    server.down.lock().unwrap_or_else(|e| e.into_inner()).extend([4u8, 5, 6]);
    let delivered = exchange(&mut session, &socket, b"ghijkl").await;
    assert_eq!(delivered, vec![4], "one byte for the smaller payload");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dial_carries_three_hundred_bytes_each_way_over_a_udp_socket() {
    pod_testkit::need_loopback!();
    let server = mock(0).await;
    server.down.lock().unwrap_or_else(|e| e.into_inner()).extend(three_hundred());
    let ntp =
        Ntp::new().with_payload(300).with_poll(Duration::from_secs(16)).with_reply_timeout(Duration::from_secs(2));
    let mut io = ntp.dial(&server.addr.to_string()).await.expect("a dial");
    let data = three_hundred();
    io.write_all(&data).await.expect("the bytes up");
    let mut back = vec![0u8; 300];
    timeout(Duration::from_secs(5), io.read_exact(&mut back))
        .await
        .expect("300 bytes down in time")
        .expect("the bytes down");
    assert_eq!(back, data);
    assert_eq!(server.up.lock().unwrap_or_else(|e| e.into_inner()).as_slice(), data.as_slice());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listener_takes_a_peer_and_the_bytes_up() {
    pod_testkit::need_loopback!();
    let ntp = Ntp::new().with_reply_timeout(Duration::from_secs(2));
    let mut door = ntp.listen("127.0.0.1:0").await.expect("a listener");
    let address = door.address();
    let mut client = ntp.dial(&address).await.expect("a dial");
    client.write_all(b"ping").await.expect("the bytes up");
    let mut far = timeout(Duration::from_secs(5), door.accept()).await.expect("an accept in time").expect("a peer");
    let mut got = [0u8; 4];
    timeout(Duration::from_secs(5), far.read_exact(&mut got)).await.expect("the bytes in time").expect("four bytes");
    assert_eq!(&got, b"ping");
}
