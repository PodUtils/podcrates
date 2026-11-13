//! The send side of the ARQ: the records cut into fragments, the frames in
//! flight, the acknowledgements, the retransmission of each lost fragment,
//! the probe of a closed window, and the wrap of the sequence.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::error::ArqError;
use super::rto::Rto;
use super::settings::ArqSettings;
use super::stats::Stats;
use super::wire::{Bitmap, Datagram, after, before};

/// The frames above a lost one whose acknowledgement sends it again before
/// its timer (the packet threshold of RFC 9002).
const REORDER: u32 = 3;

/// One frame of the sender: a fragment of a record, or the end.
#[derive(Debug, Clone)]
pub(crate) enum Body {
    Data { index: u16, count: u16, payload: Vec<u8> },
    Fin,
}

impl Body {
    fn bytes(&self) -> usize {
        match self {
            Body::Data { payload, .. } => payload.len(),
            Body::Fin => 0,
        }
    }

    pub(crate) fn datagram(&self, seq: u32) -> Datagram {
        match self {
            Body::Data { index, count, payload } => {
                Datagram::Data { seq, index: *index, count: *count, payload: payload.clone() }
            }
            Body::Fin => Datagram::Fin { seq },
        }
    }
}

/// A frame in flight.
struct Flight {
    body: Body,
    first: Instant,
    last: Instant,
    /// The retransmissions so far.
    retries: u8,
    /// The timeouts so far: the RTO of the frame doubles with each.
    backoff: u8,
    /// A retransmission before the timer since the last timeout.
    fast: bool,
    /// In the queue of frames to send again.
    queued: bool,
    acked: bool,
}

/// The probe of a peer whose window is closed.
#[derive(Clone, Copy)]
struct Probe {
    since: Instant,
    last: Instant,
    tries: u8,
    backoff: u8,
}

pub(crate) struct Sender {
    fragment: usize,
    record: usize,
    window: u32,
    send_buffer: usize,
    retries: u8,
    rekey_ahead: u32,
    /// The next new sequence, and the count of sequences used.
    next: u32,
    used: u64,
    /// The oldest frame in flight: `flight[i]` is `una + i`.
    una: u32,
    flight: VecDeque<Flight>,
    queue: VecDeque<Body>,
    /// The bytes of the records that are not acknowledged.
    held: usize,
    resend: VecDeque<u32>,
    /// The first sequence that the peer's window does not take, and the
    /// `next` of the ACK that gave it.
    limit: u32,
    peer_next: u32,
    fin: Fin,
    /// The caller rekeyed: the sequence can pass from `u32::MAX` to 0.
    wrap_ok: bool,
    probe: Option<Probe>,
    /// A probe to send.
    probe_out: bool,
    pub(crate) rto: Rto,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fin {
    Open,
    Queued,
    Sent,
    Acked,
}

impl Sender {
    pub(crate) fn new(s: &ArqSettings, fragment: usize) -> Sender {
        Sender {
            fragment,
            record: s.record,
            window: u32::from(s.window),
            send_buffer: s.send_buffer,
            retries: s.retries,
            rekey_ahead: s.rekey_ahead,
            next: s.start,
            used: 0,
            una: s.start,
            flight: VecDeque::new(),
            queue: VecDeque::new(),
            held: 0,
            resend: VecDeque::new(),
            limit: s.start.wrapping_add(u32::from(s.window)),
            peer_next: s.start,
            fin: Fin::Open,
            wrap_ok: false,
            probe: None,
            probe_out: false,
            rto: Rto::new(s.rto_initial, s.rto_floor, s.rto_ceiling),
        }
    }

    /// The bytes that a write can queue now.
    pub(crate) fn room(&self) -> usize {
        if self.fin != Fin::Open { 0 } else { self.send_buffer.saturating_sub(self.held).min(self.record) }
    }

