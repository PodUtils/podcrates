//! The stand-ins of the tests of the iroh road: a relay server on the
//! loopback, under a name that only the stand-in proxy resolves, so that a
//! connection that does not go through the proxy finds no relay at all. The
//! proxy speaks HTTP CONNECT here, and SOCKS5 in [`socks5`].

// Each test binary uses a part of it.
#![allow(dead_code)]

pub mod socks5;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use iroh::RelayUrl;
use iroh::tls::CaTlsConfig;
use pod_iroh::test_relay;
use pod_iroh::{Options, Udp};
use pod_proxy::{HttpProxy, ProxyChoice};
use pod_tls::Trust;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The relay's name, which no resolver knows: `.test` is reserved.
pub const RELAY_NAME: &str = "relay.podssh.test";
/// The address that a test pins [`RELAY_NAME`] to, for a `socks5://` proxy,
/// which gets an address: not on the loopback, which the dialer sends
/// through no proxy. The stand-in proxy forwards it to the relay.
pub const RELAY_ADDR: &str = "192.0.2.7";
/// The bound that guards a hang of a test: 60 s on a kernel, more under
/// an emulator, which runs QUIC and the crypto slower.
pub fn limit() -> Duration {
    pod_testkit::host::scaled(Duration::from_secs(60))
}

/// A relay server on the loopback, with no QUIC, and its URL under
/// [`RELAY_NAME`]. The relay stops when the server is dropped.
pub async fn relay() -> (RelayUrl, impl Sized) {
    let (_, url, server) = iroh::test_utils::run_relay_server_with(false).await.expect("a relay on the loopback");
    let port = url.port().expect("the relay's port");
    let named: RelayUrl = format!("https://{RELAY_NAME}:{port}").parse().expect("a relay URL");
    (named, server)
}

/// An endpoint's options: the relay, through the proxy, with no UDP; the
/// relay's certificate check skipped, as it is self-signed.
pub fn options(relay: &RelayUrl, proxy: SocketAddr, accepts: bool) -> Options {
    Options {
        relays: vec![relay.clone()],
        tls: test_relay::tls(),
        resolve: test_relay::resolver(),
        proxy: ProxyChoice::Via(HttpProxy::parse(&format!("http://{proxy}")).expect("a proxy URL")),
        trust: Trust::Default,
        udp: Udp::Off,
        secret: None,
        accepts,
        relay_tls: Some(CaTlsConfig::insecure_skip_verify()),
        qad_port: None,
        quic: None,
    }
}

/// A stand-in CONNECT proxy, as a sandbox's: `CONNECT relay.podssh.test:PORT`
/// opens a tunnel to the relay on the loopback; anything else gets 403. The
/// count of tunnels.
pub async fn proxy(relay_port: u16) -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port on the loopback");
    let addr = listener.local_addr().unwrap();
    let tunnels = Arc::new(AtomicUsize::new(0));
    let counted = tunnels.clone();
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let counted = counted.clone();
            tokio::spawn(async move {
                let _ = tunnel(client, relay_port, &counted).await;
            });
        }
    });
    (addr, tunnels)
}

/// A stand-in CONNECT proxy whose client can change its address, as
/// podssh's `fake-proxy.py --move`: after [`Moving::now`], each tunnel that
/// was open goes silent, with no RST, and each new one works. The count of
/// tunnels, and the switch.
pub async fn moving_proxy(relay_port: u16) -> (SocketAddr, Arc<AtomicUsize>, Arc<Moving>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port on the loopback");
    let addr = listener.local_addr().unwrap();
    let tunnels = Arc::new(AtomicUsize::new(0));
    let moving = Arc::new(Moving::default());
    let (counted, switch) = (tunnels.clone(), moving.clone());
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let (counted, switch) = (counted.clone(), switch.clone());
            tokio::spawn(async move {
                let _ = silent_after_move(client, relay_port, &counted, &switch).await;
            });
        }
    });
    (addr, tunnels, moving)
}

