//! The TURN client over UDP against the mock server on the loopback: the
//! 401 challenge, the 438 stale nonce, Allocate, Refresh, CreatePermission,
//! ChannelBind, a Send echo, a ChannelData echo, the 441 of a changed user,
//! the refresh on the client's timer, and the errors that name what failed.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{LIMIT, OTHER_PASSWORD, OTHER_USER, PASSWORD, USER, echo_peer, knobs, seen, udp_mock, udp_settings};
use pod_turn::stun::{self, ALLOCATE, CHANNEL_BIND, CREATE_PERMISSION, Class, REFRESH};
use pod_turn::{Client, Error, Event, Kept};

async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(pod_testkit::host::scaled(LIMIT), future).await.expect("the test's bound")
}

/// The first Allocate goes with no credentials; the 401 gives the realm and
/// the nonce; the second Allocate gives the relayed address and the
/// lifetime that the client asked for.
#[tokio::test]
async fn allocate_answers_the_401_and_gives_a_relayed_address() {
    pod_testkit::need_loopback!();
    let mock = udp_mock(knobs(false)).await;
    let client = within(Client::allocate(udp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    assert_eq!(seen(&mock.control), [(ALLOCATE, 401), (ALLOCATE, 0)]);
    let first = mock.control.lock().unwrap().seen[0].clone();
    assert_eq!((first.user, first.nonce), (None, None), "the first Allocate has no credentials");
    assert!(client.relayed_address().ip().is_loopback());
    assert_ne!(client.relayed_address().port(), 0);
    assert!(client.mapped_address().is_some_and(|m| m.ip().is_loopback()));
    assert_eq!(client.lifetime(), Duration::from_secs(600));
    within(client.close()).await.unwrap();
    assert_eq!(seen(&mock.control).last(), Some(&(REFRESH, 0)), "a Refresh with a lifetime of 0 ends it");
}

/// A 438 is answered with the same request, the fresh nonce and the same
/// user; a refresh then holds.
#[tokio::test]
async fn a_stale_nonce_is_answered_with_the_fresh_nonce_and_the_same_user() {
    pod_testkit::need_loopback!();
    let mock = udp_mock(knobs(false)).await;
    let mut client = within(Client::allocate(udp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    mock.control.lock().unwrap().stale_next = 1;
    within(client.refresh()).await.unwrap();
    let control = mock.control.lock().unwrap();
    let tail: Vec<_> = control.seen[2..].to_vec();
    assert_eq!(tail.iter().map(|s| (s.method, s.code)).collect::<Vec<_>>(), [(REFRESH, 438), (REFRESH, 0)]);
    assert_ne!(tail[0].nonce, tail[1].nonce, "the second Refresh carries the fresh nonce");
    assert!(control.seen[1..].iter().all(|s| s.user.as_deref() == Some(USER)), "one user for each request");
}

/// More 438 answers than the limit fail the request by name.
#[tokio::test]
async fn stale_nonces_past_the_limit_fail() {
    pod_testkit::need_loopback!();
    let mock = udp_mock(knobs(false)).await;
    let mut client = within(Client::allocate(udp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    mock.control.lock().unwrap().stale_next = 10;
    let e = within(client.refresh()).await.unwrap_err();
    assert!(matches!(e, Error::StaleNonce { method: REFRESH, tries: 3 }), "{e:?}");
}

/// A permission, then a Send indication to a peer that echoes: the echo
/// comes back in a Data indication.
#[tokio::test]
async fn a_permission_and_a_send_indication_echo() {
    pod_testkit::need_loopback!();
    let (mock, peer) = (udp_mock(knobs(false)).await, echo_peer().await);
    let mut client = within(Client::allocate(udp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    let refused = client.send_to(peer, b"ping").await.unwrap_err();
    assert!(matches!(refused, Error::NoPermission(ip) if ip == peer.ip()));
    within(client.create_permission(peer)).await.unwrap();
    client.send_to(peer, b"ping").await.unwrap();
    match within(client.recv()).await.unwrap() {
        Event::Data { peer: from, channel: None, data } => {
            assert_eq!((from, data.as_slice()), (peer, &b"echo:ping"[..]))
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(mock.control.lock().unwrap().sent, 1);
    assert!(seen(&mock.control).contains(&(CREATE_PERMISSION, 0)));
}

/// A channel binds to 0x4000, and ChannelData to a peer that echoes comes
/// back as ChannelData of the same channel.
#[tokio::test]
async fn a_channel_binds_and_channel_data_echoes() {
    pod_testkit::need_loopback!();
    let (mock, peer) = (udp_mock(knobs(false)).await, echo_peer().await);
    let mut client = within(Client::allocate(udp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    assert!(matches!(client.send(0x4000, b"x").await, Err(Error::NoChannel(0x4000))));
    let number = within(client.bind_channel(peer)).await.unwrap();
    assert_eq!(number, 0x4000);
    assert_eq!(within(client.bind_channel(peer)).await.unwrap(), number, "a bind again keeps the number");
    client.send(number, b"pong").await.unwrap();
    match within(client.recv()).await.unwrap() {
        Event::Data { peer: from, channel, data } => {
            assert_eq!((from, channel, data.as_slice()), (peer, Some(number), &b"echo:pong"[..]));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(mock.control.lock().unwrap().channel_data, 1);
    assert_eq!(seen(&mock.control).iter().filter(|s| **s == (CHANNEL_BIND, 0)).count(), 2);
}

/// The allocation now belongs to another user: the client's Refresh gets
/// 441, the client sends no other user, and a refresh on its timer gives
/// the failure as an event.
#[tokio::test]
async fn a_changed_user_gets_441_and_the_client_keeps_its_user() {
    pod_testkit::need_loopback!();
    let mock = udp_mock(knobs(false)).await;
    let clock = Arc::new(common::Shifted::default());
    let mut settings = udp_settings(mock.addr, USER, PASSWORD);
    settings.clock = clock.clone();
    let mut client = within(Client::allocate(settings)).await.unwrap();
    mock.control.lock().unwrap().owner = Some(OTHER_USER.into());
    let e = within(client.refresh()).await.unwrap_err();
    assert!(matches!(e, Error::Server { method: REFRESH, code: 441, .. }), "{e:?}");
    assert_eq!(e.class(), pod_core::Class::Refused);
    clock.ahead(Duration::from_secs(541));
    match within(client.recv()).await.unwrap() {
        Event::RefreshFailed { kept: Kept::Allocation, error } => assert_eq!(error.code(), Some(441)),
        other => panic!("{other:?}"),
    }
    // Past the end of the lifetime (600 s), the allocation is gone.
    clock.ahead(Duration::from_secs(60));
    assert!(matches!(within(client.recv()).await, Err(Error::NoAllocation)));
    assert!(matches!(client.send_to(mock.addr, b"x").await, Err(Error::NoAllocation)));
    let control = mock.control.lock().unwrap();
    assert!(control.seen[1..].iter().all(|s| s.user.as_deref() == Some(USER)), "the client never changes its user");
    assert_eq!(control.seen.iter().filter(|s| s.code == 441).count(), 2);
}

/// A request of another user from the allocation's 5-tuple gets 441; the
/// answer, which no request of the client waits for, is an event.
#[tokio::test]
async fn a_request_of_another_user_on_the_allocation_gets_441() {
    pod_testkit::need_loopback!();
    let mock = udp_mock(knobs(false)).await;
    let socket = Arc::new(pod_core::SystemDatagrams::bind("127.0.0.1:0".parse().unwrap()).await.unwrap());
    let mut settings = udp_settings(mock.addr, USER, PASSWORD);
    settings.transport = pod_turn::Transport::Udp(Arc::new(common::Shared5Tuple(socket.clone())));
    let mut client = within(Client::allocate(settings)).await.unwrap();
    let nonce = mock.control.lock().unwrap().seen[1].nonce.clone().unwrap();
    let key = stun::long_term_key(OTHER_USER, common::REALM, OTHER_PASSWORD);
    let attributes = [
        (stun::LIFETIME, [0u8, 0, 2, 88].as_slice()),
        (stun::USERNAME, OTHER_USER.as_bytes()),
        (stun::REALM, common::REALM.as_bytes()),
        (stun::NONCE, nonce.as_slice()),
    ];
    let request =
        stun::signed(&stun::message(stun::message_type(REFRESH, Class::Request), &[9; 12], &attributes), &key);
    pod_core::datagrams::send_to(socket.as_ref(), &request, mock.addr).await.unwrap();
    match within(client.recv()).await.unwrap() {
        Event::ServerError { method: REFRESH, code: 441, .. } => {}
        other => panic!("{other:?}"),
    }
    let control = mock.control.lock().unwrap();
    let last = control.seen.last().unwrap();
    assert_eq!((last.method, last.user.as_deref(), last.code), (REFRESH, Some(OTHER_USER), 441));
}

/// The client refreshes the allocation and the permission on its timer
/// while it waits for data (PodUtils/podnet#485, TURN-2), before their end.
#[tokio::test]
async fn the_client_refreshes_on_its_timer() {
    pod_testkit::need_loopback!();
    let (mock, peer) = (udp_mock(knobs(false)).await, echo_peer().await);
    let clock = Arc::new(common::Shifted::default());
    let mut settings = udp_settings(mock.addr, USER, PASSWORD);
    settings.clock = clock.clone();
    let mut client = within(Client::allocate(settings)).await.unwrap();
    within(client.create_permission(peer)).await.unwrap();
    assert_eq!(client.next_refresh().1, Kept::Permission(peer.ip()), "a permission lives 300 s, less than 600 s");
    // 241 s: the permission is due (60 s before its end), the allocation is not.
    clock.ahead(Duration::from_secs(241));
    client.send_to(peer, b"one").await.unwrap();
    assert!(matches!(within(client.recv()).await.unwrap(), Event::Data { .. }));
    assert_eq!(seen(&mock.control)[3..], [(CREATE_PERMISSION, 0)]);
    // 541 s: the allocation is due.
    clock.ahead(Duration::from_secs(300));
    client.send_to(peer, b"two").await.unwrap();
    assert!(matches!(within(client.recv()).await.unwrap(), Event::Data { .. }));
    assert!(seen(&mock.control)[4..].contains(&(REFRESH, 0)));
}

/// A wrong password: the second 401 fails the Allocate; no third try.
#[tokio::test]
async fn a_wrong_password_fails_with_401_and_no_loop() {
    pod_testkit::need_loopback!();
    let mock = udp_mock(knobs(false)).await;
    let e = within(Client::allocate(udp_settings(mock.addr, USER, "not-the-password"))).await.unwrap_err();
    assert!(matches!(e, Error::Server { method: ALLOCATE, code: 401, .. }), "{e:?}");
    assert_eq!(seen(&mock.control), [(ALLOCATE, 401), (ALLOCATE, 401)]);
}

/// A realm that the settings do not expect fails before a credential is
/// sent.
#[tokio::test]
async fn a_realm_that_the_settings_do_not_expect_fails() {
    pod_testkit::need_loopback!();
    let mock = udp_mock(knobs(false)).await;
    let mut settings = udp_settings(mock.addr, USER, PASSWORD);
    settings.realm = Some("other.test".into());
    let e = within(Client::allocate(settings)).await.unwrap_err();
    assert!(matches!(&e, Error::Realm { offered, .. } if offered == common::REALM), "{e:?}");
    assert_eq!(seen(&mock.control), [(ALLOCATE, 401)]);
}

/// A server that answers with bytes that do not decode: the error names
/// the type of the message and the offset. A server that does not answer:
/// the request goes again over UDP, then fails by its method.
#[tokio::test]
async fn a_bad_answer_names_its_type_and_offset_and_no_answer_times_out() {
    pod_testkit::need_loopback!();
    let fake = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = fake.local_addr().unwrap();
    let answer = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        let (n, from) = fake.recv_from(&mut buf).await.unwrap();
        let mut bad = stun::message(0x0113, &buf[8..20].try_into().unwrap(), &[(stun::SOFTWARE, b"abcd".as_slice())]);
        bad[23] = 0x40;
        fake.send_to(&bad, from).await.unwrap();
        let _ = n;
        let mut count = 0;
        while tokio::time::timeout(Duration::from_millis(500), fake.recv_from(&mut buf)).await.is_ok() {
            count += 1;
        }
        count
    });
    let e = within(Client::allocate(udp_settings(addr, USER, PASSWORD))).await.unwrap_err();
    match &e {
        Error::Decode(d) => assert_eq!((d.kind, d.offset), (Some(0x0113), 20), "{e}"),
        other => panic!("{other:?}"),
    }
    assert!(e.to_string().contains("at byte 20 of a message of type 0x0113"), "{e}");
    assert_eq!(within(answer).await.unwrap(), 0);

    let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut settings = udp_settings(silent.local_addr().unwrap(), USER, PASSWORD);
    settings.limits.rto = Duration::from_millis(20);
    settings.limits.retransmits = 2;
    settings.limits.request_timeout = Duration::from_millis(400);
    let e = within(Client::allocate(settings)).await.unwrap_err();
    assert!(matches!(e, Error::Timeout { method: ALLOCATE, .. }), "{e:?}");
    let mut buf = [0u8; 512];
    for _ in 0..3 {
        let (n, _) = within(silent.recv_from(&mut buf)).await.unwrap();
        assert_eq!(&buf[..2], &[0x00, 0x03], "an Allocate request, {n} bytes");
    }
    assert!(silent.try_recv_from(&mut buf).is_err(), "3 sends: the first and 2 more");
}

/// A fake server: the 401 challenge, then a success of Allocate that
/// `sign` signs (or not).
async fn forged(sign: fn(&[u8]) -> Vec<u8>) -> std::net::SocketAddr {
    let fake = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = fake.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        let (_, from) = fake.recv_from(&mut buf).await.unwrap();
        let id: [u8; 12] = buf[8..20].try_into().unwrap();
        let error = [[0u8, 0, 4, 1].as_slice(), b"Unauthorized"].concat();
        let attributes =
            [(stun::ERROR_CODE, error.as_slice()), (stun::REALM, common::REALM.as_bytes()), (stun::NONCE, b"n-1")];
        let challenge = stun::message(stun::message_type(ALLOCATE, Class::Error), &id, &attributes);
        fake.send_to(&challenge, from).await.unwrap();
        let (_, from) = fake.recv_from(&mut buf).await.unwrap();
        let id: [u8; 12] = buf[8..20].try_into().unwrap();
        let relayed = stun::write_address("127.0.0.1:4000".parse().unwrap(), Some(&id));
        let attributes = [(stun::XOR_RELAYED_ADDRESS, relayed.as_slice()), (stun::LIFETIME, &[0, 0, 2, 88])];
        let success = stun::message(stun::message_type(ALLOCATE, Class::Success), &id, &attributes);
        fake.send_to(&sign(&success), from).await.unwrap();
    });
    addr
}

/// A success whose MESSAGE-INTEGRITY is of another key, or that has none,
/// fails by its type: the client takes no forged allocation.
#[tokio::test]
async fn an_answer_with_a_wrong_integrity_or_none_fails() {
    pod_testkit::need_loopback!();
    let other = |bytes: &[u8]| stun::signed(bytes, &stun::long_term_key(USER, common::REALM, "another-password"));
    let addr = forged(other).await;
    let e = within(Client::allocate(udp_settings(addr, USER, PASSWORD))).await.unwrap_err();
    assert!(matches!(e, Error::Integrity { kind: 0x0103, error: stun::IntegrityError::Mismatch }), "{e:?}");
    let addr = forged(<[u8]>::to_vec).await;
    let e = within(Client::allocate(udp_settings(addr, USER, PASSWORD))).await.unwrap_err();
    assert!(matches!(e, Error::Integrity { kind: 0x0103, error: stun::IntegrityError::Missing(_) }), "{e:?}");
    let right = |bytes: &[u8]| stun::signed(bytes, &stun::long_term_key(USER, common::REALM, PASSWORD));
    let addr = forged(right).await;
    let client = within(Client::allocate(udp_settings(addr, USER, PASSWORD))).await.unwrap();
    assert_eq!(client.relayed_address(), "127.0.0.1:4000".parse().unwrap());
}
