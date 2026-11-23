//! The TCP relay of RFC 6062 over the mock's plain TCP face: a TCP
//! allocation, Connect to a peer, ConnectionBind on a second connection,
//! the bytes both ways, the refresh of the allocation while the
//! stream lives, a ConnectionAttempt of a peer that dialed the relayed
//! address, and each named failure.

mod common;

use std::net::SocketAddr;
use std::time::Duration;

use common::mock_tcp::{echo_peer, plain_echo_peer, tcp_mock};
use common::{LIMIT, PASSWORD, USER, knobs, seen, tcp_settings, udp_mock, udp_settings};
use pod_turn::stun::{ALLOCATE, CONNECT, REFRESH};
use pod_turn::{Client, Error};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(pod_testkit::host::scaled(LIMIT), future).await.expect("the test's bound")
}

/// Read exactly `n` bytes of the relayed stream.
async fn read_n<S: tokio::io::AsyncRead + Unpin>(stream: &mut S, n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    stream.read_exact(&mut out).await.unwrap();
    out
}

/// An allocation of REQUESTED-TRANSPORT TCP, a Connect to an echoing peer,
/// and bytes both ways. The status names the lifetime and the rate
/// limit of the caller.
#[tokio::test]
async fn an_allocation_over_tcp_relays_a_connection_to_a_peer() {
    pod_testkit::need_loopback!();
    let mock = tcp_mock(knobs(false)).await;
    let peer = echo_peer().await;
    let mut settings = tcp_settings(mock.addr, USER, PASSWORD);
    settings.rate_limit = Some(1_000_000);
    let client = within(Client::allocate(settings)).await.unwrap();
    assert!(client.relayed_address().ip().is_loopback());
    assert_eq!(client.lifetime(), Duration::from_secs(600));
    assert_eq!(client.status().rate_limit, Some(1_000_000));
    let mut stream = within(client.connect(peer)).await.unwrap();
    assert_eq!(stream.peer(), peer);
    assert_eq!(stream.status().lifetime, Duration::from_secs(600));
    within(stream.write_frame(b"ping")).await.unwrap();
    assert_eq!(within(read_n(&mut stream, b"echo:ping".len())).await.as_slice(), b"echo:ping");
    assert!(seen(&mock.control).contains(&(CONNECT, 0)));
}

