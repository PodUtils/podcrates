//! A local middlebox simulator. It applies faults to a capture with no raw
//! socket and no network namespace, and it records each decision.
//!
//! The fault model is this crate's own: `pod-sidecar` is not in this
//! worktree. The faults change bytes, sizes, times and loss, which are the
//! parts that a classifier reads.

use crate::capture::{Capture, Direction, Packet};
use crate::error::{Error, Result};

/// One fault of the simulator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Add a TLS application data record of zero bytes to each TLS packet,
    /// so that each packet is at least `to` bytes.
    Pad { to: usize },
    /// Split each packet into chunks of `size` bytes.
    Chunk { size: usize },
    /// Add `micros` to each timestamp.
    Delay { micros: u64 },
    /// Drop each `every`th packet from the client to the server.
    DropEvery { every: usize },
}

/// One decision of the simulator: what it did to which packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// The index of the packet in the input capture.
    pub packet: usize,
    /// The name of the fault.
    pub fault: &'static str,
    /// What changed.
    pub detail: String,
}

/// The simulator: a list of faults and the decisions of its last run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Middlebox {
    faults: Vec<Fault>,
    decisions: Vec<Decision>,
}

impl Fault {
    pub fn name(&self) -> &'static str {
        match self {
            Fault::Pad { .. } => "pad",
            Fault::Chunk { .. } => "chunk",
            Fault::Delay { .. } => "delay",
            Fault::DropEvery { .. } => "drop",
        }
    }

    /// A fault of the `faults` array of a case file:
    /// `{"kind":"pad","to":1200}`.
    pub fn from_json(value: &serde_json::Value) -> Result<Fault> {
        let kind =
            value.get("kind").and_then(serde_json::Value::as_str).ok_or_else(|| Error::new("a fault has no kind"))?;
        let number = |key: &str| {
            value
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| Error::new(format!("the fault {kind} has no {key}")))
        };
        let fault = match kind {
            "pad" => Fault::Pad { to: number("to")? as usize },
            "chunk" => Fault::Chunk { size: number("size")? as usize },
            "delay" => Fault::Delay { micros: number("micros")? },
            "drop" => Fault::DropEvery { every: number("every")? as usize },
            other => return Err(Error::new(format!("the fault {other} is unknown"))),
        };
        fault.check()?;
        Ok(fault)
    }

    fn check(&self) -> Result<()> {
        match self {
            Fault::Pad { to } if *to < 6 => Err(Error::new("a pad below 6 bytes cannot hold a record header")),
            Fault::Chunk { size: 0 } => Err(Error::new("a chunk of 0 bytes is not a packet")),
            Fault::DropEvery { every: 0 } => Err(Error::new("a drop of every 0 packets is not a rule")),
            _ => Ok(()),
        }
    }
}

impl Middlebox {
    pub fn new(faults: Vec<Fault>) -> Middlebox {
        Middlebox { faults, decisions: Vec::new() }
    }

    pub fn faults(&self) -> &[Fault] {
        &self.faults
    }

    /// The decisions of the last [`Middlebox::apply`].
    pub fn decisions(&self) -> &[Decision] {
        &self.decisions
    }

    /// Apply each fault to the capture, in the order that the faults came.
    /// The decisions of this run replace the decisions of the last run.
    pub fn apply(&mut self, capture: &Capture) -> Capture {
        self.decisions.clear();
        let mut packets: Vec<(usize, Packet)> = capture.packets.iter().cloned().enumerate().collect();
        for fault in &self.faults {
            let mut next = Vec::new();
            for (index, packet) in packets {
                match fault {
                    Fault::Pad { to } => {
                        let before = packet.bytes.len();
                        let padded = pad(packet, *to);
                        if padded.bytes.len() != before {
                            self.decisions.push(Decision {
                                packet: index,
                                fault: fault.name(),
                                detail: format!("padded {before} bytes to {}", padded.bytes.len()),
                            });
                        }
                        next.push((index, padded));
                    }
                    Fault::Chunk { size } => {
                        let chunks = chunk(packet, *size);
                        if chunks.len() > 1 {
                            self.decisions.push(Decision {
                                packet: index,
                                fault: fault.name(),
                                detail: format!("split into {} chunks of {size}", chunks.len()),
                            });
                        }
                        next.extend(chunks.into_iter().map(|p| (index, p)));
                    }
                    Fault::Delay { micros } => {
                        let mut delayed = packet;
                        delayed.timestamp_micros = delayed.timestamp_micros.saturating_add(*micros);
                        self.decisions.push(Decision {
                            packet: index,
                            fault: fault.name(),
                            detail: format!("delayed by {micros} microseconds"),
                        });
                        next.push((index, delayed));
                    }
                    Fault::DropEvery { every } => {
                        if packet.direction == Direction::ClientToServer {
                            // Count the client's packets by their index in
                            // the input, not by their place after a chunk.
                            let ordinal = capture.packets[..index]
                                .iter()
                                .filter(|p| p.direction == Direction::ClientToServer)
                                .count()
                                + 1;
                            if ordinal % *every == 0 {
                                self.decisions.push(Decision {
                                    packet: index,
                                    fault: fault.name(),
                                    detail: format!("dropped client packet {ordinal}"),
                                });
                                continue;
                            }
                        }
                        next.push((index, packet));
                    }
                }
            }
            packets = next;
        }
        let mut out = Capture::new();
        for (_, packet) in packets {
            out.push(packet);
        }
        out
    }
}

