//! The caps of the accept loop of `pod_ws::serve` (T-439, GitHub #116):
//! with 64 connections before `ready` from 127.0.0.1, the next accept from
//! it is closed and one from 127.0.0.2 is taken; with `max_unauth` full, an
//! accept evicts the oldest connection of the earliest stage, also one in
//! the hands of the caller.

mod common;

use std::net::{IpAddr, Ipv4Addr};

use common::serve::{cut, id, long, next, session_from, start, tcp_from, tls_over, upgrade};

const SECOND: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

#[tokio::test]
async fn an_accept_over_the_cap_of_its_prefix_is_closed_and_another_prefix_is_taken() {
    pod_testkit::need_loopback!();
    let mut cfg = long();
    cfg.caps.per_prefix = 64;
    let (addr, mut rx, _loop) = start(cfg).await;
    // 64 connections silent in TLS, in the order of the accepts.
    let mut held = Vec::new();
    for _ in 0..64 {
        held.push(tcp_from(None, addr).await.unwrap());
    }
    let mut over = tcp_from(None, addr).await.unwrap();
    assert!(cut(&mut over).await, "the 65th accept of 127.0.0.1 is closed");
    let binds = tokio::net::TcpSocket::new_v4().unwrap().bind((SECOND, 0).into()).is_ok();
    if !binds {
        eprintln!("skip the second prefix: this host binds no 127.0.0.2");
        return;
    }
    let _taken = session_from(Some(SECOND), addr).await;
    let accepted = next(&mut rx).await;
    assert_eq!(accepted.peer.ip(), SECOND);
    assert_eq!(accepted.key, Some(id(2).public()));
    // Each of the 64 is still open: none was cut for the other prefix.
    let mut byte = [0u8; 1];
    for tcp in &mut held {
        assert!(tcp.try_read(&mut byte).is_err(), "a held connection is open with nothing to read");
    }
}

#[tokio::test]
async fn a_full_loop_evicts_the_oldest_connection_of_the_earliest_stage() {
    pod_testkit::need_loopback!();
    let mut cfg = long();
    cfg.caps.max_unauth = 4;
    cfg.caps.per_prefix = 8;
    let (addr, mut rx, _loop) = start(cfg).await;
    // c and d wait for their first frame; a and b are silent in TLS. c is
    // the oldest, a the oldest of the earliest stage.
    let mut c = tls_over(tcp_from(None, addr).await.unwrap(), 2).await.unwrap();
    assert!(upgrade(&mut c).await.is_empty());
    let mut d = tls_over(tcp_from(None, addr).await.unwrap(), 2).await.unwrap();
    assert!(upgrade(&mut d).await.is_empty());
    let mut a = tcp_from(None, addr).await.unwrap();
    let b = tcp_from(None, addr).await.unwrap();
    let _e = tcp_from(None, addr).await.unwrap();
    assert!(cut(&mut a).await, "the oldest of the stage TLS is evicted");

    // b, c and d were not: each goes on to the caller.
    let mut b = tls_over(b, 2).await.unwrap();
    let pending = upgrade(&mut b).await;
    let mut kept = Vec::new();
    for (name, stream) in [("c", c), ("d", d), ("b", b)] {
        let session = pod_ws::RelaySession::new(stream, Vec::new(), None, common::serve::LIMIT);
        session.send_binary(name.as_bytes()).await.unwrap();
        let accepted = next(&mut rx).await;
        assert_eq!(accepted.session.read_frame().await.unwrap().payload, name.as_bytes());
        accepted.gate.ready();
        kept.push((session, accepted.session));
    }
    assert!(pending.is_empty());
}

#[tokio::test]
async fn a_session_in_the_hands_of_the_caller_is_evicted_when_it_is_the_only_one() {
    pod_testkit::need_loopback!();
    let mut cfg = long();
    cfg.caps.max_unauth = 1;
    let (addr, mut rx, _loop) = start(cfg).await;
    let held = session_from(None, addr).await;
    let accepted = next(&mut rx).await;
    let later = tcp_from(None, addr).await.unwrap();
    let e = tokio::time::timeout(common::serve::LIMIT, held.read_frame()).await.expect("the eviction cuts");
    assert!(e.is_err(), "the session before ready is evicted for the new accept");
    // The eviction freed the slot for the new accept, which waits in TLS.
    let mut byte = [0u8; 1];
    assert!(later.try_read(&mut byte).is_err(), "the new accept is open");
    drop(accepted);
}