/// The allocation is refreshed on its timer while the stream is open, and
/// the status follows the lifetime that the server gives.
#[tokio::test]
async fn the_stream_refreshes_the_allocation_on_its_timer() {
    pod_testkit::need_loopback!();
    let mut knobs = knobs(false);
    knobs.max_lifetime = 2;
    let mock = tcp_mock(knobs).await;
    let peer = echo_peer().await;
    let mut settings = tcp_settings(mock.addr, USER, PASSWORD);
    settings.limits.lifetime = Duration::from_secs(2);
    settings.limits.refresh_before = Duration::from_secs(1);
    let client = within(Client::allocate(settings)).await.unwrap();
    let mut stream = within(client.connect(peer)).await.unwrap();
    assert_eq!(stream.status().lifetime, Duration::from_secs(2));
    let start = tokio::time::Instant::now();
    while !seen(&mock.control).contains(&(REFRESH, 0)) {
        assert!(start.elapsed() < Duration::from_secs(5), "no Refresh on its timer: {:?}", seen(&mock.control));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    within(stream.write_frame(b"still")).await.unwrap();
    assert_eq!(within(read_n(&mut stream, b"echo:still".len())).await.as_slice(), b"echo:still");
    assert_eq!(stream.status().lifetime, Duration::from_secs(2));
}

/// A peer that dials the relayed address, with a permission in place, gives
/// a ConnectionAttempt; Accept binds it and carries the bytes both
/// ways.
#[tokio::test]
async fn a_connection_attempt_binds_a_peer_that_dialed_the_relay() {
    pod_testkit::need_loopback!();
    let mock = tcp_mock(knobs(false)).await;
    let client = within(Client::allocate(tcp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    let relayed = client.relayed_address();
    let mut client = client;
    within(client.create_permission(SocketAddr::new(relayed.ip(), 1))).await.unwrap();
    let mut peer = TcpStream::connect(relayed).await.unwrap();
    let mut stream = within(client.accept()).await.unwrap();
    peer.write_all(b"hi").await.unwrap();
    assert_eq!(within(read_n(&mut stream, 2)).await.as_slice(), b"hi");
    within(stream.write_frame(b"yo")).await.unwrap();
    let mut data = [0u8; 2];
    peer.read_exact(&mut data).await.unwrap();
    assert_eq!(&data, b"yo");
}

/// The mock keeps the pending connection under another id than the one that
/// it gave: the ConnectionBind is refused by its code.
#[tokio::test]
async fn a_bind_with_a_wrong_connection_id_is_refused() {
    pod_testkit::need_loopback!();
    let mut knobs = knobs(false);
    knobs.bind_salt = 1;
    let mock = tcp_mock(knobs).await;
    let peer = echo_peer().await;
    let client = within(Client::allocate(tcp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    let e = within(client.connect(peer)).await.unwrap_err();
    assert!(matches!(e, Error::BindRefused { code: 400, .. }), "{e:?}");
}

/// A write larger than one read buffer arrives whole: the relay frames
/// nothing, so no length prefix is added.
#[tokio::test]
async fn a_large_write_arrives_whole_and_unframed() {
    pod_testkit::need_loopback!();
    let mock = tcp_mock(knobs(false)).await;
    let peer = plain_echo_peer().await;
    let client = within(Client::allocate(tcp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    let mut stream = within(client.connect(peer)).await.unwrap();
    let data = vec![0x5a; 100_000];
    within(stream.write_frame(&data)).await.unwrap();
    let got = within(read_n(&mut stream, data.len())).await;
    assert_eq!(got, data);
}

/// A peer that closes ends the relayed stream by name.
#[tokio::test]
async fn a_closed_relayed_stream_is_named() {
    pod_testkit::need_loopback!();
    let mock = tcp_mock(knobs(false)).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            drop(stream);
        }
    });
    let client = within(Client::allocate(tcp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    let mut stream = within(client.connect(peer)).await.unwrap();
    let e = within(stream.read_frame()).await.unwrap_err();
    assert!(matches!(e, Error::RelayedClosed), "{e:?}");
}

/// A server that refuses the Connect names the code that it gave.
#[tokio::test]
async fn a_refused_connect_names_its_code() {
    pod_testkit::need_loopback!();
    let mut knobs = knobs(false);
    knobs.refuse_connect = Some(447);
    let mock = tcp_mock(knobs).await;
    let peer = echo_peer().await;
    let client = within(Client::allocate(tcp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    let e = within(client.connect(peer)).await.unwrap_err();
    assert!(matches!(e, Error::ConnectRefused { code: 447, .. }), "{e:?}");
    assert_eq!(e.class(), pod_core::Class::Timeout);
}

/// RFC 6062 needs a TCP allocation: a UDP allocation refuses Connect by
/// name, before a send.
#[tokio::test]
async fn a_connect_of_a_udp_allocation_is_refused() {
    pod_testkit::need_loopback!();
    let mock = udp_mock(knobs(false)).await;
    let client = within(Client::allocate(udp_settings(mock.addr, USER, PASSWORD))).await.unwrap();
    let e = within(client.connect("127.0.0.1:9".parse().unwrap())).await.unwrap_err();
    assert!(matches!(e, Error::NoTcpAllocation), "{e:?}");
}

/// With no peer, Accept waits for the limit and names the timeout.
#[tokio::test]
async fn accept_with_no_connection_attempt_times_out() {
    pod_testkit::need_loopback!();
    let mock = tcp_mock(knobs(false)).await;
    let mut settings = tcp_settings(mock.addr, USER, PASSWORD);
    settings.limits.connection_attempt_timeout = Duration::from_millis(100);
    let client = within(Client::allocate(settings)).await.unwrap();
    let e = within(client.accept()).await.unwrap_err();
    assert!(matches!(e, Error::ConnectionAttemptTimeout { .. }), "{e:?}");
}

/// An allocation whose lifetime ended has no allocation to connect with.
#[tokio::test]
async fn a_connect_with_no_allocation_is_refused() {
    pod_testkit::need_loopback!();
    let mut knobs = knobs(false);
    knobs.max_lifetime = 1;
    let mock = tcp_mock(knobs).await;
    let clock = std::sync::Arc::new(common::Shifted::default());
    let mut settings = tcp_settings(mock.addr, USER, PASSWORD);
    settings.limits.lifetime = Duration::from_secs(1);
    settings.clock = clock.clone();
    let client = within(Client::allocate(settings)).await.unwrap();
    assert_eq!(seen(&mock.control).last(), Some(&(ALLOCATE, 0)));
    clock.ahead(Duration::from_secs(2));
    let e = within(client.connect("127.0.0.1:9".parse().unwrap())).await.unwrap_err();
    assert!(matches!(e, Error::NoAllocation), "{e:?}");
}
