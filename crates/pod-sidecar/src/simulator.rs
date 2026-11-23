//! A deterministic local TCP-like middlebox simulator. It opens no socket
//! and needs no privilege: the same seed and the same strategy give the same
//! bytes, the same states and the same divergence.
//!
//! A run is a three-way handshake, a data phase with an echo, and a close.
//! Two ends hold the state of RFC 793, a window, a reassembly queue and a
//! checksum check. The middlebox between them applies a strategy's steps,
//! then the policy of the path, and the run reports the difference from a
//! run with no strategy: the exact state, the exact bytes, or the segment
//! count.

use std::collections::VecDeque;
use std::fmt;

use crate::endpoint::Endpoint;
use crate::middlebox::{Direction, Fault, Medium};
use crate::packet::{Segment, WINDOW};
use crate::primitive::{Step, Strategy};
use crate::probe::{Observation, ProbeKind};
use crate::state::{Side, TcpState};

/// The model of the path between the two ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// The path reassembles a split or a disorder, so the receiving end sees
    /// the original byte stream (rule SPLIT-2).
    pub reassembles: bool,
    /// The path requires a valid checksum and drops a segment without one.
    pub valid_checksum: bool,
    /// The path honours a low TTL and drops the segment at zero.
    pub honour_ttl: bool,
    /// The path refuses a plain datagram that carries no fake-TCP frame.
    pub refuse_plain_udp: bool,
    /// The path rejects a FIN while outbound data is unacknowledged.
    pub fin_requires_ack: bool,
    /// The path resets a connection that sends an out-of-window segment.
    pub rst_on_out_of_window: bool,
}

impl Policy {
    /// A path that defeats split, disorder and a tampered checksum.
    pub fn strict() -> Policy {
        Policy { reassembles: true, valid_checksum: true, honour_ttl: true, ..Policy::default() }
    }

    /// A path that refuses a plain datagram.
    pub fn refusing_udp() -> Policy {
        Policy { refuse_plain_udp: true, ..Policy::default() }
    }

    /// A path with a known disagreement: it resets an out-of-window segment,
    /// rejects a FIN before an ACK, and rewrites a retransmission.
    pub fn disagreeing() -> Policy {
        Policy { rst_on_out_of_window: true, fin_requires_ack: true, ..Policy::default() }
    }
}

/// The conversation of a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// A handshake, two sends with an echo, and a close.
    Echo,
    /// The server refuses the connection with a RST.
    Refused,
}

/// One segment on the wire, with its direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub direction: Direction,
    pub segment: Segment,
}

impl Frame {
    pub fn bytes(&self) -> Vec<u8> {
        self.segment.to_bytes()
    }
}

/// The difference between a run with no strategy and a run with one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Divergence {
    pub states: Vec<StateDiff>,
    pub bytes: Vec<BytesDiff>,
    pub segments: Vec<SegmentsDiff>,
}

/// One end finished in another state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateDiff {
    pub side: Side,
    pub expected: TcpState,
    pub got: TcpState,
}

/// The byte stream of one end differs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BytesDiff {
    pub side: Side,
    /// The first index that differs, or the shorter length.
    pub offset: usize,
    pub expected: Vec<u8>,
    pub got: Vec<u8>,
}

/// One direction carried another number of segments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentsDiff {
    pub direction: Direction,
    pub expected: usize,
    pub got: usize,
}

impl Divergence {
    /// None when the strategy changed nothing.
    pub fn is_empty(&self) -> bool {
        self.states.is_empty() && self.bytes.is_empty() && self.segments.is_empty()
    }

    pub fn kinds(&self) -> usize {
        usize::from(!self.states.is_empty())
            + usize::from(!self.bytes.is_empty())
            + usize::from(!self.segments.is_empty())
    }

    /// A value for the search: a state counts more than a byte, and a byte
    /// more than a segment count.
    pub fn magnitude(&self) -> i64 {
        let states = self.states.len() as i64 * 1000;
        let bytes: i64 = self
            .bytes
            .iter()
            .map(|b| first_difference(&b.expected, &b.got) as i64 + missing(&b.expected, &b.got) as i64)
            .sum();
        let segments = self.segments.len() as i64 * 10;
        states + bytes * 100 + segments
    }

    /// One line, for a report or a test message.
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return "no divergence".to_string();
        }
        let mut parts = Vec::new();
        for state in &self.states {
            parts.push(format!("{} state {} -> {}", state.side, state.expected, state.got));
        }
        for bytes in &self.bytes {
            parts.push(format!(
                "{} bytes {:?} -> {:?} at {}",
                bytes.side,
                String::from_utf8_lossy(&bytes.expected),
                String::from_utf8_lossy(&bytes.got),
                bytes.offset
            ));
        }
        for segments in &self.segments {
            parts.push(format!("{} segments {} -> {}", segments.direction.as_str(), segments.expected, segments.got));
        }
        parts.join("; ")
    }

    fn between(baseline: &Run, observed: &Run) -> Divergence {
        let mut out = Divergence::default();
        for (side, base, got) in
            [(Side::Client, &baseline.client, &observed.client), (Side::Server, &baseline.server, &observed.server)]
        {
            if base.state != got.state {
                out.states.push(StateDiff { side, expected: base.state, got: got.state });
            }
            if base.stream() != got.stream() {
                let offset = first_difference(base.stream(), got.stream());
                out.bytes.push(BytesDiff {
                    side,
                    offset,
                    expected: base.stream().to_vec(),
                    got: got.stream().to_vec(),
                });
            }
        }
        for direction in [Direction::Out, Direction::In] {
            let expected = baseline.frames.iter().filter(|f| f.direction == direction).count();
            let got = observed.frames.iter().filter(|f| f.direction == direction).count();
            if expected != got {
                out.segments.push(SegmentsDiff { direction, expected, got });
            }
        }
        out
    }
}

