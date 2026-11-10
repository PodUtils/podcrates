//! The race of handshakes (podssh's `doctor`), against a stand-in server on
//! the loopback: QUIC at both ends through noq, both TLS handshakes on ring,
//! the stand-in's certificate minted in pure Rust under a CA of the test,
//! which the client trusts.

mod common;

use std::net::{Ipv4Addr, UdpSocket};

use common::{LIMIT, NAME, server, tls};
use pod_quic::connect;
use pod_quic::{HANDSHAKE_WAIT, Settings};
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread")]
async fn doctors_handshake_answers_and_closes_with_nothing_registered() {
    pod_testkit::need_loopback!();
    let (edge, addr, roots) = server();
    let probe = tokio::spawn(async move { connect::handshake(&[addr], NAME, tls(&roots), &Settings::default()).await });
    let incoming = timeout(LIMIT, edge.accept()).await.unwrap().expect("a connection");
    let at_edge = timeout(LIMIT, incoming).await.unwrap().expect("the handshake");
    let (answered, took) = timeout(LIMIT, probe).await.unwrap().unwrap().expect("an answer");
    assert_eq!(answered, addr);
    assert!(took < HANDSHAKE_WAIT, "{took:?}");
    // Closed at once, with no stream: nothing is registered at the edge.
    assert!(timeout(LIMIT, at_edge.accept_bi()).await.expect("an end in time").is_err(), "a stream was opened");
    let closed = timeout(LIMIT, at_edge.closed()).await.expect("closed in time");
    assert!(matches!(closed, pod_quic::noq::ConnectionError::ApplicationClosed(_)), "{closed:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn doctors_handshake_with_a_silent_edge_says_that_none_answered() {
    pod_testkit::need_loopback!();
    // A port that takes each datagram and answers none, as a network that
    // drops UDP further on.
    let silent = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let (_edge, _, roots) = server();
    let started = std::time::Instant::now();
    let why =
        timeout(LIMIT, connect::handshake(&[silent.local_addr().unwrap()], NAME, tls(&roots), &Settings::default()))
            .await
            .unwrap()
            .expect_err("no answer from a silent edge");
    let took = started.elapsed();
    assert!(
        why.to_string().contains("no address answered over QUIC") && why.to_string().contains("within 5 s"),
        "{why}"
    );
    assert_eq!((why.stage, why.class), (pod_core::Stage::Quic, pod_core::Class::Timeout));
    let wait = HANDSHAKE_WAIT;
    assert!(took >= wait - std::time::Duration::from_millis(100) && took < wait * 2, "{took:?}");
    drop(silent);
}

/// The addresses at once, where a node tries them in turn: a silent first
/// address costs no wait. Linux only, where all of 127/8 is the loopback.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn doctors_handshake_tries_the_regions_at_once() {
    pod_testkit::need_loopback!();
    let (edge, addr, roots) = server();
    let silent = UdpSocket::bind((Ipv4Addr::new(127, 0, 0, 2), addr.port())).unwrap();
    let regions = [silent.local_addr().unwrap(), addr];
    let started = std::time::Instant::now();
    let probe =
        tokio::spawn(async move { connect::handshake(&regions, NAME, tls(&roots), &Settings::default()).await });
    let incoming = timeout(LIMIT, edge.accept()).await.unwrap().expect("a connection");
    let _at_edge = timeout(LIMIT, incoming).await.unwrap().expect("the handshake");
    let (answered, _) = timeout(LIMIT, probe).await.unwrap().unwrap().expect("an answer");
    let took = started.elapsed();
    assert_eq!(answered, addr, "the region that answers, not the silent one");
    assert!(took < HANDSHAKE_WAIT, "the silent region was waited for: {took:?}");
    drop(silent);
}

/// A server whose certificate the client does not trust: a failure of
/// QUIC at once, not a wait.
#[tokio::test(flavor = "multi_thread")]
async fn an_untrusted_server_fails_the_connection_at_once() {
    pod_testkit::need_loopback!();
    let (edge, addr, _) = server();
    let other_roots = common::other_roots();
    // The stand-in takes the handshake, which the client then ends.
    tokio::spawn(async move {
        if let Some(incoming) = edge.accept().await {
            let _ = incoming.await;
        }
    });
    let endpoint = pod_quic::endpoint::system(false).unwrap();
    let started = std::time::Instant::now();
    let why = connect::connect(&endpoint, addr, NAME, tls(&other_roots), &Settings::default())
        .await
        .expect_err("an untrusted certificate");
    assert!(started.elapsed() < HANDSHAKE_WAIT, "{:?}", started.elapsed());
    assert_eq!(why.stage, pod_core::Stage::Quic);
    assert_ne!(why.class, pod_core::Class::Timeout, "{why}");
}
