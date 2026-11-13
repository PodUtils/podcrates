//! The helpers of the tests of the accept loop: made-up identities, the
//! TLS of each side with the pin of each key, a loop on the loopback, a
//! client that dials from an address of its choice, upgrades and sends its
//! first frame, and the wait for a cut.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer::{self, Keys};
use pod_ws::RelaySession;
use pod_ws::serve::{Accepted, Serve};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// The bound of each wait of a test for an event.
pub const LIMIT: Duration = Duration::from_secs(20);

/// A deadline that no test reaches.
pub const LONG: Duration = Duration::from_secs(300);

/// The ALPN of a WebSocket over TLS.
const HTTP1: &[u8] = b"http/1.1";

/// A made-up identity: the seed is `n` in each byte.
pub fn id(n: u8) -> Arc<Identity> {
    Arc::new(Identity::from_seed(&[n; 32]))
}

pub fn ring() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The TLS of a server of the identity 1 that takes the clients of
/// `clients`.
pub fn server_tls(clients: &[PublicKey]) -> Arc<rustls::ServerConfig> {
    let keys = Keys::new(clients.iter().map(|k| KeyName::Key(*k)));
    peer::server_config(ring(), id(1), keys, &[HTTP1]).unwrap()
}

/// The TLS of a client of the identity `n` that takes the server of the
/// identity 1.
pub fn client_tls(n: u8) -> Arc<rustls::ClientConfig> {
    peer::client_config(ring(), id(n), KeyName::Key(id(1).public()), &[HTTP1]).unwrap()
}

/// Deadlines that no test reaches.
pub fn long() -> Serve {
    let mut cfg = Serve::default();
    cfg.deadlines.tls = LONG;
    cfg.deadlines.head = LONG;
    cfg.deadlines.first = LONG;
    cfg.deadlines.ready = LONG;
    cfg
}

/// A loop on a free port of the loopback that takes the client of the
/// identity 2: its address, the sessions it accepts, and its task.
pub async fn start(cfg: Serve) -> (SocketAddr, mpsc::UnboundedReceiver<Accepted>, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let on = move |accepted| {
            let _ = tx.send(accepted);
        };
        pod_ws::serve::serve(vec![listener], server_tls(&[id(2).public()]), cfg, on).await.unwrap();
    });
    (local, rx, task)
}

/// A TCP connection to `to` from the address `from`, or from the system's
/// choice; none when the host cannot bind `from`.
pub async fn tcp_from(from: Option<IpAddr>, to: SocketAddr) -> Option<TcpStream> {
    let socket = TcpSocket::new_v4().unwrap();
    if let Some(ip) = from {
        socket.bind(SocketAddr::new(ip, 0)).ok()?;
    }
    Some(socket.connect(to).await.unwrap())
}

pub type ClientTls = tokio_rustls::client::TlsStream<TcpStream>;

/// The TLS of the client `n` over `tcp`.
pub async fn tls_over(tcp: TcpStream, n: u8) -> std::io::Result<ClientTls> {
    let name = ServerName::IpAddress(tcp.peer_addr()?.ip().into());
    tokio_rustls::TlsConnector::from(client_tls(n)).connect(name, tcp).await
}

/// The upgrade of a client over `tls`: the bytes after the 101.
pub async fn upgrade(tls: &mut ClientTls) -> Vec<u8> {
    let key = pod_ws::handshake::generate_key().unwrap();
    let url = pod_ws::url::WsUrl::parse("wss://loop.test/serve").unwrap();
    tls.write_all(&pod_ws::url::request(&url, &key, "pod-ws/test").unwrap()).await.unwrap();
    pod_ws::handshake::read_response(tls, &key).await.unwrap()
}

/// A client of the identity 2 from `from` that upgrades and sends its
/// first frame: its session.
pub async fn session_from(from: Option<IpAddr>, to: SocketAddr) -> RelaySession<ClientTls> {
    let tcp = tcp_from(from, to).await.expect("a bind of the client");
    let mut tls = tls_over(tcp, 2).await.unwrap();
    let pending = upgrade(&mut tls).await;
    let session = RelaySession::new(tls, pending, None, LIMIT);
    session.send_binary(b"hello").await.unwrap();
    session
}

/// Whether the server ended `stream` within [`LIMIT`]: a read gives its
/// end, or an error.
pub async fn cut<S: tokio::io::AsyncRead + Unpin>(stream: &mut S) -> bool {
    let mut byte = [0u8; 64];
    let read = async {
        loop {
            match stream.read(&mut byte).await {
                Ok(0) | Err(_) => return,
                // A refusal or an alert before the end.
                Ok(_) => continue,
            }
        }
    };
    tokio::time::timeout(LIMIT, read).await.is_ok()
}

/// The next session that the loop gave, within [`LIMIT`].
pub async fn next(rx: &mut mpsc::UnboundedReceiver<Accepted>) -> Accepted {
    tokio::time::timeout(LIMIT, rx.recv()).await.expect("a session in time").expect("the loop runs")
}
