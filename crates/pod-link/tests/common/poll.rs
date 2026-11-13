//! The helpers of the tests of the poll carriers: a carrier of a made-up
//! identity in a form, a listener on the loopback, the dial address of a
//! listener, a way out that relays each connection through rules that cut
//! it (a silence, a lost answer, a cut answer), and a raw request of a pinned
//! client.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Stage, Target};
use pod_identity::{Identity, KeyName, PublicKey};
use pod_link::poll::{Form, Poll, PollListener};
use pod_link::tcp::Keys;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::net::TcpStream;

use super::tcp::{id, ring};

/// A carrier of the identity `n` on ring in `form`, over `way`, whose
/// listeners take the keys of `clients`.
pub fn carrier(n: u8, way: Arc<dyn Dial>, clients: &[PublicKey], form: Form) -> Poll {
    Poll::new(ring(), id(n), way).with_form(form).with_clients(Keys::new(clients.iter().map(|k| KeyName::Key(*k))))
}

/// A listener on a free port of the loopback.
pub async fn bound(poll: &Poll) -> PollListener {
    poll.bind("127.0.0.1:0".parse().unwrap()).await.expect("bind on the loopback")
}

/// The dial address of the listener `door` whose key is `key`.
pub fn at(key: &PublicKey, door: &PollListener) -> String {
    format!("{}@https://{}/link", key.hex(), door.local_addr())
}

/// The bytes of one way of a connection after which a rule acts: past the
/// TLS handshake, within a large body.
const PAST: usize = 16 << 10;

/// A way out to the loopback that relays each connection through its
/// rules, and counts its dials.
#[derive(Default)]
pub struct Relay {
    /// Cut a connection with no byte for this time.
    pub idle: Option<Duration>,
    pub dials: AtomicUsize,
    pub faults: Arc<Faults>,
}

/// The faults of a relay, each one time, and the count of its cuts.
#[derive(Default)]
pub struct Faults {
    /// On a connection whose dialer sent more than 16 KiB, drop the next
    /// answer and close.
    pub lose_up: AtomicBool,
    /// On a connection whose listener sent more than 16 KiB, cut it there.
    pub cut_down: AtomicBool,
    pub cuts: AtomicUsize,
    /// Fail each dial and cut each connection.
    pub dead: AtomicBool,
    killed: tokio::sync::Notify,
}

impl Faults {
    /// Cut each connection and fail each dial from now on.
    pub fn kill(&self) {
        self.dead.store(true, Ordering::SeqCst);
        self.killed.notify_waiters();
    }
}

impl Relay {
    /// A relay that loses one answer of a POST and cuts one answer down.
    pub fn lossy() -> Relay {
        let faults = Faults { lose_up: AtomicBool::new(true), cut_down: AtomicBool::new(true), ..Faults::default() };
        Relay { faults: Arc::new(faults), ..Relay::default() }
    }

    /// A relay that cuts each connection silent for `idle`.
    pub fn cutting(idle: Duration) -> Relay {
        Relay { idle: Some(idle), ..Relay::default() }
    }
}

impl Dial for Relay {
    fn label(&self) -> String {
        "test-relay".into()
    }

    fn dial<'a>(&'a self, target: &'a Target, _: tokio::time::Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            self.dials.fetch_add(1, Ordering::SeqCst);
            if self.faults.dead.load(Ordering::SeqCst) {
                return Err(Failure::new(Stage::Connect, Class::Unavailable, "the test relay is dead"));
            }
            let Target::Addr(addr) = target else {
                return Err(Failure::new(Stage::Dns, Class::NotFound, "the test resolves no name"));
            };
            let tcp = TcpStream::connect(addr)
                .await
                .map_err(|e| Failure::new(Stage::Connect, Class::of_io(e.kind()), e.to_string()))?;
            let (near, far) = tokio::io::duplex(64 << 10);
            let idle = self.idle.unwrap_or(Duration::from_secs(86400));
            tokio::spawn(relay(far, tcp, idle, self.faults.clone()));
            Ok(Connected::new(Box::new(near)))
        })
    }
}

async fn relay(mut near: DuplexStream, mut tcp: TcpStream, idle: Duration, faults: Arc<Faults>) {
    let (mut up, mut down, mut lose_next) = (0usize, 0usize, false);
    let (mut a, mut b) = (vec![0u8; 16 << 10], vec![0u8; 16 << 10]);
    loop {
        let killed = faults.killed.notified();
        tokio::pin!(killed);
        killed.as_mut().enable();
        if faults.dead.load(Ordering::SeqCst) {
            return;
        }
        tokio::select! {
            () = &mut killed => return,
            r = near.read(&mut a) => {
                let n = r.unwrap_or(0);
                if n == 0 || tcp.write_all(&a[..n]).await.is_err() {
                    return;
                }
                up += n;
                if up > PAST && !lose_next {
                    lose_next = faults.lose_up.swap(false, Ordering::SeqCst);
                }
            }
            r = tcp.read(&mut b) => {
                let n = r.unwrap_or(0);
                if n == 0 {
                    return;
                }
                if lose_next {
                    faults.cuts.fetch_add(1, Ordering::SeqCst);
                    return;
                }
                if down + n > PAST && faults.cut_down.swap(false, Ordering::SeqCst) {
                    let _ = near.write_all(&b[..PAST.saturating_sub(down)]).await;
                    faults.cuts.fetch_add(1, Ordering::SeqCst);
                    return;
                }
                down += n;
                if near.write_all(&b[..n]).await.is_err() {
                    return;
                }
            }
            () = tokio::time::sleep(idle) => {
                faults.cuts.fetch_add(1, Ordering::SeqCst);
                return;
            }
        }
    }
}

/// A raw request of HTTP/1.1 of the client `client` to the listener at
/// `local` of the key `server`: the status of its answer.
pub async fn raw(server: &PublicKey, client: Arc<Identity>, local: std::net::SocketAddr, request: &str) -> u16 {
    let config = pod_tls::peer::client_config(ring(), client, KeyName::Key(*server), &[b"http/1.1"]).unwrap();
    let tcp = TcpStream::connect(local).await.unwrap();
    let name = rustls::pki_types::ServerName::IpAddress(local.ip().into());
    let mut tls = tokio_rustls::TlsConnector::from(config).connect(name, tcp).await.unwrap();
    tls.write_all(request.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8];
    while !head.ends_with(b"\r\n\r\n") {
        tls.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }
    let line = String::from_utf8_lossy(&head).into_owned();
    line.split(' ').nth(1).unwrap().parse().unwrap()
}

/// The key of the identity `n`.
pub fn key(n: u8) -> PublicKey {
    id(n).public()
}

/// A request of a lane with an empty body.
pub fn post(path: &str) -> String {
    format!("POST {path} HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n")
}

pub fn identity(n: u8) -> Arc<Identity> {
    id(n)
}
