//! The DNS carrier over DNS over HTTPS (T-1168, PodUtils/podssh#349): a
//! dialer behind a proxy of HTTP sends each query as a GET of the wire form
//! to a stand-in resolver on the loopback, over one connection through the
//! proxy, and the listener of the carrier answers behind the stand-in; over
//! HTTP/2 and over HTTP/1.1. A `429` of the resolver pauses the queries for
//! its `Retry-After`, at most `DnsOverHttps::pause_max`.

#![cfg(feature = "dns")]

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::dns::{at, bound, carrier};
use common::doh::{over, stand_in};
use common::proxy::Tunnel;
use common::tcp::id;
use common::{echo, limit, pattern};
use pod_link::{Carrier, CarrierErrorKind, Link, Listener, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

/// A link over a stand-in of HTTP/2 or of HTTP/1.1 carries a stream both
/// ways; the dialer opens one connection, through the proxy.
async fn a_link_over(http2: bool) {
    let client = id(2);
    let mut listening = carrier(1, &[client.public()]);
    let mut dialing = carrier(2, &[]);
    if !http2 {
        // Over HTTP/1.1 a held poll delays each query after it.
        (listening.dns.hold, dialing.dns.hold) = (Duration::ZERO, Duration::ZERO);
    }
    let mut door = bound(&listening).await;
    let (port, seen) = stand_in(door.local_addr()).await;
    let proxy = Tunnel::start(([127, 0, 0, 1], port).into()).await;
    let dialing = dialing.with_way(proxy.way()).over_https(over(port, http2));
    let address = at(&id(1).public());
    let started = std::time::Instant::now();
    let both = async { tokio::join!(dialing.dial(&address), door.accept_peer()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    let (near, far) = (near.expect("the dial over HTTPS"), far.expect("the accept"));
    assert_eq!(far.peer, Some(client.public()));

    let dialer = Link::new(near, Role::Dialer, Settings::default()).unwrap();
    let _echo = echo(Link::new(far.io, Role::Listener, Settings::default()).unwrap());
    let mut stream = dialer.open().unwrap();
    let data = pattern(3, 8 << 10);
    let (mut reader, mut writer) = tokio::io::split(&mut stream);
    let send = async {
        writer.write_all(&data).await.unwrap();
        writer.shutdown().await.unwrap();
    };
    let mut back = Vec::new();
    let (_, read) = timeout(limit(), async { tokio::join!(send, reader.read_to_end(&mut back)) }).await.unwrap();
    read.unwrap();
    assert!(back == data, "the bytes came back whole");
    dialer.close().await;
    let requests = seen.requests.lock().unwrap().len();
    let form = if http2 { "HTTP/2" } else { "HTTP/1.1" };
    eprintln!("{form}: the dial and 8 KiB each way in {:?}, {requests} requests", started.elapsed());
    assert!(requests > 20, "{requests} requests");
    assert_eq!(proxy.tunnels.load(Ordering::SeqCst), 1, "one tunnel through the proxy for {requests} requests");
    assert_eq!(seen.connections.load(Ordering::SeqCst), 1, "one connection for {requests} requests");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_dns_over_https_through_a_proxy_carries_streams() {
    pod_testkit::need_loopback!();
    a_link_over(true).await;
    a_link_over(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_429_pauses_the_queries_for_its_retry_after() {
    pod_testkit::need_loopback!();
    let client = id(2);
    let mut door = bound(&carrier(1, &[client.public()])).await;
    let (port, seen) = stand_in(door.local_addr()).await;
    let proxy = Tunnel::start(([127, 0, 0, 1], port).into()).await;
    let mut doh = over(port, true);
    doh.pause_max = Duration::from_millis(1500);
    let mut dialing = carrier(2, &[]).with_way(proxy.way()).over_https(doh);
    // One query at a time, so that the query after a 429 is the first after
    // the pause.
    dialing.dns.in_flight = 1;
    // A Retry-After of 1 s, one past the bound, and none.
    let waits = [("1", 1000), ("3600", 1500), ("", 1000)];
    seen.refuse.lock().unwrap().extend(waits.iter().map(|(said, _)| (!said.is_empty()).then(|| said.to_string())));
    let address = at(&id(1).public());
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    near.expect("the dial after three 429");
    far.expect("the accept");

    let requests = seen.requests.lock().unwrap().clone();
    let mut gaps = Vec::new();
    for (i, pair) in requests.windows(2).take(waits.len()).enumerate() {
        assert!(pair[0].1, "request {i} got a 429");
        gaps.push(pair[1].0.duration_since(pair[0].0));
    }
    eprintln!("the gaps after the three 429: {gaps:?}");
    for ((said, wait), gap) in waits.iter().zip(&gaps) {
        let wait = Duration::from_millis(*wait);
        assert!(*gap + Duration::from_millis(50) >= wait, "Retry-After {said:?}: the next query after {gap:?}");
        assert!(*gap < wait + Duration::from_secs(1), "Retry-After {said:?}: the pause is bounded, {gap:?}");
    }
}

#[tokio::test]
async fn dns_over_https_needs_a_way_out() {
    let dialing = carrier(2, &[]).over_https(over(443, true));
    let e = dialing.dial(&at(&id(1).public())).await.err().expect("a dial over HTTPS with no way out");
    assert_eq!(e.kind, CarrierErrorKind::Usage, "{e}");
    assert!(format!("{dialing:?}").contains("DnsOverHttps"));
}