fn first_difference(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()))
}

fn missing(a: &[u8], b: &[u8]) -> usize {
    a.len().abs_diff(b.len())
}

/// One run: the wire, both ends, the faults of the path and the divergence.
#[derive(Clone, Debug)]
pub struct Run {
    pub frames: Vec<Frame>,
    pub client: Endpoint,
    pub server: Endpoint,
    pub faults: Vec<Fault>,
    pub divergence: Divergence,
}

impl Run {
    /// The bytes of each segment on the wire, in order.
    pub fn wire(&self) -> Vec<Vec<u8>> {
        self.frames.iter().map(Frame::bytes).collect()
    }

    /// The segments toward `direction`.
    pub fn toward(&self, direction: Direction) -> Vec<&Segment> {
        self.frames.iter().filter(|f| f.direction == direction).map(|f| &f.segment).collect()
    }
}

/// The simulator. Each call of [`Simulator::run`] is a new conversation.
#[derive(Clone, Debug)]
pub struct Simulator {
    seed: u64,
    policy: Policy,
}

impl Simulator {
    pub fn new(seed: u64) -> Simulator {
        Simulator { seed, policy: Policy::default() }
    }

    pub fn with_policy(mut self, policy: Policy) -> Simulator {
        self.policy = policy;
        self
    }

    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Run `strategy` and report the divergence from a run with no strategy.
    pub fn run(&self, strategy: &Strategy) -> Run {
        self.run_with(strategy, Scenario::Echo)
    }

    /// Run `strategy` in `scenario`.
    pub fn run_with(&self, strategy: &Strategy, scenario: Scenario) -> Run {
        let observed = self.play(&strategy.steps, scenario);
        let baseline = self.play(&[], scenario);
        Run { divergence: Divergence::between(&baseline, &observed), ..observed }
    }

    /// The run of a path that refuses a plain datagram. A framed datagram
    /// passes (rule FAKE-1).
    pub fn udp_echo(&self, payload: &[u8]) -> Result<Vec<u8>, crate::error::Refusal> {
        if self.policy.refuse_plain_udp {
            return Err(crate::error::Refusal::new("the path refuses a plain udp datagram"));
        }
        Ok(payload.to_vec())
    }

    /// A pair of datagram channels for [`crate::fake_tcp::FakeTcp`]. When the
    /// path refuses a plain datagram, only a datagram with the frame magic
    /// passes.
    pub fn fake_tcp_pair(&self) -> (crate::fake_tcp::Channel, crate::fake_tcp::Channel) {
        if self.policy.refuse_plain_udp {
            crate::fake_tcp::filtered_pair(|bytes| bytes.starts_with(crate::fake_tcp::MAGIC))
        } else {
            crate::fake_tcp::pair()
        }
    }

    /// The observation of one unusual segment (rule PROBE-1). It is a local
    /// run, so it always returns an observation.
    pub fn probe(&self, kind: ProbeKind) -> Observation {
        self.observe(kind)
    }

    /// The first outbound data batch: two segments of MSS bytes each.
    pub fn outbound_data(&self) -> Vec<Segment> {
        let mut session = Session::new(self.seed, &self.policy, &[]);
        session.handshake();
        session.client.send_data(b"hello, sidecar!")
    }

    /// Carry `segments` to a new server after a handshake, and give the
    /// bytes that it delivered and its state.
    pub fn deliver(&self, segments: &[Segment]) -> (Vec<u8>, TcpState) {
        let mut session = Session::new(self.seed, &self.policy, &[]);
        session.handshake();
        for segment in segments {
            session.push(Direction::Out, vec![segment.clone()]);
            session.pump();
        }
        (session.server.stream().to_vec(), session.server.state)
    }

    fn play(&self, steps: &[Step], scenario: Scenario) -> Run {
        let mut session = Session::new(self.seed, &self.policy, steps);
        match scenario {
            Scenario::Echo => {
                session.handshake();
                if session.client.state == TcpState::Established {
                    for payload in [b"hello, sidecar!".as_slice(), b"again".as_slice()] {
                        session.exchange(payload);
                    }
                    session.finish();
                }
            }
            Scenario::Refused => session.refused(),
        }
        session.into_run()
    }

