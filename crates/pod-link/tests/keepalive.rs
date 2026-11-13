//! The keepalive, on a paused clock: a `PING` after each silence, the end
//! of a link whose peer is silent, and a quiet link that lives.

mod common;

use std::time::Duration;

use common::{RawPeer, echo, limit};
use pod_link::{Error, Frame, Link, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Instant, timeout};

/// A peer that reads and never answers gets a `PING` at 10 s and at 20 s
/// of silence; at 30 s the link ends as dead, and its streams fail.
#[tokio::test(start_paused = true)]
async fn a_silent_peer_ends_the_link_after_the_dead_time() {
    let start = Instant::now();
    let (link, mut peer) = RawPeer::start(Role::Dialer, Settings::default()).await;
    let mut stream = link.open().unwrap();
    let mut pings = Vec::new();
    while let Some(frame) = peer.next().await {
        if let Frame::Ping(_) = frame {
            pings.push(start.elapsed());
        }
    }
    assert_eq!(pings, [Duration::from_secs(10), Duration::from_secs(20)]);
    let e = link.closed().await;
    assert_eq!(e, Error::Dead(Duration::from_secs(30)));
    assert_eq!(e.class(), pod_core::Class::Timeout);
    assert_eq!(start.elapsed(), Duration::from_secs(30));
    let e = stream.read(&mut [0; 1]).await.unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
}

/// A peer that holds the carrier and sends nothing: the same end, with
/// no read of the carrier at all.
#[tokio::test(start_paused = true)]
async fn a_carrier_that_is_held_and_silent_is_dead() {
    let settings =
        Settings { keepalive: Duration::from_secs(2), dead_after: Duration::from_secs(5), ..Settings::default() };
    let (near, _far) = pod_link::memory::pair(1 << 16);
    let start = Instant::now();
    let link = Link::new(near, Role::Listener, settings).unwrap();
    assert_eq!(link.closed().await, Error::Dead(Duration::from_secs(5)));
    assert_eq!(start.elapsed(), Duration::from_secs(5));
    assert!(matches!(link.accept().await, Err(Error::Dead(_))));
}

/// Two ends that say nothing for an hour keep their link: each answers
/// the other's `PING`. Then a stream still echoes, and the round trip of
/// the last `PING` is known.
#[tokio::test(start_paused = true)]
async fn a_quiet_link_lives() {
    let (dialer, listener) = common::over_memory(Settings::default(), Settings::default()).await;
    let _server = echo(listener);
    tokio::time::sleep(Duration::from_secs(3600)).await;
    assert!(dialer.ended().is_none());
    assert!(dialer.rtt().is_some());
    let mut s = dialer.open().unwrap();
    s.write_all(b"after an hour").await.unwrap();
    s.shutdown().await.unwrap();
    let mut back = Vec::new();
    timeout(limit(), s.read_to_end(&mut back)).await.unwrap().unwrap();
    assert_eq!(back, b"after an hour");
}

/// A peer that answers each `PING` keeps the link, with no other frame.
#[tokio::test(start_paused = true)]
async fn a_peer_that_answers_keeps_the_link() {
    let (link, mut peer) = RawPeer::start(Role::Dialer, Settings::default()).await;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(300) {
        if let Frame::Ping(v) = peer.until(|f| matches!(f, Frame::Ping(_))).await {
            peer.send(Frame::Pong(v)).await;
        }
    }
    assert!(link.ended().is_none());
    assert_eq!(link.rtt(), Some(Duration::ZERO));
}
