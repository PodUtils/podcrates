//! One end of a TCP-like connection: the handshake, the data phase, the
//! window, reassembly and the FIN rules. It is a value, with no socket and
//! no clock, so the simulator gives the same run for the same input.

use std::collections::BTreeMap;

use crate::packet::{MSS, Segment, WINDOW};
use crate::state::{Side, TcpState};

/// What one received segment did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The handshake advanced.
    Handshake,
    /// Payload arrived in order and entered the stream.
    Data,
    /// A pure ACK advanced the send side.
    Acked,
    /// The FIN arrived at the expected sequence number.
    Fin,
    /// The segment was seen before.
    Duplicate,
    /// The segment starts after a hole; it waits in the window.
    OutOfOrder,
    /// The segment starts outside the window.
    OutOfWindow,
    /// The checksum does not match.
    BadChecksum,
    /// A retransmission carries bytes that differ from the first copy.
    RetransmissionMismatch,
    /// The FIN starts ahead of the data, so it cannot close the stream.
    FinBeforeAck,
    /// The peer reset the connection.
    Reset,
    /// The state has no rule for this segment.
    Ignored,
}

/// The counts of one end. A test reads them from outside the run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub received: u64,
    pub duplicates: u64,
    pub out_of_order: u64,
    pub out_of_window: u64,
    pub bad_checksum: u64,
    pub retransmission_mismatch: u64,
    pub fin_before_ack: u64,
    pub resets: u64,
}

/// The answer of one end to one segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Received {
    pub outcome: Outcome,
    /// The segment that this end sends back, if it sends one.
    pub reply: Option<Segment>,
}

impl Received {
    fn new(outcome: Outcome, reply: Option<Segment>) -> Received {
        Received { outcome, reply }
    }
}

/// One end of a connection.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub side: Side,
    pub state: TcpState,
    pub iss: u32,
    /// The next sequence number to send.
    pub snd_nxt: u32,
    /// The next sequence number to receive.
    pub rcv_nxt: u32,
    pub window: u16,
    /// The sequence number of `stream[0]`.
    stream_start: u32,
    stream: Vec<u8>,
    out_of_order: BTreeMap<u32, Vec<u8>>,
    pub counts: Counts,
}

impl Endpoint {
    pub fn client(iss: u32) -> Endpoint {
        Endpoint::new(Side::Client, TcpState::Closed, iss)
    }

    pub fn server(iss: u32) -> Endpoint {
        Endpoint::new(Side::Server, TcpState::Listen, iss)
    }

    fn new(side: Side, state: TcpState, iss: u32) -> Endpoint {
        Endpoint {
            side,
            state,
            iss,
            snd_nxt: iss,
            rcv_nxt: 0,
            window: WINDOW,
            stream_start: 0,
            stream: Vec::new(),
            out_of_order: BTreeMap::new(),
            counts: Counts::default(),
        }
    }

    /// The bytes that this end delivered to its application, in order.
    pub fn stream(&self) -> &[u8] {
        &self.stream
    }

    /// The payloads that wait behind a hole.
    pub fn waiting(&self) -> usize {
        self.out_of_order.len()
    }

    fn orient(&self, mut segment: Segment) -> Segment {
        if self.side == Side::Server {
            segment.reverse();
        }
        segment.seal();
        segment
    }

    fn answer(&self, segment: Segment) -> Received {
        Received::new(Outcome::Acked, Some(self.orient(segment)))
    }

    fn ack(&self) -> Segment {
        Segment::ack(self.snd_nxt, self.rcv_nxt)
    }

    /// Send a SYN and enter SYN-SENT. A server never calls this.
    pub fn connect(&mut self) -> Option<Segment> {
        if self.state != TcpState::Closed || self.side != Side::Client {
            return None;
        }
        let syn = Segment::syn(self.iss);
        self.snd_nxt = self.iss.wrapping_add(1);
        self.state = TcpState::SynSent;
        Some(self.orient(syn))
    }

