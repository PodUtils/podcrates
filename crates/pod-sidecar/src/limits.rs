//! The limits of the sidecar (rule LIM-1). Each limit names the condition
//! that triggers it and the check that detects it. `podvm/docs/limits.md`
//! is the model: a page says what a carrier does not do.
//!
//! A test of this crate fails when a split behaviour has no limit here.

use crate::split::Behaviour;

/// One limit: its condition and the check that detects it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit {
    /// The behaviour that the limit applies to.
    pub behaviour: &'static str,
    /// The condition that triggers the limit.
    pub condition: &'static str,
    /// The check that detects it.
    pub detection: &'static str,
}

/// Each limit of the sidecar.
pub const LIMITS: &[Limit] = &[
    Limit {
        behaviour: "split-at-an-offset",
        condition: "a receiver that reassembles the IP fragments, or a TCP stack that reassembles the byte stream in order",
        detection: "Simulator::split with Policy::strict returns SplitError::Reassembled, and the appended bytes arrive whole",
    },
    Limit {
        behaviour: "disorder",
        condition: "a receiver that reassembles the byte stream and tolerates a gap, as every TCP stack does",
        detection: "Simulator::split with Policy::strict returns SplitError::Reassembled, and the delivered stream is unchanged",
    },
    Limit {
        behaviour: "fake-ttl",
        condition: "a path that honours the TTL, decrements it and drops the segment at zero",
        detection: "Simulator::split with Policy::strict returns SplitError::TimeExceeded",
    },
    Limit {
        behaviour: "tamper-a-checksum",
        condition: "a receiver that requires a valid TCP checksum, or a middlebox that repairs the checksum",
        detection: "Simulator::split with Policy::strict returns SplitError::ChecksumRejected; the endpoint also counts a bad checksum",
    },
    Limit {
        behaviour: "pad-the-first-segment",
        condition: "a receiver that reassembles the stream and compares it with the original",
        detection: "Simulator::split with Policy::strict returns SplitError::Reassembled; the padding is visible in SplitRun::padding",
    },
    Limit {
        behaviour: "fake-tcp",
        condition: "a path that refuses UDP by protocol number, or repairs the checksum of a frame",
        detection: "Simulator::udp_echo returns a refusal, and FakeTcp::recv returns FakeTcpError::BadChecksum when a frame is changed",
    },
    Limit {
        behaviour: "raw-socket-probe",
        condition: "a host with no CAP_NET_RAW and no uid 0",
        detection: "probe::raw_capability returns a Refusal that names the error, and Probe reports Verdict::Unknown",
    },
    Limit {
        behaviour: "silent-probe",
        condition: "a path that drops every unusual segment and answers nothing",
        detection: "the verdict is Verdict::Agrees with an empty response, so silence is not evidence of a disagreement",
    },
    Limit {
        behaviour: "bond-one-flow",
        condition: "a session that is one flow, so the per-flow scheduler puts it on one link",
        detection: "Bond::estimate of the flow is 10 of a sum of 11 on links at 10:1, which is 91 percent of the sum",
    },
    Limit {
        behaviour: "bond-move",
        condition: "a link that fails, which moves each of its flows once",
        detection: "Bond::moves counts each move, and a flow on a live link does not move",
    },
];

/// Look up the limit of a behaviour.
pub fn limit_of(behaviour: &str) -> Option<&'static Limit> {
    LIMITS.iter().find(|limit| limit.behaviour == behaviour)
}

/// The split limits, in the order of [`Behaviour::ALL`].
pub fn split_limits() -> Vec<&'static Limit> {
    Behaviour::ALL.iter().filter_map(|behaviour| limit_of(behaviour.name())).collect()
}
