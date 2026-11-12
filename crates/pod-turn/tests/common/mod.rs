// The mock TURN server of the tests, over UDP and over TLS on TCP, on the
// loopback; a UDP peer that echoes; and the settings of a client for it.
#![allow(dead_code)]

pub mod mock;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mock::{Control, Knobs, Mock, Relayed, Session, Shared};
use pod_turn::{Credentials, Settings, SystemUdp};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::mpsc;

// The certificates of the tests of pod-tls, minted in pure Rust.
pub mod cert {
    include!("../../../pod-tls/tests/common/cert_for_test.rs");
}

/// The realm and the users of the mock: made-up values of the tests.
pub const REALM: &str = "mock.test";
pub const USER: &str = "turn-user";
pub const PASSWORD: &str = "turn-test-password";
pub const OTHER_USER: &str = "turn-other";
pub const OTHER_PASSWORD: &str = "turn-other-password";
/// The name of the mock's certificate.
pub const NAME: &str = "turn.test";
/// The bound of each wait of a test.
pub const LIMIT: Duration = Duration::from_secs(10);

pub fn knobs(sha256: bool) -> Knobs {
    Knobs {
        realm: REALM.into(),
        users: vec![(USER.into(), PASSWORD.into()), (OTHER_USER.into(), OTHER_PASSWORD.into())],
        sha256,
        max_lifetime: 3600,
    }
}

/// A running mock: its address and what a test reads and changes.
pub struct Running {
    pub addr: SocketAddr,
    pub control: Shared,
    /// The roots that trust the mock's certificate (TLS only).
    pub roots: Option<rustls::RootCertStore>,
}

/// The mock over UDP on 127.0.0.1.
pub async fn udp_mock(knobs: Knobs) -> Running {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let addr = socket.local_addr().unwrap();
    let control: Shared = Arc::new(Mutex::new(Control::default()));
    let (relayed, mut from_relays) = mpsc::unbounded_channel::<Relayed>();
    let mock = Mock { knobs, control: control.clone(), relayed };
    tokio::spawn(async move {
        let mut sessions = std::collections::HashMap::<SocketAddr, Session>::new();
        let mut buf = vec![0u8; 65_536];
        loop {
            tokio::select! {
                got = socket.recv_from(&mut buf) => {
                    let Ok((n, from)) = got else { return };
                    let session = sessions.entry(from).or_insert_with(|| Session::new(from));
                    let bytes = buf[..n].to_vec();
                    if bytes.first().is_some_and(|b| b >> 6 == 1) && bytes.len() >= 4 {
                        let len = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
                        let number = u16::from_be_bytes([bytes[0], bytes[1]]);
                        if let Some(data) = bytes.get(4..4 + len) {
                            mock.channel_data(session, number, data).await;
                        }
                        continue;
                    }
                    for answer in mock.handle(session, &bytes).await {
                        let _ = socket.send_to(&answer, from).await;
                    }
                }
                Some((client, peer, data)) = from_relays.recv() => {
                    let Some(session) = sessions.get(&client) else { continue };
                    if let Some(out) = mock.to_client(session, peer, &data, false) {
                        let _ = socket.send_to(&out, client).await;
                    }
                }
            }
        }
    });
    Running { addr, control, roots: None }
}

/// The mock over TLS on TCP on 127.0.0.1: one session for each connection.
pub async fn tls_mock(knobs: Knobs) -> Running {
    let (acceptor, roots) = cert::server_for(NAME).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let control: Shared = Arc::new(Mutex::new(Control::default()));
    let shared = control.clone();
    tokio::spawn(async move {
        while let Ok((tcp, from)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let (relayed, from_relays) = mpsc::unbounded_channel::<Relayed>();
            let mock = Mock { knobs: knobs.clone(), control: shared.clone(), relayed };
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else { return };
                serve_stream(mock, tls, from, from_relays).await;
            });
        }
    });
    Running { addr, control, roots: Some(roots) }
}

/// One connection of the TLS mock: each message cut by its own length,
/// ChannelData with its padding.
async fn serve_stream<S>(mock: Mock, mut io: S, from: SocketAddr, mut from_relays: mpsc::UnboundedReceiver<Relayed>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut session = Session::new(from);
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 16 * 1024];
    loop {
        while buf.len() >= 4 {
            let len = usize::from(u16::from_be_bytes([buf[2], buf[3]]));
            let (stun, whole) = match buf[0] >> 6 {
                0 => (true, 20 + len),
                1 => (false, 4 + len + (4 - len % 4) % 4),
                _ => return,
            };
            if buf.len() < whole {
                break;
            }
            let message: Vec<u8> = buf.drain(..whole).collect();
            if stun {
                for answer in mock.handle(&mut session, &message).await {
                    if io.write_all(&answer).await.is_err() {
                        return;
                    }
                }
            } else {
                let number = u16::from_be_bytes([message[0], message[1]]);
                mock.channel_data(&session, number, &message[4..4 + len]).await;
            }
        }
        tokio::select! {
            n = io.read(&mut chunk) => match n {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            },
            Some((_, peer, data)) = from_relays.recv() => {
                if let Some(out) = mock.to_client(&session, peer, &data, true)
                    && io.write_all(&out).await.is_err()
                {
                    return;
                }
            }
        }
    }
}