    fn observe(&self, kind: ProbeKind) -> Observation {
        let mut session = Session::new(self.seed, &self.policy, &[]);
        session.handshake();
        if session.client.state != TcpState::Established {
            return Observation::silent();
        }
        session.observe(kind)
    }
}

/// One conversation, with its queue of segments that wait for delivery.
struct Session<'a> {
    client: Endpoint,
    server: Endpoint,
    medium: Medium<'a>,
    frames: Vec<Frame>,
    queue: VecDeque<(Direction, Vec<Segment>)>,
}

impl<'a> Session<'a> {
    fn new(seed: u64, policy: &'a Policy, steps: &'a [Step]) -> Session<'a> {
        let client = Endpoint::client(0x1000_0000u32.wrapping_add(seed as u32 & 0xffff));
        let server = Endpoint::server(0x2000_0000u32.wrapping_add(seed as u32 & 0xffff));
        let medium = Medium::new(steps, policy);
        Session { client, server, medium, frames: Vec::new(), queue: VecDeque::new() }
    }

    fn push(&mut self, direction: Direction, segments: Vec<Segment>) {
        if !segments.is_empty() {
            self.queue.push_back((direction, segments));
        }
    }

    fn pump(&mut self) {
        while let Some((direction, batch)) = self.queue.pop_front() {
            let passed = self.medium.pass(direction, batch);
            for segment in passed {
                let reply = {
                    let target = if direction == Direction::Out { &mut self.server } else { &mut self.client };
                    target.receive(&segment).reply
                };
                self.frames.push(Frame { direction, segment });
                if let Some(reply) = reply {
                    self.queue.push_back((direction.other(), vec![reply]));
                }
            }
        }
    }

    fn handshake(&mut self) {
        let Some(syn) = self.client.connect() else { return };
        self.push(Direction::Out, vec![syn]);
        self.pump();
    }

    fn refused(&mut self) {
        let Some(syn) = self.client.connect() else { return };
        let passed = self.medium.pass(Direction::Out, vec![syn]);
        for segment in passed {
            self.frames.push(Frame { direction: Direction::Out, segment });
        }
        let mut rst = Segment::rst(self.server.iss, self.client.iss.wrapping_add(1));
        rst.seal();
        rst.reverse();
        self.push(Direction::In, vec![rst]);
        self.pump();
    }

    /// The client sends `payload`, and the server echoes what it accepted.
    fn exchange(&mut self, payload: &[u8]) {
        let before = self.server.stream().len();
        let batch = self.client.send_data(payload);
        if batch.is_empty() {
            return;
        }
        self.push(Direction::Out, batch);
        self.pump();
        let accepted = self.server.stream()[before..].to_vec();
        if !accepted.is_empty() {
            let echo = self.server.send_data(&accepted);
            self.push(Direction::In, echo);
            self.pump();
        }
    }

    fn finish(&mut self) {
        if let Some(fin) = self.client.close() {
            self.push(Direction::Out, vec![fin]);
            self.pump();
        }
        if let Some(fin) = self.server.close() {
            self.push(Direction::In, vec![fin]);
            self.pump();
        }
    }

    fn observe(&mut self, kind: ProbeKind) -> Observation {
        let start = self.frames.len();
        let before = self.server.stream().len();
        let mut expected_ack = None;
        let expected_stream;
        match kind {
            ProbeKind::OutOfWindow => {
                expected_ack = Some(self.server.rcv_nxt);
                expected_stream = Some(Vec::new());
                let mut segment = Segment::data(
                    self.client.snd_nxt.wrapping_add(u32::from(WINDOW) + 10),
                    self.client.rcv_nxt,
                    b"out-of-window".to_vec(),
                );
                segment.seal();
                self.push(Direction::Out, vec![segment]);
            }
            ProbeKind::Retransmit => {
                expected_stream = Some(b"probe".to_vec());
                let batch = self.client.send_data(b"probe");
                let Some(first) = batch.first().cloned() else { return Observation::silent() };
                self.push(Direction::Out, batch);
                self.pump();
                let mut changed = first;
                changed.payload = b"PROBE".to_vec();
                changed.seal();
                self.push(Direction::Out, vec![changed]);
            }
            ProbeKind::FinBeforeAck => {
                expected_stream = Some(b"fin".to_vec());
                let data = self.client.send_data(b"fin");
                let fin = self.client.close();
                self.push(Direction::Out, data);
                if let Some(fin) = fin {
                    self.push(Direction::Out, vec![fin]);
                }
            }
        }
        self.pump();
        let responses = self.frames[start..]
            .iter()
            .filter(|frame| frame.direction == Direction::In)
            .map(|frame| frame.segment.clone())
            .collect();
        Observation {
            responses,
            stream: self.server.stream()[before..].to_vec(),
            expected_ack,
            expected_stream,
            fault: self.medium.faults.last().copied(),
        }
    }

    fn into_run(self) -> Run {
        Run {
            frames: self.frames,
            client: self.client,
            server: self.server,
            faults: self.medium.faults,
            divergence: Divergence::default(),
        }
    }
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.summary())
    }
}
