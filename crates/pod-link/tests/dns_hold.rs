//! The held poll of the DNS carrier (T-1166, PodUtils/podssh#349): the
//! listener holds a poll of a session with no datagram down for up to
//! `DnsSettings::hold`, and the dialer starts its wait for the next poll at
//! the answer of the last poll. An idle link sends half the queries, and
//! data down comes sooner; a dialer that polls faster than the hold stacks
//! no queries.

#![cfg(feature = "dns")]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::dns::{at, bound, carrier, fast_dns, query_name, relay, session_of};
use common::limit;
use common::tcp::id;
use pod_dns::txt::{self, TXT};
use pod_link::dns::{DnsListener, DnsSettings, EDNS_SIZE};
use pod_link::{BoxIo, Carrier};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Instant, sleep, timeout};

/// The DNS settings of the tests: a short poll interval and the hold
/// `hold`.
fn settings(hold: Duration) -> DnsSettings {
    let mut dns = fast_dns();
    dns.poll_max = pod_testkit::host::scaled(Duration::from_millis(100));
    dns.hold = hold;
    dns
}

/// A link of the DNS carrier with `dns` at both ends, through a relay that
/// keeps the queries: the end of the dialer, the end of the listener, the
/// queries, and the listener.
async fn link(dns: DnsSettings) -> (BoxIo, BoxIo, Arc<Mutex<Vec<Vec<u8>>>>, DnsListener) {
    let (server, client) = (id(1), id(2));
    let (mut listening, mut dialing) = (carrier(1, &[client.public()]), carrier(2, &[]));
    (listening.dns, dialing.dns) = (dns, dns);
    let mut door = bound(&listening).await;
    let (front, kept) = relay(door.local_addr()).await;
    let dialing = dialing.to(front);
    let address = at(&server.public());
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    (near.expect("the dial"), far.expect("the accept").0, kept, door)
}

/// The queries of one idle minute (counted over `window`) and the median
/// and the mean latency of `samples` bytes down on an idle link with `dns`.
async fn measure(dns: DnsSettings, window: Duration, samples: u32) -> (u64, Duration, Duration) {
    let (mut near, mut far, kept, _door) = link(dns).await;
    // The poll interval grows to `poll_max` after the handshake.
    sleep(dns.poll_max * 10).await;
    let before = kept.lock().unwrap().len();
    sleep(window).await;
    let idle = (kept.lock().unwrap().len() - before) as u64 * 60_000 / window.as_millis() as u64;
    // Each byte down at another point of the poll cycle: the gaps spread
    // over one cycle of a poll and a hold of four intervals.
    let mut latencies = Vec::new();
    for i in 0..samples {
        sleep(dns.poll_max * 7 + dns.poll_max * ((i * 53) % 50) / 10).await;
        let sent = Instant::now();
        far.write_all(&[i as u8]).await.unwrap();
        far.flush().await.unwrap();
        let mut byte = [0u8];
        timeout(limit(), near.read_exact(&mut byte)).await.expect("the byte down in time").unwrap();
        assert_eq!(byte[0], i as u8);
        latencies.push(sent.elapsed());
    }
    latencies.sort();
    if samples == 0 {
        return (idle, Duration::ZERO, Duration::ZERO);
    }
    let mean = latencies.iter().sum::<Duration>() / samples;
    (idle, latencies[latencies.len() / 2], mean)
}

/// The held poll halves the idle queries and cuts the latency of data down.
/// A hold as long as the poll interval makes one idle cycle a hold and a
/// wait: half the queries. A hold of four poll intervals holds a poll for
/// four fifths of the cycle, and a byte down in the hold goes at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_held_poll_halves_the_idle_queries_and_cuts_the_latency_down() {
    pod_testkit::need_loopback!();
    let interval = settings(Duration::ZERO).poll_max;
    let window = pod_testkit::host::scaled(Duration::from_secs(3));
    let (idle_before, latency_before, mean_before) = measure(settings(Duration::ZERO), window, 12).await;
    let (idle_after, _, _) = measure(settings(interval), window, 0).await;
    let (idle_long, latency_after, mean_after) = measure(settings(interval * 4), window, 12).await;
    eprintln!(
        "poll_max {interval:?}, queries in one idle minute: {idle_before} with no hold, {idle_after} with a hold \
         of {interval:?}, {idle_long} with a hold of {:?}; a byte down: {latency_before:?} (median), \
         {mean_before:?} (mean) with no hold, {latency_after:?} (median), {mean_after:?} (mean) with a hold of {:?}",
        interval * 4,
        interval * 4
    );
    assert!(idle_after * 10 < idle_before * 6, "the idle queries: {idle_before} with no hold, {idle_after} held");
    assert!(latency_after * 2 < latency_before, "the latency down: {latency_before:?}, {latency_after:?} held");
}

/// A dialer that polls faster than the hold stacks at most `MAX_HELD` (2)
/// queries of its session: each further poll answers the oldest at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn polls_faster_than_the_hold_stack_no_queries() {
    pod_testkit::need_loopback!();
    let hold = pod_testkit::host::scaled(Duration::from_millis(1500));
    let (_near, _far, kept, door) = link(settings(hold)).await;
    let session = session_of(&kept.lock().unwrap()[0]);
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let sent = Instant::now();
    for n in 0..6u16 {
        let query = txt::query(0x7000 + n, &query_name(session, 0xfff0_0000 + u32::from(n), &[]), TXT, Some(EDNS_SIZE));
        socket.send_to(&query.unwrap(), door.local_addr()).await.unwrap();
    }
    let (mut fast, mut late, mut buf) = (0, 0, vec![0u8; 1 << 16]);
    for _ in 0..6 {
        let (n, _) = timeout(hold * 3, socket.recv_from(&mut buf)).await.expect("each poll gets its answer").unwrap();
        let id = u16::from_be_bytes([buf[0], buf[1]]);
        let answer = txt::read(&buf[..n], id, &query_name(session, 0xfff0_0000 + u32::from(id - 0x7000), &[]), TXT);
        assert_eq!(answer.ok().map(|a| a.rcode), Some(0), "the answer {id:#x}");
        if sent.elapsed() < hold / 2 { fast += 1 } else { late += 1 }
    }
    eprintln!("6 polls of one session at once, a hold of {hold:?}: {fast} answered at once, {late} at the hold");
    assert!(late <= 2 && fast >= 4, "the session held {late} polls");
}

#[test]
fn the_session_of_a_query_comes_from_its_name() {
    for session in [0, 7, 0x1234_5678, u32::MAX] {
        let query = txt::query(1, &query_name(session, 9, &[1, 2, 3]), TXT, None).unwrap();
        assert_eq!(session_of(&query), session);
    }
}