/// The switch of [`moving_proxy`]: the moves so far, and until when a new
/// tunnel is refused.
#[derive(Debug, Default)]
pub struct Moving {
    moves: AtomicUsize,
    away_until: std::sync::Mutex<Option<tokio::time::Instant>>,
}

impl Moving {
    /// The client's address changes: each open tunnel goes silent.
    pub fn now(&self) {
        self.moves.fetch_add(1, Ordering::SeqCst);
    }

    /// The client's address changes, and the new one has no network for
    /// `away`: each new tunnel in that time is refused.
    pub fn now_away(&self, away: Duration) {
        *self.away_until.lock().unwrap() = Some(tokio::time::Instant::now() + away);
        self.now();
    }

    fn away(&self) -> bool {
        self.away_until.lock().unwrap().is_some_and(|until| tokio::time::Instant::now() < until)
    }

    fn count(&self) -> usize {
        self.moves.load(Ordering::SeqCst)
    }
}

async fn silent_after_move(
    client: TcpStream,
    relay_port: u16,
    tunnels: &AtomicUsize,
    moving: &Moving,
) -> std::io::Result<()> {
    if moving.away() {
        return Ok(());
    }
    let Some((client, relay)) = open(client, relay_port, tunnels).await? else { return Ok(()) };
    let born = moving.count();
    let (client_r, client_w) = client.into_split();
    let (relay_r, relay_w) = relay.into_split();
    tokio::join!(pipe(client_r, relay_w, moving, born), pipe(relay_r, client_w, moving, born));
    Ok(())
}

/// One way of a tunnel: after a move, its bytes go nowhere, and nothing
/// tells either end, as with an address that is gone.
async fn pipe<R, W>(mut from: R, mut to: W, moving: &Moving, born: usize)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 64 * 1024];
    while let Ok(n) = from.read(&mut buf).await {
        if n == 0 {
            break;
        }
        if moving.count() != born {
            // Held open: the sockets stay, and no byte passes.
            let _hold = (from, to);
            std::future::pending::<()>().await;
            return;
        }
        if to.write_all(&buf[..n]).await.is_err() {
            break;
        }
    }
    let _ = to.shutdown().await;
}

/// The head of a CONNECT read, and the relay dialled: `None` for a request
/// that is refused.
async fn open(
    mut client: TcpStream,
    relay_port: u16,
    tunnels: &AtomicUsize,
) -> std::io::Result<Option<(TcpStream, TcpStream)>> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < 16 * 1024 {
        if client.read(&mut byte).await? == 0 {
            return Ok(None);
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let line = head.lines().next().unwrap_or_default().to_string();
    if line != format!("CONNECT {RELAY_NAME}:{relay_port} HTTP/1.1") {
        client.write_all(b"HTTP/1.1 403 not on the egress allowlist\r\nContent-Length: 0\r\n\r\n").await?;
        return Ok(None);
    }
    let relay = TcpStream::connect(("127.0.0.1", relay_port)).await?;
    tunnels.fetch_add(1, Ordering::SeqCst);
    client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await?;
    Ok(Some((client, relay)))
}

async fn tunnel(mut client: TcpStream, relay_port: u16, tunnels: &AtomicUsize) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < 16 * 1024 {
        if client.read(&mut byte).await? == 0 {
            return Ok(());
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let line = head.lines().next().unwrap_or_default().to_string();
    if line != format!("CONNECT {RELAY_NAME}:{relay_port} HTTP/1.1") {
        client.write_all(b"HTTP/1.1 403 not on the egress allowlist\r\nContent-Length: 0\r\n\r\n").await?;
        return Ok(());
    }
    let mut relay = TcpStream::connect(("127.0.0.1", relay_port)).await?;
    tunnels.fetch_add(1, Ordering::SeqCst);
    client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await?;
    let _ = tokio::io::copy_bidirectional(&mut client, &mut relay).await;
    Ok(())
}
