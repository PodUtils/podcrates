//! A mock NTP server of the tests: a UDP socket on the loopback whose clock
//! stands `offset` nanoseconds ahead of the host's. It records the type of
//! each request and the bytes that it took, and it sends down the bytes that
//! a test gives it, cut when the test asks for a cut.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use pod_link::ntp::wire::{self, Timestamp};
use tokio::net::UdpSocket;

/// The mock server and what it saw.
pub struct MockNtp {
    pub addr: SocketAddr,
    /// The nanoseconds that the server's clock stands ahead.
    pub offset: i128,
    /// The type of the extension field of each request, in order.
    pub types: Arc<Mutex<Vec<u16>>>,
    /// The bytes that the requests carried, in order.
    pub up: Arc<Mutex<Vec<u8>>>,
    /// The bytes that the replies carry next.
    pub down: Arc<Mutex<Vec<u8>>>,
    /// The bytes to cut from each reply, after its field says its length.
    pub cut: Arc<AtomicUsize>,
    /// The requests that arrived.
    pub requests: Arc<AtomicUsize>,
}

/// Start the server. The replies carry as many bytes as the request carried
/// up, at most, so each direction moves at the payload of the carrier.
pub async fn mock(offset: i128) -> MockNtp {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("a UDP socket of the loopback");
    let addr = socket.local_addr().expect("the address of the server");
    let types = Arc::new(Mutex::new(Vec::new()));
    let up = Arc::new(Mutex::new(Vec::new()));
    let down = Arc::new(Mutex::new(Vec::new()));
    let cut = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(AtomicUsize::new(0));
    let (seen, taken, given, trimmed, count) = (types.clone(), up.clone(), down.clone(), cut.clone(), requests.clone());
    tokio::spawn(async move {
        let mut buffer = vec![0u8; 4096];
        loop {
            let Ok((bytes, from)) = socket.recv_from(&mut buffer).await else { return };
            let Ok(request) = wire::parse(&buffer[..bytes]) else { continue };
            if request.mode != wire::MODE_CLIENT || request.version != wire::VERSION {
                continue;
            }
            let at = count.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(field) = request.fields.first() {
                seen.lock().unwrap_or_else(|e| e.into_inner()).push(field.kind);
            }
            let payload = request.payload();
            taken.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(&payload);
            let mut value = {
                let mut queue = given.lock().unwrap_or_else(|e| e.into_inner());
                let take = payload.len().min(queue.len());
                queue.drain(..take).collect::<Vec<u8>>()
            };
            let t2 = Timestamp::now().shifted(offset);
            let t3 = Timestamp::now().shifted(offset);
            let kind = 0x3000 + (at as u16 % 0x1000);
            let answered = wire::reply(&request, t2, t3, request.transmit, 2, kind, value.clone());
            value.clear();
            let Ok(mut encoded) = answered.encode() else { continue };
            let trim = trimmed.load(Ordering::SeqCst);
            if trim > 0 {
                encoded.truncate(encoded.len().saturating_sub(trim));
            }
            let _ = socket.send_to(&encoded, from).await;
        }
    });
    MockNtp { addr, offset, types, up, down, cut, requests }
}
