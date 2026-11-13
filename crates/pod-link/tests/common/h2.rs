//! The helpers of the tests of the h2 carrier: a carrier of a made-up
//! identity, a listener on the loopback, the dial address of a listener,
//! and a stand-in proxy that tunnels each CONNECT to the listener with no
//! buffer, behind a way out of `pod-proxy`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pod_core::Dial;
use pod_identity::{KeyName, PublicKey};
use pod_link::h2::{H2, H2Listener};
use pod_link::tcp::Keys;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::tcp::{id, ring};

/// A carrier of the identity `n` on ring, over `way`, whose listeners
/// take the keys of `clients`.
pub fn carrier(n: u8, way: Arc<dyn Dial>, clients: &[PublicKey]) -> H2 {
    H2::new(ring(), id(n), way).with_clients(Keys::new(clients.iter().map(|k| KeyName::Key(*k))))
}

/// A listener of `h2` on a free port of the loopback.
pub async fn bound(h2: &H2) -> H2Listener {
    h2.bind("127.0.0.1:0".parse().unwrap()).await.expect("bind on the loopback")
}

/// The dial address of the listener `door` whose key is `key`.
pub fn at(key: &PublicKey, door: &H2Listener) -> String {
    format!("{}@https://{}/link", key.hex(), door.local_addr())
}

/// A stand-in proxy of HTTP: it takes each CONNECT, whatever it names,
/// and carries each byte to `far` as it comes. It counts its tunnels.
pub struct Tunnel {
    pub port: u16,
    pub tunnels: Arc<AtomicUsize>,
}

impl Tunnel {
    pub async fn start(far: std::net::SocketAddr) -> Tunnel {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let tunnels = Arc::new(AtomicUsize::new(0));
        let count = tunnels.clone();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(tunnel(tcp, far));
            }
        });
        Tunnel { port, tunnels }
    }

    /// A way out of `pod-proxy` through this proxy.
    pub fn way(&self) -> Arc<dyn Dial> {
        let proxy = pod_proxy::Proxy::parse(&format!("http://127.0.0.1:{}", self.port)).unwrap();
        let settings = pod_proxy::Settings::new("pod-link", pod_proxy::ProxyChoice::Via(proxy));
        Arc::new(pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new())))
    }
}

async fn tunnel(mut tcp: TcpStream, far: std::net::SocketAddr) {
    // The head of the CONNECT byte by byte, so that nothing of the tunnel
    // is read.
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match tcp.read(&mut byte).await {
            Ok(1) if head.len() < 16 * 1024 => head.push(byte[0]),
            _ => return,
        }
    }
    if !head.starts_with(b"CONNECT ") || tcp.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.is_err() {
        return;
    }
    let Ok(mut to) = TcpStream::connect(far).await else { return };
    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut to).await;
}
