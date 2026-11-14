//! The DNS carrier through a stand-in resolver on the loopback (T-543): a
//! resolver that sends no EDNS0 toward the server gets answers of 512 bytes
//! or less, with no OPT record; one that sends the size of 1232 gets
//! answers within it; a resolver that cuts each third answer (the TC bit,
//! with no record or with a record cut) does not stall the link.

#![cfg(feature = "dns")]

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::dns::{at, bound, carrier};
use common::tcp::id;
use common::{limit, pattern};
use pod_link::arq::{ArqSettings, MAX_RETRIES};
use pod_link::dns::EDNS_SIZE;
use pod_link::{BoxIo, Carrier};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::time::timeout;

/// What the stand-in resolver does to the queries and the answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Path {
    /// It sends each query on whole.
    Whole,
    /// It removes the OPT record of each query: a resolver with no EDNS0.
    NoEdns,
    /// It cuts each third answer and sets the TC bit: to the header and
    /// the question, or in its second record.
    Cuts,
}

/// The counts of the stand-in resolver.
#[derive(Default)]
struct Seen {
    answers: AtomicUsize,
    largest: AtomicUsize,
    with_opt: AtomicUsize,
    cuts: AtomicUsize,
}

/// The end of the question of `msg`: after its name, its type and its class.
fn question_end(msg: &[u8]) -> usize {
    let mut at = 12;
    while msg[at] != 0 {
        at += 1 + usize::from(msg[at]);
    }
    at + 1 + 4
}

/// The end of the answer record at `at`: an owner pointer, the fixed
/// fields and the data.
fn record_end(msg: &[u8], at: usize) -> usize {
    let len = usize::from(u16::from_be_bytes([msg[at + 10], msg[at + 11]]));
    at + 12 + len
}

/// A resolver at a free UDP port of the loopback that sends each query to
/// `server` and each answer back to the client, along `path`.
async fn resolver(server: SocketAddr, path: Path) -> (SocketAddr, Arc<Seen>) {
    let front = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let back = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = front.local_addr().unwrap();
    let seen = Arc::new(Seen::default());
    let counts = seen.clone();
    tokio::spawn(async move {
        let (mut up, mut down) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
        let mut client = None;
        loop {
            tokio::select! {
                got = front.recv_from(&mut up) => {
                    let Ok((n, from)) = got else { return };
                    client = Some(from);
                    let mut query = up[..n].to_vec();
                    // The OPT record is the last record of the query of the carrier.
                    if path == Path::NoEdns && query[11] == 1 {
                        query.truncate(query.len() - 11);
                        query[11] = 0;
                    }
                    let _ = back.send_to(&query, server).await;
                }
                got = back.recv_from(&mut down) => {
                    let Ok((n, _)) = got else { return };
                    let mut answer = down[..n].to_vec();
                    let count = counts.answers.fetch_add(1, Ordering::Relaxed);
                    counts.largest.fetch_max(n, Ordering::Relaxed);
                    if answer[11] > 0 {
                        counts.with_opt.fetch_add(1, Ordering::Relaxed);
                    }
                    if path == Path::Cuts && count % 3 == 2 {
                        counts.cuts.fetch_add(1, Ordering::Relaxed);
                        answer[2] |= 0x02;
                        let first = question_end(&answer);
                        let records = u16::from_be_bytes([answer[6], answer[7]]);
                        if count % 2 == 0 || records < 2 {
                            answer.truncate(first);
                            answer[6..12].fill(0);
                        } else {
                            answer.truncate(record_end(&answer, first) + 5);
                        }
                    }
                    if let Some(to) = client {
                        let _ = front.send_to(&answer, to).await;
                    }
                }
            }
        }
    });
    (addr, seen)
}

/// A link of the DNS carrier through a resolver along `path`. The RTO of
/// the ARQ is short, so a datagram that a cut answer held goes again soon.
/// The most retries and a ceiling of 2 s give a lost frame about 27 s,
/// within the 60 s of a transfer, so a slow host does not end the link.
async fn link(path: Path) -> (BoxIo, BoxIo, Arc<Seen>) {
    let (server, client) = (id(1), id(2));
    let arq = ArqSettings {
        rto_initial: Duration::from_millis(200),
        rto_floor: Duration::from_millis(50),
        rto_ceiling: Duration::from_secs(2),
        retries: MAX_RETRIES,
        ..ArqSettings::default()
    };
    let (mut listening, mut dialing) = (carrier(1, &[client.public()]), carrier(2, &[]));
    (listening.arq, dialing.arq) = (arq, arq);
    let mut door = bound(&listening).await;
    let (front, seen) = resolver(door.local_addr(), path).await;
    let dialing = dialing.to(front);
    let address = at(&server.public());
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    (near.expect("the dial"), far.expect("the accept").0, seen)
}

/// `size` bytes each way at once, whole and in order.
async fn cross(near: BoxIo, far: BoxIo, size: usize) {
    let (near_read, mut near_write) = tokio::io::split(near);
    let (far_read, mut far_write) = tokio::io::split(far);
    let (up, down) = (pattern(1, size), pattern(2, size));
    let (up2, down2) = (up.clone(), down.clone());
    let send_up = async move {
        near_write.write_all(&up2).await.unwrap();
        near_write.shutdown().await.unwrap();
    };
    let send_down = async move {
        far_write.write_all(&down2).await.unwrap();
        far_write.shutdown().await.unwrap();
    };
    let read = |mut from: tokio::io::ReadHalf<BoxIo>| async move {
        let mut got = Vec::new();
        from.read_to_end(&mut got).await.unwrap();
        got
    };
    let all = async { tokio::join!(send_up, send_down, read(far_read), read(near_read)) };
    let (_, _, got_up, got_down) = timeout(Duration::from_secs(60), all).await.expect("the transfer in time");
    assert!(got_up == up, "up: whole and in order, {} of {}", got_up.len(), up.len());
    assert!(got_down == down, "down: whole and in order, {} of {}", got_down.len(), down.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_query_with_no_edns_gets_an_answer_of_512_bytes_or_less() {
    pod_testkit::need_loopback!();
    let (near, far, seen) = link(Path::NoEdns).await;
    cross(near, far, 32 << 10).await;
    let largest = seen.largest.load(Ordering::Relaxed);
    assert!(largest <= 512, "an answer of {largest} bytes to a query with no EDNS0");
    assert!(largest > 300, "the answers held datagrams: {largest} bytes at most");
    assert_eq!(seen.with_opt.load(Ordering::Relaxed), 0, "an answer with an OPT record to a query with none");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_query_with_edns_gets_an_answer_within_its_size() {
    pod_testkit::need_loopback!();
    let (near, far, seen) = link(Path::Whole).await;
    cross(near, far, 32 << 10).await;
    let largest = seen.largest.load(Ordering::Relaxed);
    assert!(largest <= usize::from(EDNS_SIZE), "an answer of {largest} bytes");
    assert!(largest > 512, "the answers use the size of the query: {largest} bytes at most");
    assert_eq!(seen.with_opt.load(Ordering::Relaxed), seen.answers.load(Ordering::Relaxed));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cut_answer_does_not_stall_the_link() {
    pod_testkit::need_loopback!();
    let (near, far, seen) = link(Path::Cuts).await;
    cross(near, far, 32 << 10).await;
    assert!(seen.cuts.load(Ordering::Relaxed) > 10, "the resolver cut answers");
}
