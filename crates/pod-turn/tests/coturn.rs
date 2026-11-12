//! The TURN client against coturn, a server that this crate did not write
//! (call C151, T-380): over UDP and over TLS on TCP, the 401 of a static
//! user and its realm, Allocate, Refresh, a Send and a ChannelData echo of a
//! peer on the loopback (with the padding of ChannelData on TLS), the
//! maximum of a lifetime, a stale nonce (438), an Allocate of another user
//! on the same 5-tuple (437), a request of another user (441), a wrong
//! password (401), and a certificate of another root.
//!
//! `scripts/ci/turn-tests.sh` starts coturn on 127.0.0.1 with the login of
//! this file and sets `POD_TURN_TEST_COTURN_UDP`, `POD_TURN_TEST_COTURN_TLS`
//! (`HOST:PORT`) and `POD_TURN_TEST_COTURN_CA` (the PEM of the root of its
//! certificate). With none of them each test skips with the reason, unless
//! `POD_TURN_TEST_COTURN=required`, which the script sets.

mod common;

use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use common::{LIMIT, Loopback, Shared5Tuple, echo_peer};
use pod_core::{Datagrams, RecvMeta, Transmit};
use pod_turn::stun::{self, ALLOCATE, Class, NONCE, PASSWORD_ALGORITHMS, REALM, REFRESH};
use pod_turn::{Client, Credentials, Error, Event, Settings, Transport};

/// The made-up login of `scripts/ci/turn-tests.sh`: coturn's `--user` and
/// `--realm`.
const USER: &str = "pod-turn-test";
const PASSWORD: &str = "made-up-coturn-password";
const REALM_NAME: &str = "coturn.test";
/// A user that coturn does not know.
const OTHER_USER: &str = "pod-turn-other";
/// The name of coturn's certificate.
const NAME: &str = "coturn.test";
/// coturn's `--max-allocate-lifetime`.
const MAX_LIFETIME: Duration = Duration::from_secs(1800);
/// coturn's `--stale-nonce`: the life of a nonce.
const STALE: Duration = Duration::from_secs(2);
/// The longest wait for coturn to free a deleted allocation (1 s measured).
const FREED: Duration = Duration::from_secs(4);

/// Where coturn listens, and the root of its certificate.
struct Coturn {
    udp: SocketAddr,
    tls: SocketAddr,
    ca: PathBuf,
}

/// coturn of the variables, or `None` with a line when they name none.
/// `POD_TURN_TEST_COTURN=required` makes no coturn a failure.
fn coturn() -> Option<Coturn> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let named = (var("POD_TURN_TEST_COTURN_UDP"), var("POD_TURN_TEST_COTURN_TLS"), var("POD_TURN_TEST_COTURN_CA"));
    let (Some(udp), Some(tls), Some(ca)) = named else {
        let why = "POD_TURN_TEST_COTURN_UDP, POD_TURN_TEST_COTURN_TLS and POD_TURN_TEST_COTURN_CA name no coturn";
        assert!(
            std::env::var("POD_TURN_TEST_COTURN").as_deref() != Ok("required"),
            "POD_TURN_TEST_COTURN=required: no coturn here: {why}"
        );
        println!("skipped: {why}");
        return None;
    };
    let address = |v: String| v.parse::<SocketAddr>().expect("HOST:PORT of coturn, an IP address");
    Some(Coturn { udp: address(udp), tls: address(tls), ca: PathBuf::from(ca) })
}

async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(pod_testkit::host::scaled(LIMIT), future).await.expect("the test's bound")
}

/// The settings of a client over UDP through `socket`, with the realm that
/// coturn must offer.
fn udp_settings(coturn: &Coturn, socket: &Arc<pod_core::SystemDatagrams>, user: &str, password: &str) -> Settings {
    let source = Arc::new(Shared5Tuple(socket.clone()));
    let mut settings = Settings::udp(pod_core::Target::Addr(coturn.udp), Credentials::new(user, password), source);
    settings.realm = Some(REALM_NAME.into());
    settings.software = Some("pod-turn-coturn-test".into());
    settings
}

