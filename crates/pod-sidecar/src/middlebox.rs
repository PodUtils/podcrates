//! The middlebox of the simulator: it applies the steps of a strategy, then
//! the policy of the path, to each batch of segments.
//!
//! A step fires on its trigger. `Always` fires on every outbound segment;
//! the other triggers fire once. The policy is the model of the path: a path
//! that reassembles, a path that requires a valid checksum, a path that
//! honours the TTL, and a path that refuses a plain datagram.

use crate::packet::{Segment, WINDOW};
use crate::primitive::{Params, Primitive, Step, Trigger};
use crate::simulator::Policy;

/// The way of a segment through the middlebox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// From the protected host to the server.
    Out,
    /// From the server to the protected host.
    In,
}

impl Direction {
    pub fn other(self) -> Direction {
        match self {
            Direction::Out => Direction::In,
            Direction::In => Direction::Out,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Out => "out",
            Direction::In => "in",
        }
    }
}

/// A fact that the middlebox observed, and that a probe reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A FIN arrived while outbound data was not acknowledged.
    FinBeforeAck,
    /// An outbound segment started outside the receiver's window.
    OutOfWindow,
    /// A segment failed the checksum that the path requires.
    Checksum,
}

impl Fault {
    pub fn as_str(self) -> &'static str {
        match self {
            Fault::FinBeforeAck => "fin-before-ack",
            Fault::OutOfWindow => "out-of-window",
            Fault::Checksum => "checksum",
        }
    }
}

/// The middlebox of one run.
pub(crate) struct Medium<'a> {
    steps: &'a [Step],
    policy: &'a Policy,
    fired: Vec<bool>,
    pub faults: Vec<Fault>,
    /// The sequence number that the server expects next.
    expected: u32,
    /// The end of the outbound data that the server has not acknowledged.
    outstanding_end: u32,
    outstanding: bool,
}

impl<'a> Medium<'a> {
    pub fn new(steps: &'a [Step], policy: &'a Policy) -> Medium<'a> {
        Medium {
            steps,
            policy,
            fired: vec![false; steps.len()],
            faults: Vec::new(),
            expected: 0,
            outstanding_end: 0,
            outstanding: false,
        }
    }

    /// Take one batch toward `direction` and give the batch that arrives.
    pub fn pass(&mut self, direction: Direction, batch: Vec<Segment>) -> Vec<Segment> {
        let changed = self.apply(direction, batch);
        let mut out = Vec::with_capacity(changed.len());
        for segment in changed {
            if direction == Direction::Out && segment.is_syn() {
                self.expected = segment.seq.wrapping_add(1);
            }
            if direction == Direction::Out && segment.has_payload() && !segment.is_rst() {
                self.outstanding = true;
                self.outstanding_end = segment.end_seq();
            }
            if direction == Direction::In && segment.is_ack() && self.ack_covers(segment.ack) {
                self.outstanding = false;
            }
            if direction == Direction::Out && segment.has_payload() && segment.seq == self.expected {
                self.expected = segment.end_seq();
            }
            if self.policy.valid_checksum && !segment.checksum_valid() {
                self.faults.push(Fault::Checksum);
                continue;
            }
            if direction == Direction::Out && segment.is_fin() && self.outstanding && self.policy.fin_requires_ack {
                self.faults.push(Fault::FinBeforeAck);
                let mut rst = Segment::rst(segment.seq, segment.ack);
                rst.seal();
                out.push(rst);
                continue;
            }
            if direction == Direction::Out
                && segment.has_payload()
                && self.out_of_window(&segment)
                && self.policy.rst_on_out_of_window
            {
                self.faults.push(Fault::OutOfWindow);
                let mut rst = Segment::rst(segment.seq, segment.ack);
                rst.seal();
                out.push(rst);
                continue;
            }
            out.push(segment);
        }
        out
    }

    /// Whether an ACK covers all outbound data that is still outstanding.
    fn ack_covers(&self, ack: u32) -> bool {
        self.outstanding && self.outstanding_end.wrapping_sub(ack) as i32 <= 0
    }

    /// Whether `segment` starts beyond the window edge of the receiver.
    fn out_of_window(&self, segment: &Segment) -> bool {
        if self.expected == 0 {
            return false;
        }
        let ahead = segment.seq.wrapping_sub(self.expected) as i32;
        ahead > i32::from(WINDOW)
    }

    fn apply(&mut self, direction: Direction, batch: Vec<Segment>) -> Vec<Segment> {
        let mut out: Vec<Segment> = Vec::new();
        let mut coalesce = false;
        for segment in batch {
            let mut pieces = vec![segment];
            for index in 0..self.steps.len() {
                let step = self.steps[index];
                let Some(head) = pieces.first() else { break };
                if !Self::matches(step.trigger, direction, head) {
                    continue;
                }
                if step.trigger != Trigger::Always {
                    if self.fired[index] {
                        continue;
                    }
                    self.fired[index] = true;
                }
                match step.primitive {
                    Primitive::Drop => {
                        pieces.clear();
                        break;
                    }
                    Primitive::Tamper => {
                        for piece in &mut pieces {
                            tamper(piece, step.params);
                        }
                    }
                    Primitive::Duplicate => {
                        let copies = pieces.clone();
                        let count = step.params.count.unwrap_or(1);
                        for _ in 0..count {
                            pieces.extend(copies.iter().cloned());
                        }
                    }
                    Primitive::Fragment => {
                        let at = step.params.offset;
                        pieces = pieces.into_iter().flat_map(|segment| fragment(segment, at)).collect();
                    }
                    Primitive::Coalesce => coalesce = true,
                }
            }
            out.extend(pieces);
        }
        if coalesce { coalesce_contiguous(out) } else { out }
    }

    fn matches(trigger: Trigger, direction: Direction, segment: &Segment) -> bool {
        match trigger {
            Trigger::FirstOutboundData => direction == Direction::Out && segment.has_payload(),
            Trigger::SynAck => direction == Direction::In && segment.is_syn() && segment.is_ack(),
            Trigger::FirstInboundRst => direction == Direction::In && segment.is_rst(),
            Trigger::Always => direction == Direction::Out,
        }
    }
}

fn tamper(segment: &mut Segment, params: Params) {
    let offset = params.offset.unwrap_or(0);
    let mask = params.byte.unwrap_or(0xff);
    if segment.has_payload() {
        segment.tamper_payload(offset, mask);
    } else {
        segment.window ^= u16::from(mask);
        segment.seal();
    }
}

/// Split a segment at `offset`, or at its middle when the offset is absent.
fn fragment(mut segment: Segment, offset: Option<usize>) -> Vec<Segment> {
    let len = segment.payload.len();
    if len < 2 {
        segment.seal();
        return vec![segment];
    }
    let mut at = offset.unwrap_or(len / 2) % len;
    if at == 0 {
        at = 1;
    }
    let tail = segment.payload.split_off(at);
    let mut second = segment.clone();
    second.seq = segment.seq.wrapping_add(at as u32);
    second.payload = tail;
    segment.seal();
    second.seal();
    vec![segment, second]
}

/// Merge each pair of contiguous segments into one.
fn coalesce_contiguous(batch: Vec<Segment>) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for segment in batch {
        match out.last_mut() {
            Some(previous)
                if previous.end_seq() == segment.seq
                    && previous.has_payload()
                    && segment.has_payload()
                    && previous.flags == segment.flags
                    && previous.is_ack()
                    && !previous.is_syn()
                    && !previous.is_fin() =>
            {
                previous.payload.extend_from_slice(&segment.payload);
                previous.seal();
            }
            _ => out.push(segment),
        }
    }
    out
}