    /// Send payload in segments of at most `MSS` bytes. Each segment is
    /// oriented from this end.
    pub fn send_data(&mut self, payload: &[u8]) -> Vec<Segment> {
        if self.state != TcpState::Established && self.state != TcpState::CloseWait {
            return Vec::new();
        }
        let mut out = Vec::new();
        for chunk in payload.chunks(MSS) {
            let mut segment = Segment::data(self.snd_nxt, self.rcv_nxt, chunk.to_vec());
            segment.seal();
            out.push(self.orient(segment));
            self.snd_nxt = self.snd_nxt.wrapping_add(chunk.len() as u32);
        }
        out
    }

    /// Send a FIN.
    pub fn close(&mut self) -> Option<Segment> {
        let next = match self.state {
            TcpState::Established => TcpState::FinWait1,
            TcpState::CloseWait => TcpState::LastAck,
            _ => return None,
        };
        let mut fin = Segment::fin(self.snd_nxt, self.rcv_nxt);
        fin.seal();
        self.snd_nxt = self.snd_nxt.wrapping_add(1);
        self.state = next;
        Some(self.orient(fin))
    }

    /// Receive one segment that comes toward this end.
    pub fn receive(&mut self, segment: &Segment) -> Received {
        if !segment.checksum_valid() {
            self.counts.bad_checksum += 1;
            return Received::new(Outcome::BadChecksum, None);
        }
        self.counts.received += 1;
        if segment.is_rst() {
            self.counts.resets += 1;
            self.state = TcpState::Closed;
            return Received::new(Outcome::Reset, None);
        }
        match self.state {
            TcpState::Listen => self.on_listen(segment),
            TcpState::SynSent => self.on_syn_sent(segment),
            TcpState::SynReceived => self.on_syn_received(segment),
            TcpState::Established
            | TcpState::CloseWait
            | TcpState::FinWait1
            | TcpState::FinWait2
            | TcpState::Closing
            | TcpState::LastAck
            | TcpState::TimeWait => self.on_open(segment),
            TcpState::Closed => Received::new(Outcome::Ignored, None),
        }
    }

    fn on_listen(&mut self, segment: &Segment) -> Received {
        if !segment.is_syn() || segment.is_ack() {
            return Received::new(Outcome::Ignored, None);
        }
        self.rcv_nxt = segment.seq.wrapping_add(1);
        self.snd_nxt = self.iss.wrapping_add(1);
        self.state = TcpState::SynReceived;
        let syn_ack = Segment::syn_ack(self.iss, self.rcv_nxt);
        Received::new(Outcome::Handshake, Some(self.orient(syn_ack)))
    }

    fn on_syn_sent(&mut self, segment: &Segment) -> Received {
        if !segment.is_syn() || !segment.is_ack() {
            return Received::new(Outcome::Ignored, None);
        }
        if segment.ack != self.snd_nxt {
            return Received::new(Outcome::Ignored, None);
        }
        self.rcv_nxt = segment.seq.wrapping_add(1);
        self.state = TcpState::Established;
        self.answer(self.ack())
    }

    fn on_syn_received(&mut self, segment: &Segment) -> Received {
        if segment.is_ack() && segment.ack == self.snd_nxt {
            self.state = TcpState::Established;
            return Received::new(Outcome::Handshake, None);
        }
        Received::new(Outcome::Ignored, None)
    }

    fn on_open(&mut self, segment: &Segment) -> Received {
        if segment.is_syn() {
            return Received::new(Outcome::Ignored, None);
        }
        let mut outcome = Outcome::Acked;
        if segment.has_payload() {
            let (seen, reply) = self.accept_data(segment);
            outcome = seen;
            if !segment.is_fin() {
                return Received::new(outcome, reply);
            }
        }
        if segment.is_fin() {
            let (seen, reply) = self.accept_fin(segment);
            // A mismatch on the data is the stronger fact.
            if outcome == Outcome::RetransmissionMismatch {
                return Received::new(outcome, reply);
            }
            return Received::new(seen, reply);
        }
        if segment.is_ack() {
            self.on_ack(segment);
        }
        Received::new(outcome, None)
    }