/// The settings of a client over TLS: the loopback dial, and the root of
/// the script alone, or the default roots.
fn tls_settings(coturn: &Coturn, own_root: bool) -> Settings {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-turn-test")));
    let target = pod_core::Target::Addr(coturn.tls);
    let mut settings = Settings::tls(target, Credentials::new(USER, PASSWORD), Arc::new(Loopback), Arc::new(context));
    if let Transport::Tls { trust, server_name, .. } = &mut settings.transport {
        if own_root {
            *trust = pod_tls::Trust::File(coturn.ca.clone());
        }
        *server_name = NAME.into();
    }
    settings.realm = Some(REALM_NAME.into());
    settings
}

async fn bound_socket() -> Arc<pod_core::SystemDatagrams> {
    Arc::new(pod_core::SystemDatagrams::bind("127.0.0.1:0".parse().unwrap()).await.unwrap())
}

/// The next event must be `echo:` and `sent` from `peer`, on `channel`.
async fn echoed(client: &mut Client, peer: SocketAddr, channel: Option<u16>, sent: &[u8]) {
    match within(client.recv()).await.unwrap() {
        Event::Data { peer: from, channel: on, data } => {
            assert_eq!((from, on), (peer, channel));
            assert_eq!(data, [b"echo:".as_slice(), sent].concat());
        }
        other => panic!("{other:?}"),
    }
}

/// A permission, a Send echo, a channel, and a ChannelData echo of each
/// length class: on TLS, 3 and 1001 bytes take padding, 0 and 4 none.
async fn relay_both_ways(client: &mut Client, peer: SocketAddr) {
    within(client.create_permission(peer)).await.unwrap();
    client.send_to(peer, b"abc").await.unwrap();
    echoed(client, peer, None, b"abc").await;
    let number = within(client.bind_channel(peer)).await.unwrap();
    for data in [&b"xyz"[..], b"", b"four", &[7u8; 1001]] {
        client.send(number, data).await.unwrap();
        echoed(client, peer, Some(number), data).await;
    }
}

