//! The real path of the ICMP carrier over a raw ICMPv4 socket (T-497): a
//! link over the loopback carries a stream. It needs `CAP_NET_RAW` or uid 0;
//! where the host has none, the test prints a skip line and returns (call
//! C276). Unix only; the framing itself runs without a privilege in
//! `tests/icmp.rs`.

#![cfg(all(feature = "icmp", unix))]

mod common;

use std::net::Ipv4Addr;

use common::tcp::{id, ring};
use common::{LIMIT, echo, pattern};
use pod_identity::{Identity, KeyName};
use pod_link::icmp::Icmp;
use pod_link::tcp::Keys;
use pod_link::{Carrier, Link, Listener, Role, Settings};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

fn carrier(n: u8, clients: &[pod_identity::PublicKey]) -> Icmp {
    Icmp::new(ring(), id(n)).with_clients(Keys::new(clients.iter().map(|k| KeyName::Key(*k))))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_a_raw_socket_carries_a_stream_or_skips() {
    pod_testkit::need_loopback!();
    let (server, client): (Arc<Identity>, Arc<Identity>) = (id(1), id(2));
    let listening = carrier(1, &[client.public()]);
    if let Err(e) = listening.available() {
        eprintln!("skip a_link_over_a_raw_socket_carries_a_stream_or_skips: {e}");
        return;
    }
    let mut door = listening.bind(Ipv4Addr::LOCALHOST).await.expect("a raw ICMP listener");
    let dialing = carrier(2, &[]);
    let address = format!("{}@127.0.0.1", server.public().hex());
    let both = async { tokio::join!(dialing.dial(&address), door.accept_peer()) };
    let (near, far) = timeout(LIMIT, both).await.expect("the dial and the accept in time");
    let (near, far) = (near.expect("the dial"), far.expect("the accept"));
    assert_eq!(far.peer, Some(client.public()));
    let dialer = Link::new(near, Role::Dialer, Settings::default()).unwrap();
    let _echo = echo(Link::new(far.io, Role::Listener, Settings::default()).unwrap());
    let mut stream = dialer.open().unwrap();
    let data = pattern(5, 64 << 10);
    let sent = data.clone();
    let (mut reader, mut writer) = tokio::io::split(&mut stream);
    let send = async move {
        writer.write_all(&sent).await.unwrap();
        writer.shutdown().await.unwrap();
    };
    let mut back = Vec::new();
    let (_, read) = tokio::join!(send, reader.read_to_end(&mut back));
    read.unwrap();
    assert!(back == data, "the bytes came back whole");
    dialer.close().await;
}