fn pad(mut packet: Packet, to: usize) -> Packet {
    let is_tls = matches!(packet.bytes.first(), Some(0x14..=0x17));
    if !is_tls || packet.bytes.len() >= to {
        return packet;
    }
    let payload = (to - 5).min(u16::MAX as usize);
    packet.bytes.extend_from_slice(&[0x17, 0x03, 0x03]);
    packet.bytes.extend_from_slice(&(payload as u16).to_be_bytes());
    packet.bytes.resize(packet.bytes.len() + payload, 0);
    packet
}

fn chunk(packet: Packet, size: usize) -> Vec<Packet> {
    if size == 0 || packet.bytes.len() <= size {
        return vec![packet];
    }
    let mut out = Vec::new();
    for (n, part) in packet.bytes.chunks(size).enumerate() {
        out.push(Packet {
            direction: packet.direction,
            timestamp_micros: packet.timestamp_micros.saturating_add(n as u64),
            bytes: part.to_vec(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture() -> Capture {
        Capture::parse(include_str!("../fixtures/captures/tunnel.cap")).unwrap()
    }

    #[test]
    fn a_pad_adds_a_record_and_records_the_packet() {
        let input = capture();
        let mut box_ = Middlebox::new(vec![Fault::Pad { to: 1200 }]);
        let output = box_.apply(&input);
        assert_eq!(output.packets.len(), input.packets.len());
        for (before, after) in input.packets.iter().zip(&output.packets) {
            assert!(after.bytes.len() >= 1200, "{}", after.bytes.len());
            assert_eq!(after.timestamp_micros, before.timestamp_micros);
        }
        assert_eq!(box_.decisions().len(), input.packets.len());
        assert_eq!(box_.decisions()[0].fault, "pad");
        assert!(Capture::parse(&output.to_text()).unwrap() == output);
    }

    #[test]
    fn a_chunk_splits_a_packet_and_keeps_each_byte() {
        let input = capture();
        let mut box_ = Middlebox::new(vec![Fault::Chunk { size: 16 }]);
        let output = box_.apply(&input);
        assert!(output.packets.len() > input.packets.len());
        let joined: Vec<u8> = output.packets.iter().flat_map(|p| p.bytes.clone()).collect();
        let original: Vec<u8> = input.packets.iter().flat_map(|p| p.bytes.clone()).collect();
        assert_eq!(joined, original);
    }

    #[test]
    fn a_delay_moves_each_timestamp_and_a_drop_removes_a_client_packet() {
        let input = capture();
        let mut delay = Middlebox::new(vec![Fault::Delay { micros: 25 }]);
        let moved = delay.apply(&input);
        for (before, after) in input.packets.iter().zip(&moved.packets) {
            assert_eq!(after.timestamp_micros, before.timestamp_micros + 25);
        }
        let mut drop = Middlebox::new(vec![Fault::DropEvery { every: 2 }]);
        let dropped = drop.apply(&input);
        let client = input.packets.iter().filter(|p| p.direction == Direction::ClientToServer).count();
        let after = dropped.packets.iter().filter(|p| p.direction == Direction::ClientToServer).count();
        assert_eq!(after, client - client / 2);
        assert!(!drop.decisions().is_empty());
    }

    #[test]
    fn a_bad_fault_is_refused() {
        assert!(Fault::from_json(&serde_json::json!({"kind":"chunk","size":0})).is_err());
        assert!(Fault::from_json(&serde_json::json!({"kind":"drop","every":0})).is_err());
        assert!(Fault::from_json(&serde_json::json!({"kind":"nope"})).is_err());
        assert!(Fault::from_json(&serde_json::json!({"kind":"pad","to":2})).is_err());
    }
}