    fn on_ack(&mut self, segment: &Segment) {
        match self.state {
            TcpState::FinWait1 if segment.ack == self.snd_nxt => self.state = TcpState::FinWait2,
            TcpState::LastAck if segment.ack == self.snd_nxt => self.state = TcpState::Closed,
            TcpState::Closing if segment.ack == self.snd_nxt => self.state = TcpState::TimeWait,
            _ => {}
        }
    }

    fn accept_data(&mut self, segment: &Segment) -> (Outcome, Option<Segment>) {
        let ahead = ahead(segment.seq, self.rcv_nxt);
        let len = segment.payload.len() as u32;
        if ahead == 0 {
            if self.stream.is_empty() && self.out_of_order.is_empty() {
                self.stream_start = segment.seq;
            }
            self.stream.extend_from_slice(&segment.payload);
            self.rcv_nxt = self.rcv_nxt.wrapping_add(len);
            self.drain();
            return (Outcome::Data, Some(self.orient(self.ack())));
        }
        if ahead > 0 && ahead <= i64::from(self.window) {
            self.out_of_order.entry(segment.seq).or_insert_with(|| segment.payload.clone());
            self.counts.out_of_order += 1;
            return (Outcome::OutOfOrder, Some(self.orient(self.ack())));
        }
        if ahead > i64::from(self.window) {
            self.counts.out_of_window += 1;
            return (Outcome::OutOfWindow, Some(self.orient(self.ack())));
        }
        self.counts.duplicates += 1;
        if let Some(stored) = self.stored(segment.seq, len)
            && stored != segment.payload.as_slice()
        {
            self.counts.retransmission_mismatch += 1;
            return (Outcome::RetransmissionMismatch, Some(self.orient(self.ack())));
        }
        (Outcome::Duplicate, Some(self.orient(self.ack())))
    }

    fn accept_fin(&mut self, segment: &Segment) -> (Outcome, Option<Segment>) {
        let ahead = ahead(segment.seq, self.rcv_nxt);
        if ahead == 0 {
            self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
            self.state = match self.state {
                TcpState::Established => TcpState::CloseWait,
                TcpState::FinWait1 => TcpState::TimeWait,
                TcpState::FinWait2 => TcpState::TimeWait,
                other => other,
            };
            return (Outcome::Fin, Some(self.orient(self.ack())));
        }
        if ahead > 0 {
            self.counts.fin_before_ack += 1;
            return (Outcome::FinBeforeAck, Some(self.orient(self.ack())));
        }
        self.counts.duplicates += 1;
        (Outcome::Duplicate, Some(self.orient(self.ack())))
    }

    /// The bytes of the stream that start at `seq`, where they are known.
    fn stored(&self, seq: u32, len: u32) -> Option<&[u8]> {
        let offset = seq.wrapping_sub(self.stream_start);
        let end = offset.checked_add(len)? as usize;
        if offset > self.stream.len() as u32 || end > self.stream.len() {
            return None;
        }
        self.stream.get(offset as usize..end)
    }

    /// Move every waiting segment that now starts at the window edge into
    /// the stream.
    fn drain(&mut self) {
        while let Some(payload) = self.out_of_order.remove(&self.rcv_nxt) {
            self.stream.extend_from_slice(&payload);
            self.rcv_nxt = self.rcv_nxt.wrapping_add(payload.len() as u32);
        }
        self.out_of_order.retain(|seq, _| ahead(*seq, self.rcv_nxt) > 0);
    }
}

/// The distance from `b` to `a`, in the sequence space of TCP.
fn ahead(a: u32, b: u32) -> i64 {
    i64::from(a.wrapping_sub(b) as i32)
}
