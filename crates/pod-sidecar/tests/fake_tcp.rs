//! Fake TCP: message framing over a datagram channel, and a UDP echo through
//! it against a path that refuses a plain datagram.

use pod_sidecar::fake_tcp::{DATA, FakeTcp, FakeTcpError, Frame, pair};
use pod_sidecar::{Policy, Simulator};

fn drain<C: pod_sidecar::fake_tcp::Datagram>(link: &mut FakeTcp<C>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(message) = link.recv().unwrap() {
        out.push(message);
    }
    out
}

#[test]
fn a_message_crosses_a_datagram_pair_in_one_frame() {
    let (a, b) = pair();
    let mut client = FakeTcp::new(a);
    let mut server = FakeTcp::new(b);
    client.send(b"ping").unwrap();
    assert_eq!(server.recv().unwrap(), Some(b"ping".to_vec()));
    server.send(b"pong").unwrap();
    assert_eq!(client.recv().unwrap(), Some(b"pong".to_vec()));
    assert_eq!(client.recv().unwrap(), None);
}

#[test]
fn a_message_larger_than_the_frame_is_fragmented_and_reassembled() {
    let (a, b) = pair();
    let mut client = FakeTcp::new(a).with_mtu(64);
    let mut server = FakeTcp::new(b).with_mtu(64);
    let message: Vec<u8> = (0..200u32).map(|n| (n % 251) as u8).collect();
    client.send(&message).unwrap();
    assert_eq!(server.recv().unwrap(), Some(message));
}

#[test]
fn a_frame_out_of_order_waits_in_the_window() {
    let (_a, b) = pair();
    let receiver = FakeTcp::new(b);
    let first = Frame { message: 9, index: 0, count: 2, total: 8, flags: DATA, payload: b"four".to_vec() };
    let second = Frame { message: 9, index: 1, count: 2, total: 8, flags: DATA, payload: b"more".to_vec() };
    receiver.channel().inject(second.encode());
    receiver.channel().inject(first.encode());
    let mut receiver = receiver;
    assert_eq!(receiver.recv().unwrap(), Some(b"fourmore".to_vec()));
}

#[test]
fn a_frame_with_a_bad_checksum_is_named() {
    let (_a, b) = pair();
    let mut receiver = FakeTcp::new(b);
    let frame = Frame { message: 1, index: 0, count: 1, total: 4, flags: DATA, payload: b"data".to_vec() };
    let mut bytes = frame.encode();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    receiver.channel().inject(bytes);
    assert_eq!(receiver.recv(), Err(FakeTcpError::BadChecksum));
}

#[test]
fn a_frame_beyond_the_window_is_named() {
    let (_a, b) = pair();
    let receiver = FakeTcp::new(b).with_window(1);
    for message in 1..=2u32 {
        let frame = Frame { message, index: 0, count: 2, total: 8, flags: DATA, payload: b"wait".to_vec() };
        receiver.channel().inject(frame.encode());
    }
    let mut receiver = receiver;
    assert_eq!(receiver.recv(), Err(FakeTcpError::OutOfWindow { window: 1 }));
}

#[test]
fn a_plain_datagram_is_refused_and_the_framed_echo_passes() {
    let simulator = Simulator::new(3).with_policy(Policy::refusing_udp());
    let refused = simulator.udp_echo(b"ping").unwrap_err();
    assert!(refused.reason().contains("refuses a plain udp datagram"), "{refused}");
    let (a, b) = simulator.fake_tcp_pair();
    let mut client = FakeTcp::new(a);
    let mut server = FakeTcp::new(b);
    client.send(b"ping").unwrap();
    let got = server.recv().unwrap().unwrap();
    assert_eq!(got, b"ping");
    server.send(&got).unwrap();
    assert_eq!(drain(&mut client), vec![b"ping".to_vec()]);
}

#[test]
fn a_frame_without_the_magic_is_named() {
    let (_a, b) = pair();
    let mut receiver = FakeTcp::new(b);
    receiver.channel().inject(b"plain udp".to_vec());
    assert!(matches!(receiver.recv(), Err(FakeTcpError::BadFrame(_))));
}

#[test]
fn a_host_without_a_raw_socket_reports_a_named_skip() {
    if let Err(refusal) = pod_sidecar::probe::raw_capability() {
        assert!(refusal.reason().contains("raw socket"), "{}", refusal.reason());
        eprintln!("skipping, {}", refusal.reason());
    }
}