/// A UDP peer on 127.0.0.1 that sends back each datagram with `echo:` in
/// front.
pub async fn echo_peer() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65_536];
        while let Ok((n, from)) = socket.recv_from(&mut buf).await {
            let out = [b"echo:".as_slice(), &buf[..n]].concat();
            let _ = socket.send_to(&out, from).await;
        }
    });
    addr
}

/// The settings of a client of the UDP mock at `addr`.
pub fn udp_settings(addr: SocketAddr, user: &str, password: &str) -> Settings {
    let mut settings =
        Settings::udp(pod_core::Target::Addr(addr), Credentials::new(user, password), Arc::new(SystemUdp::new()));
    settings.software = Some("pod-turn-test".into());
    settings
}

/// The settings of a client of the TLS mock: the loopback dial of
/// pod-proxy, and a context that trusts the mock's root alone.
pub fn tls_settings(running: &Running, user: &str, password: &str) -> Settings {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-turn-test")));
    let roots = running.roots.clone().unwrap();
    let config = context
        .client_config(&pod_tls::TlsRoots { count: roots.len(), roots, source: "the mock's root".into() })
        .unwrap();
    let dialer = Arc::new(Loopback);
    let mut settings = Settings::tls(
        pod_core::Target::Addr(running.addr),
        Credentials::new(user, password),
        dialer,
        Arc::new(context),
    );
    if let pod_turn::Transport::Tls { trust, server_name, .. } = &mut settings.transport {
        *trust = pod_tls::Trust::caller(config).unwrap();
        *server_name = NAME.into();
    }
    settings
}

/// A dial of TCP to an address of the loopback, with no proxy.
struct Loopback;

impl pod_core::Dial for Loopback {
    fn label(&self) -> String {
        "loopback".into()
    }

    fn dial<'a>(
        &'a self,
        target: &'a pod_core::Target,
        _deadline: tokio::time::Instant,
    ) -> pod_core::BoxFuture<'a, Result<pod_core::Connected, pod_core::Failure>> {
        Box::pin(async move {
            let pod_core::Target::Addr(addr) = target else { panic!("the tests dial addresses") };
            let tcp = tokio::net::TcpStream::connect(addr)
                .await
                .map_err(|e| pod_core::Failure::io(pod_core::Stage::Connect, e))?;
            Ok(pod_core::Connected::new(Box::new(tcp)))
        })
    }
}

/// The requests that the mock read, as (method, answer code).
pub fn seen(control: &Shared) -> Vec<(u16, u16)> {
    control.lock().unwrap().seen.iter().map(|s| (s.method, s.code)).collect()
}

/// A clock that the test moves ahead: tokio's time plus a shift, so a
/// refresh comes due with no wait.
#[derive(Default)]
pub struct Shifted {
    shift: Mutex<Duration>,
}

impl Shifted {
    pub fn ahead(&self, by: Duration) {
        *self.shift.lock().unwrap() += by;
    }

    fn shift(&self) -> Duration {
        *self.shift.lock().unwrap()
    }
}

impl pod_turn::Clock for Shifted {
    fn now(&self) -> tokio::time::Instant {
        tokio::time::Instant::now() + self.shift()
    }

    fn sleep_until(&self, deadline: tokio::time::Instant) -> pod_core::BoxFuture<'_, ()> {
        let real = deadline.checked_sub(self.shift()).unwrap_or_else(tokio::time::Instant::now);
        Box::pin(tokio::time::sleep_until(real))
    }
}

/// A UDP source that hands out one socket, which the test keeps too: a
/// request of the test goes from the client's 5-tuple.
pub struct Shared5Tuple(pub Arc<pod_core::SystemDatagrams>);

impl pod_turn::UdpSource for Shared5Tuple {
    fn open<'a>(
        &'a self,
        server: &'a pod_core::Target,
        _deadline: tokio::time::Instant,
    ) -> pod_core::BoxFuture<'a, Result<pod_turn::Opened, pod_core::Failure>> {
        Box::pin(async move {
            let pod_core::Target::Addr(addr) = server else { panic!("the tests use addresses") };
            Ok((self.0.clone() as Arc<dyn pod_core::Datagrams>, *addr))
        })
    }
}