    /// Queue `bytes` as one record, all of it or none. False when the
    /// buffer has no room for it now.
    pub(crate) fn push_record(&mut self, bytes: &[u8]) -> Result<bool, ArqError> {
        if self.fin != Fin::Open {
            return Err(ArqError::WriteClosed);
        }
        if bytes.is_empty() {
            return Ok(true);
        }
        if bytes.len() > self.room() {
            return Ok(false);
        }
        let count = bytes.len().div_ceil(self.fragment);
        let count = u16::try_from(count).expect("a record of at most 65535 fragments (settings)");
        for (index, chunk) in bytes.chunks(self.fragment).enumerate() {
            self.queue.push_back(Body::Data { index: index as u16, count, payload: chunk.to_vec() });
        }
        self.held += bytes.len();
        Ok(true)
    }

    /// Queue the end of the bytes, after the records.
    pub(crate) fn close(&mut self) {
        if self.fin == Fin::Open {
            self.fin = Fin::Queued;
        }
    }

    /// The end was sent and acknowledged.
    pub(crate) fn closed(&self) -> bool {
        self.fin == Fin::Acked
    }

    pub(crate) fn write_closed(&self) -> bool {
        self.fin != Fin::Open
    }

    /// Each record and the end are acknowledged, or nothing waits.
    pub(crate) fn idle(&self) -> bool {
        self.flight.is_empty() && self.queue.is_empty() && self.fin != Fin::Queued
    }

    pub(crate) fn in_flight(&self) -> usize {
        self.flight.iter().filter(|f| !f.acked).count()
    }

    pub(crate) fn queued_bytes(&self) -> usize {
        self.held
    }

    /// The frames before the wrap of the sequence to 0.
    fn left(&self) -> u64 {
        if self.next == 0 && self.used > 0 { 0 } else { (1u64 << 32) - u64::from(self.next) }
    }

    pub(crate) fn rekey_due(&self) -> bool {
        !self.wrap_ok && self.left() <= u64::from(self.rekey_ahead)
    }

    pub(crate) fn rekeyed(&mut self) {
        self.wrap_ok = true;
    }

    /// The windows and the wrap let a new frame go.
    fn can_take(&self) -> bool {
        after(self.una, self.next) < self.window && before(self.next, self.limit) && (self.left() > 0 || self.wrap_ok)
    }

    /// The sequence of a new frame, when the windows and the wrap let one go.
    fn take_seq(&mut self, stats: &mut Stats) -> Option<u32> {
        if !self.can_take() {
            return None;
        }
        if self.left() == 0 {
            // The wrap: never use a sequence again under the same keys.
            self.wrap_ok = false;
            stats.rekeys += 1;
        }
        let seq = self.next;
        self.next = self.next.wrapping_add(1);
        self.used += 1;
        Some(seq)
    }

    /// The next datagram of the send side: a frame again, a new frame, or a
    /// probe of a closed window.
    pub(crate) fn poll_transmit(&mut self, now: Instant, stats: &mut Stats) -> Option<Datagram> {
        if std::mem::take(&mut self.probe_out) {
            return Some(Datagram::Probe);
        }
        while let Some(seq) = self.resend.pop_front() {
            let i = after(self.una, seq) as usize;
            let Some(f) = self.flight.get_mut(i).filter(|f| f.queued && !f.acked) else { continue };
            f.queued = false;
            f.last = now;
            stats.retransmitted += 1;
            return Some(f.body.datagram(seq));
        }
        let body_ready = !self.queue.is_empty() || self.fin == Fin::Queued;
        if body_ready && let Some(seq) = self.take_seq(stats) {
            let body = match self.queue.pop_front() {
                Some(body) => body,
                None => {
                    self.fin = Fin::Sent;
                    Body::Fin
                }
            };
            let datagram = body.datagram(seq);
            let f = Flight {
                body,
                first: now,
                last: now,
                retries: 0,
                backoff: 0,
                fast: false,
                queued: false,
                acked: false,
            };
            self.flight.push_back(f);
            stats.sent += 1;
            self.probe = None;
            return Some(datagram);
        }
        self.watch(now);
        None
    }

    /// Start the probe when bytes wait, nothing is in flight, and no new
    /// frame can go; stop it when one can.
    fn watch(&mut self, now: Instant) {
        let body_ready = !self.queue.is_empty() || self.fin == Fin::Queued;
        let blocked = body_ready && self.flight.is_empty() && !self.can_take();
        match (&self.probe, blocked) {
            (None, true) => self.probe = Some(Probe { since: now, last: now, tries: 0, backoff: 0 }),
            (Some(_), false) => self.probe = None,
            _ => {}
        }
    }

