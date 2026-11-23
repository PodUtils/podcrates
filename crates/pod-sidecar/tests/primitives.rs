//! Each primitive produces the divergence that it exists for, and a
//! hand-written strategy is reproduced byte for byte with a fixed seed.

use pod_sidecar::simulator::BytesDiff;
use pod_sidecar::{Date, Divergence, Primitive, Run, Scenario, Side, Simulator, Step, Strategy, Trigger};

fn date() -> Date {
    Date::parse("2026-11-12").unwrap()
}

fn strategy(steps: Vec<Step>) -> Strategy {
    Strategy::new(date(), "lab-a", "hand", steps)
}

fn run(primitive: Primitive) -> Run {
    Simulator::new(42).run(&strategy(vec![Step::new(Trigger::FirstOutboundData, primitive)]))
}

fn server_bytes(divergence: &Divergence) -> &BytesDiff {
    divergence.bytes.iter().find(|diff| diff.side == Side::Server).expect("a server byte difference")
}

#[test]
fn drop_removes_bytes_and_segments() {
    let run = run(Primitive::Drop);
    let bytes = server_bytes(&run.divergence);
    assert_ne!(bytes.expected, bytes.got);
    assert!(bytes.got.len() < bytes.expected.len(), "{}", run.divergence);
    assert!(
        run.divergence.segments.iter().any(|diff| diff.expected > diff.got),
        "a dropped segment leaves the wire: {}",
        run.divergence
    );
}

#[test]
fn tamper_changes_one_byte_and_the_checksum_still_matches() {
    let run = run(Primitive::Tamper);
    let bytes = server_bytes(&run.divergence);
    assert_eq!(bytes.expected.len(), bytes.got.len());
    assert_ne!(bytes.expected, bytes.got);
    let changed: usize = bytes.expected.iter().zip(&bytes.got).filter(|(a, b)| a != b).count();
    assert_eq!(changed, 1, "one payload byte changes: {}", run.divergence);
    for frame in &run.frames {
        assert!(frame.segment.checksum_valid(), "the middlebox repairs the checksum");
    }
}

#[test]
fn duplicate_sends_the_segment_twice_and_keeps_the_stream() {
    let run = run(Primitive::Duplicate);
    assert!(run.divergence.bytes.is_empty(), "a duplicate carries no new byte: {}", run.divergence);
    let out = run.divergence.segments.iter().find(|diff| diff.direction.as_str() == "out").expect("out");
    assert_eq!(out.got, out.expected + 1);
}

#[test]
fn fragment_splits_the_segment_and_the_stream_arrives_whole() {
    let run = run(Primitive::Fragment);
    assert!(run.divergence.bytes.is_empty(), "TCP reassembles the fragments: {}", run.divergence);
    let out = run.divergence.segments.iter().find(|diff| diff.direction.as_str() == "out").expect("out");
    assert_eq!(out.got, out.expected + 1);
}

#[test]
fn coalesce_merges_the_batch_and_the_stream_arrives_whole() {
    let run = run(Primitive::Coalesce);
    assert!(run.divergence.bytes.is_empty(), "the stream is unchanged: {}", run.divergence);
    let out = run.divergence.segments.iter().find(|diff| diff.direction.as_str() == "out").expect("out");
    assert_eq!(out.got, out.expected - 1);
}

#[test]
fn the_syn_ack_trigger_reaches_the_handshake() {
    let run = Simulator::new(42).run(&strategy(vec![Step::new(Trigger::SynAck, Primitive::Drop)]));
    let client = run.divergence.states.iter().find(|diff| diff.side == Side::Client).expect("a client state");
    assert_eq!(client.expected.as_str(), "TIME-WAIT");
    assert_eq!(client.got.as_str(), "SYN-SENT");
}

#[test]
fn the_first_inbound_rst_trigger_reaches_a_refused_connection() {
    let steps = vec![Step::new(Trigger::FirstInboundRst, Primitive::Drop)];
    let run = Simulator::new(42).run_with(&strategy(steps), Scenario::Refused);
    let client = run.divergence.states.iter().find(|diff| diff.side == Side::Client).expect("a client state");
    assert_eq!(client.expected.as_str(), "CLOSED");
    assert_eq!(client.got.as_str(), "SYN-SENT");
}

#[test]
fn always_reaches_each_outbound_segment() {
    let run = Simulator::new(42).run(&strategy(vec![Step::new(Trigger::Always, Primitive::Drop)]));
    let client = run.divergence.states.iter().find(|diff| diff.side == Side::Client).expect("a client state");
    assert_eq!(client.got.as_str(), "SYN-SENT");
    assert_eq!(run.server.stream(), b"");
}

#[test]
fn a_hand_written_strategy_is_reproduced_byte_for_byte_with_a_fixed_seed() {
    let hand = strategy(vec![Step::new(Trigger::FirstOutboundData, Primitive::Tamper).with_offset(3).with_byte(0x5a)]);
    let file = r#"
date = "2026-11-12"
network = "lab-a"
found_by = "hand"
[[steps]]
trigger = "FirstOutboundData"
primitive = "tamper"
offset = 3
byte = 90
"#;
    let loaded = Strategy::from_toml(file).unwrap();
    assert_eq!(hand, loaded);
    let simulator = Simulator::new(7);
    let first = simulator.run(&hand);
    let second = simulator.run(&loaded);
    assert_eq!(first.wire(), second.wire());
    assert_eq!(first.divergence, second.divergence);
    assert!(!first.wire().is_empty());
    // The same seed on another value gives the same run; another seed gives
    // another initial sequence number and so other bytes.
    let again = Simulator::new(7).run(&hand);
    assert_eq!(first.wire(), again.wire());
    let other = Simulator::new(8).run(&hand);
    assert_ne!(first.wire(), other.wire());
}

#[test]
fn a_strategy_with_no_step_changes_nothing() {
    let empty = Strategy::new(date(), "lab-a", "hand", Vec::new());
    let run = Simulator::new(42).run(&empty);
    assert!(run.divergence.is_empty(), "{}", run.divergence);
    assert_eq!(run.server.stream(), b"hello, sidecar!again");
    assert_eq!(run.client.stream(), b"hello, sidecar!again");
}