/// Over UDP: the 401 and the MD5 key, the relayed address and the mapped
/// address of the socket on 127.0.0.1, the lifetime asked for, the relay of
/// a peer both ways, a Refresh and the close. A new Allocate on the same
/// 5-tuple holds within 4 s of the close: the close ended the allocation,
/// which else lives 600 s.
#[tokio::test]
async fn an_allocation_over_udp_relays_both_ways() {
    pod_testkit::need_loopback!();
    let Some(coturn) = coturn() else { return };
    let (socket, peer) = (bound_socket().await, echo_peer().await);
    let local = socket.socket().local_addr().unwrap();
    let mut client = within(Client::allocate(udp_settings(&coturn, &socket, USER, PASSWORD))).await.unwrap();
    assert_eq!(client.relayed_address().ip(), coturn.udp.ip());
    assert_ne!(client.relayed_address().port(), 0);
    assert_eq!(client.mapped_address(), Some(local));
    assert_eq!(client.lifetime(), Duration::from_secs(600));
    relay_both_ways(&mut client, peer).await;
    within(client.refresh()).await.unwrap();
    within(client.close()).await.unwrap();
    // coturn frees a deleted allocation at its next tick of 1 s; before it,
    // an Allocate on the same 5-tuple gets 437 (RFC 8656, 7.4).
    let start = tokio::time::Instant::now();
    loop {
        match within(Client::allocate(udp_settings(&coturn, &socket, USER, PASSWORD))).await {
            Ok(again) => break within(again.close()).await.unwrap(),
            Err(Error::Server { method: ALLOCATE, code: 437, .. }) if start.elapsed() < FREED => {}
            Err(e) => panic!("an Allocate after the close, at {:?}: {e:?}", start.elapsed()),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    println!("coturn: an allocation over UDP relayed a Send and ChannelData");
}

/// Over TLS on TCP: the same, and each ChannelData both ways with its
/// padding, cut by its own length.
#[tokio::test]
async fn an_allocation_over_tls_relays_both_ways() {
    pod_testkit::need_loopback!();
    let Some(coturn) = coturn() else { return };
    let peer = echo_peer().await;
    let mut client = within(Client::allocate(tls_settings(&coturn, true))).await.unwrap();
    assert_eq!(client.relayed_address().ip(), coturn.tls.ip());
    assert_eq!(client.lifetime(), Duration::from_secs(600));
    relay_both_ways(&mut client, peer).await;
    within(client.refresh()).await.unwrap();
    within(client.close()).await.unwrap();
    println!("coturn: an allocation over TLS relayed a Send and padded ChannelData");
}

/// A lifetime past coturn's maximum gets the maximum, at the Allocate and
/// at a Refresh.
#[tokio::test]
async fn a_lifetime_past_the_maximum_gets_the_maximum() {
    pod_testkit::need_loopback!();
    let Some(coturn) = coturn() else { return };
    let mut settings = udp_settings(&coturn, &bound_socket().await, USER, PASSWORD);
    settings.limits.lifetime = MAX_LIFETIME * 4;
    let mut client = within(Client::allocate(settings)).await.unwrap();
    assert_eq!(client.lifetime(), MAX_LIFETIME);
    within(client.refresh()).await.unwrap();
    assert_eq!(client.lifetime(), MAX_LIFETIME);
    within(client.close()).await.unwrap();
    println!("coturn: a lifetime past the maximum got the maximum");
}

/// After the life of a nonce, a Refresh gets 438. A client that answers no
/// 438 fails with it. A client that answers it sends the Refresh again with
/// the fresh nonce, and the Refresh holds: its socket counts the 438. The
/// first 438 comes 2 s to 4 s after the nonce: coturn's clock of each
/// thread ticks each second.
#[tokio::test]
async fn a_stale_nonce_gets_438_and_the_request_goes_again() {
    pod_testkit::need_loopback!();
    let Some(coturn) = coturn() else { return };
    let mut strict = udp_settings(&coturn, &bound_socket().await, USER, PASSWORD);
    strict.limits.stale_nonces = 0;
    let mut strict = within(Client::allocate(strict)).await.unwrap();
    let counted = Arc::new(Counted438::bind().await);
    let mut settings = udp_settings(&coturn, &bound_socket().await, USER, PASSWORD);
    settings.transport = Transport::Udp(Arc::new(CountedSource(counted.clone())));
    let mut client = within(Client::allocate(settings)).await.unwrap();
    let start = tokio::time::Instant::now();
    let e = loop {
        assert!(start.elapsed() < STALE * 4, "no 438 within {:?}", STALE * 4);
        tokio::time::sleep(Duration::from_millis(250)).await;
        if let Err(e) = within(strict.refresh()).await {
            break e;
        }
    };
    assert!(matches!(e, Error::StaleNonce { method: REFRESH, tries: 1 }), "{e:?}");
    while counted.stale.load(Ordering::Relaxed) == 0 {
        assert!(start.elapsed() < STALE * 4, "no 438 within {:?}", STALE * 4);
        within(client.refresh()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    within(client.close()).await.unwrap();
    println!("coturn: a stale nonce got 438, and the request went again");
}

/// A UDP socket on 127.0.0.1 that counts each 438 answer that it receives.
#[derive(Debug)]
struct Counted438 {
    socket: pod_core::SystemDatagrams,
    stale: AtomicUsize,
}

impl Counted438 {
    async fn bind() -> Counted438 {
        let socket = pod_core::SystemDatagrams::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
        Counted438 { socket, stale: AtomicUsize::new(0) }
    }
}

impl Datagrams for Counted438 {
    fn poll_writable(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.socket.poll_writable(cx)
    }

    fn try_send(&self, transmit: &Transmit<'_>) -> io::Result<()> {
        self.socket.try_send(transmit)
    }

    fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        let polled = self.socket.poll_recv(cx, bufs, meta);
        if let Poll::Ready(Ok(n)) = &polled {
            for (buf, meta) in bufs.iter().zip(meta.iter()).take(*n) {
                let error = stun::decode(&buf[..meta.len]).ok().and_then(|m| m.error()).and_then(Result::ok);
                if error.is_some_and(|(code, _)| code == 438) {
                    self.stale.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        polled
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

/// The client's source of the counted socket.
struct CountedSource(Arc<Counted438>);

impl pod_turn::UdpSource for CountedSource {
    fn open<'a>(
        &'a self,
        server: &'a pod_core::Target,
        _deadline: tokio::time::Instant,
    ) -> pod_core::BoxFuture<'a, Result<pod_turn::Opened, pod_core::Failure>> {
        Box::pin(async move {
            let pod_core::Target::Addr(addr) = server else { panic!("the tests use addresses") };
            Ok((self.0.clone() as Arc<dyn Datagrams>, *addr))
        })
    }
}

/// On the 5-tuple of an allocation: an Allocate of another user gets 437,
/// and a Refresh of another user gets 441, which comes to the client as an
/// event. coturn's 401 offers no PASSWORD-ALGORITHMS, so the client keys
/// with MD5. The allocation stays the client's.
#[tokio::test]
async fn another_user_on_the_5_tuple_gets_437_and_441() {
    pod_testkit::need_loopback!();
    let Some(coturn) = coturn() else { return };
    let socket = bound_socket().await;
    let mut client = within(Client::allocate(udp_settings(&coturn, &socket, USER, PASSWORD))).await.unwrap();
    let other = Client::allocate(udp_settings(&coturn, &socket, OTHER_USER, PASSWORD));
    let e = within(other).await.unwrap_err();
    assert!(matches!(e, Error::Server { method: ALLOCATE, code: 437, .. }), "{e:?}");
    // coturn 4.18 sends no reason phrase (4.6 sends one): the line names the
    // code either way.
    assert!(e.to_string().contains("with 437 (") && e.to_string().ends_with(')'), "{e}");
    println!("coturn: an Allocate of another user on the same 5-tuple got 437");

    // A Refresh with no credentials gets the 401 with a nonce.
    let refresh = stun::message_type(REFRESH, Class::Request);
    let ask = stun::message(refresh, &[1; 12], &[(stun::LIFETIME, [0u8, 0, 2, 88].as_slice())]);
    pod_core::datagrams::send_to(socket.as_ref(), &ask, coturn.udp).await.unwrap();
    let mut buf = vec![0u8; 1500];
    let (n, _) = within(pod_core::datagrams::recv_from(socket.as_ref(), &mut buf)).await.unwrap();
    let read = stun::decode(&buf[..n]).unwrap();
    assert_eq!((read.kind, read.id), (stun::message_type(REFRESH, Class::Error), [1; 12]));
    assert_eq!(read.error().unwrap().unwrap().0, 401);
    assert_eq!(read.attribute(REALM), Some(REALM_NAME.as_bytes()));
    assert!(read.attribute(PASSWORD_ALGORITHMS).is_none(), "coturn offers no SHA-256");
    assert!(stun::fingerprint_ok(&buf[..n]), "coturn runs with --fingerprint");
    let nonce = read.attribute(NONCE).unwrap().to_vec();

    let key = stun::long_term_key(OTHER_USER, REALM_NAME, PASSWORD);
    let attributes = [
        (stun::LIFETIME, [0u8, 0, 2, 88].as_slice()),
        (stun::USERNAME, OTHER_USER.as_bytes()),
        (REALM, REALM_NAME.as_bytes()),
        (NONCE, nonce.as_slice()),
    ];
    let request = stun::signed(&stun::message(refresh, &[2; 12], &attributes), &key);
    pod_core::datagrams::send_to(socket.as_ref(), &request, coturn.udp).await.unwrap();
    match within(client.recv()).await.unwrap() {
        Event::ServerError { method: REFRESH, code: 441, .. } => {}
        other => panic!("{other:?}"),
    }
    within(client.refresh()).await.unwrap();
    within(client.close()).await.unwrap();
    println!("coturn: a Refresh of another user got 441");
}

/// A wrong password: the second 401 fails the Allocate.
#[tokio::test]
async fn a_wrong_password_gets_401() {
    pod_testkit::need_loopback!();
    let Some(coturn) = coturn() else { return };
    let settings = udp_settings(&coturn, &bound_socket().await, USER, "not-the-password");
    let e = within(Client::allocate(settings)).await.unwrap_err();
    assert!(matches!(e, Error::Server { method: ALLOCATE, code: 401, .. }), "{e:?}");
}

/// The default roots do not hold the script's root: the TLS stage fails.
#[tokio::test]
async fn a_certificate_of_another_root_is_refused() {
    pod_testkit::need_loopback!();
    let Some(coturn) = coturn() else { return };
    let e = within(Client::allocate(tls_settings(&coturn, false))).await.unwrap_err();
    match &e {
        Error::Connect(failure) => assert_eq!(failure.stage, pod_core::Stage::Tls, "{e}"),
        other => panic!("{other:?}"),
    }
    println!("coturn: a certificate of another root was refused");
}
