//! The TURN client over TLS on TCP against the mock server on the
//! loopback, with the SHA-256 credentials: the 401 with
//! PASSWORD-ALGORITHMS, the 438, Allocate, Refresh, a Send echo, a
//! ChannelData echo with its padding, a 441, and a certificate that the
//! client does not trust.

mod common;

use std::time::Duration;

use common::{LIMIT, OTHER_USER, PASSWORD, USER, echo_peer, knobs, seen, tls_mock, tls_settings};
use pod_turn::stun::{ALLOCATE, CHANNEL_BIND, PasswordAlgorithm, REFRESH};
use pod_turn::{Client, Error, Event};

async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(pod_testkit::host::scaled(LIMIT), future).await.expect("the test's bound")
}

/// Over TLS with SHA-256: the challenge offers SHA-256 and MD5, the client
/// takes SHA-256, sends the offer back, and signs with
/// MESSAGE-INTEGRITY-SHA256; a 438 goes again with the fresh nonce.
#[tokio::test]
async fn allocate_and_refresh_over_tls_with_sha256() {
    pod_testkit::need_loopback!();
    let mock = tls_mock(knobs(true)).await;
    let mut client = within(Client::allocate(tls_settings(&mock, USER, PASSWORD))).await.unwrap();
    assert!(client.relayed_address().ip().is_loopback());
    assert_eq!(client.lifetime(), Duration::from_secs(600));
    mock.control.lock().unwrap().stale_next = 1;
    within(client.refresh()).await.unwrap();
    assert_eq!(seen(&mock.control), [(ALLOCATE, 401), (ALLOCATE, 0), (REFRESH, 438), (REFRESH, 0)]);
    {
        let control = mock.control.lock().unwrap();
        let sha256 = Some(PasswordAlgorithm::Sha256.number());
        assert!(control.seen[1..].iter().all(|s| s.algorithm == sha256 && s.user.as_deref() == Some(USER)));
        assert_ne!(control.seen[2].nonce, control.seen[3].nonce);
    }
    within(client.close()).await.unwrap();
}

/// A Send echo and a ChannelData echo over the stream: data of 3 bytes
/// takes 1 byte of padding each way, and both ends cut each message by its
/// own length.
#[tokio::test]
async fn a_send_echo_and_a_channel_data_echo_over_tls() {
    pod_testkit::need_loopback!();
    let (mock, peer) = (tls_mock(knobs(true)).await, echo_peer().await);
    let mut client = within(Client::allocate(tls_settings(&mock, USER, PASSWORD))).await.unwrap();
    within(client.create_permission(peer)).await.unwrap();
    client.send_to(peer, b"abc").await.unwrap();
    match within(client.recv()).await.unwrap() {
        Event::Data { peer: from, channel: None, data } => {
            assert_eq!((from, data.as_slice()), (peer, &b"echo:abc"[..]))
        }
        other => panic!("{other:?}"),
    }
    let number = within(client.bind_channel(peer)).await.unwrap();
    for data in [&b"xyz"[..], b"", b"four", &[7u8; 1001]] {
        client.send(number, data).await.unwrap();
        match within(client.recv()).await.unwrap() {
            Event::Data { peer: from, channel, data: back } => {
                assert_eq!((from, channel), (peer, Some(number)));
                assert_eq!(back, [b"echo:".as_slice(), data].concat());
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(seen(&mock.control).contains(&(CHANNEL_BIND, 0)));
    assert_eq!(mock.control.lock().unwrap().channel_data, 4);
}

/// A 441 over TLS: the allocation belongs to another user now.
#[tokio::test]
async fn a_changed_user_gets_441_over_tls() {
    pod_testkit::need_loopback!();
    let mock = tls_mock(knobs(false)).await;
    let mut client = within(Client::allocate(tls_settings(&mock, USER, PASSWORD))).await.unwrap();
    mock.control.lock().unwrap().owner = Some(OTHER_USER.into());
    let e = within(client.refresh()).await.unwrap_err();
    assert!(matches!(e, Error::Server { method: REFRESH, code: 441, .. }), "{e:?}");
    assert_eq!(seen(&mock.control).last(), Some(&(REFRESH, 441)));
}

/// A server whose certificate the client does not trust: the TLS stage
/// fails, and no request goes.
#[tokio::test]
async fn an_untrusted_certificate_fails_at_tls() {
    pod_testkit::need_loopback!();
    let mock = tls_mock(knobs(false)).await;
    let mut settings = tls_settings(&mock, USER, PASSWORD);
    if let pod_turn::Transport::Tls { trust, .. } = &mut settings.transport {
        *trust = pod_tls::Trust::Default;
    }
    let e = within(Client::allocate(settings)).await.unwrap_err();
    match &e {
        Error::Connect(failure) => assert_eq!(failure.stage, pod_core::Stage::Tls, "{e}"),
        other => panic!("{other:?}"),
    }
    assert!(seen(&mock.control).is_empty());
}