    /// Take an `ACK` of the peer. False when it acknowledges a frame not
    /// sent: the ARQ drops it.
    pub(crate) fn on_ack(&mut self, now: Instant, next: u32, window: u16, bitmap: &Bitmap) -> bool {
        if before(self.next, next) {
            return false;
        }
        if !before(next, self.peer_next) {
            self.peer_next = next;
            self.limit = next.wrapping_add(u32::from(window));
            if let Some(p) = &mut self.probe {
                p.tries = 0;
                p.since = now;
            }
        }
        let mut sample: Option<Instant> = None;
        let mut highest: Option<u32> = None;
        for (i, f) in self.flight.iter_mut().enumerate() {
            let seq = self.una.wrapping_add(i as u32);
            let covered = before(seq, next) || bitmap.get(after(next, seq) as usize);
            if !covered {
                continue;
            }
            highest = Some(seq);
            if f.acked {
                continue;
            }
            f.acked = true;
            if f.retries == 0 && sample.is_none_or(|last| f.last > last) {
                sample = Some(f.last);
            }
        }
        if let Some(last) = sample {
            self.rto.sample(now.saturating_duration_since(last));
        }
        while self.flight.front().is_some_and(|f| f.acked) {
            let f = self.flight.pop_front().expect("a front");
            if matches!(f.body, Body::Fin) {
                self.fin = Fin::Acked;
            }
            self.held -= f.body.bytes();
            self.una = self.una.wrapping_add(1);
        }
        if let Some(high) = highest {
            self.fast_retransmit(high);
        }
        self.watch(now);
        true
    }

    /// Send again, once before its timer, each frame that has
    /// [`REORDER`] or more acknowledged frames above it.
    fn fast_retransmit(&mut self, highest: u32) {
        for (i, f) in self.flight.iter_mut().enumerate() {
            let seq = self.una.wrapping_add(i as u32);
            if !before(seq, highest) || after(seq, highest) < REORDER {
                break;
            }
            if f.acked || f.fast || f.queued || f.retries >= self.retries {
                continue;
            }
            f.fast = true;
            f.queued = true;
            f.retries += 1;
            self.resend.push_back(seq);
        }
    }

    /// The deadline of a frame in flight, or of the probe.
    pub(crate) fn poll_timeout(&self) -> Option<Instant> {
        let frames =
            self.flight.iter().filter(|f| !f.acked && !f.queued).map(|f| f.last + self.rto.backed_off(f.backoff)).min();
        let probe = self.probe.map(|p| p.last + self.rto.backed_off(p.backoff));
        frames.into_iter().chain(probe).min()
    }

    /// Run the timers at `now`: queue each frame whose RTO passed, and the
    /// probe. An error when a frame or the probe has no retry left.
    pub(crate) fn on_timeout(&mut self, now: Instant, stats: &mut Stats) -> Result<(), ArqError> {
        for (i, f) in self.flight.iter_mut().enumerate() {
            if f.acked || f.queued || now < f.last + self.rto.backed_off(f.backoff) {
                continue;
            }
            let seq = self.una.wrapping_add(i as u32);
            if f.retries >= self.retries {
                return Err(ArqError::Timeout { seq, retries: f.retries, waited: now.duration_since(f.first) });
            }
            f.retries += 1;
            f.backoff = f.backoff.saturating_add(1);
            f.fast = false;
            f.queued = true;
            stats.timeouts += 1;
            self.resend.push_back(seq);
        }
        let Some(p) = &mut self.probe else { return Ok(()) };
        if self.probe_out || now < p.last + self.rto.backed_off(p.backoff) {
            return Ok(());
        }
        if p.tries >= self.retries {
            return Err(ArqError::Timeout { seq: self.next, retries: p.tries, waited: now.duration_since(p.since) });
        }
        p.tries += 1;
        p.backoff = p.backoff.saturating_add(1);
        p.last = now;
        stats.probes += 1;
        self.probe_out = true;
        Ok(())
    }

    /// The RTO of a frame sent now.
    pub(crate) fn rto(&self) -> Duration {
        self.rto.rto()
    }
}
