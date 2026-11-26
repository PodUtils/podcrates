//! The sender's half of a session: the bytes that its caller supplies,
//! the acks, the chunk and `DONE`.

use std::time::Instant;

use sha2::Digest;

use super::{Phase, Session};
use crate::transfer::wire::Msg;
use crate::transfer::{Event, FAST_ACK, SLOW_ACK, stop};

impl Session {
    pub(in crate::transfer) fn supply(&mut self, bytes: &[u8], events: &mut Vec<Event>) {
        let Phase::Supply { offset, max } = self.phase else { return };
        if bytes.is_empty() {
            return self.stop(stop::LOCAL, "the file ended before its size", events);
        }
        let left = usize::try_from(self.size() - offset).unwrap_or(usize::MAX);
        let bytes = bytes[..bytes.len().min(max).min(left)].to_vec();
        // Each byte goes into the hash once, when it is first supplied: an
        // ack can count bytes of an earlier frame that this end cut since.
        let new = usize::try_from(self.sent.saturating_sub(offset)).unwrap_or(usize::MAX);
        if let Some(new) = bytes.get(new..) {
            self.hash.update(new);
            self.sent += new.len() as u64;
        }
        self.fly(&Msg::Data { offset, bytes });
        self.phase = Phase::Data;
        self.wait_peer(None);
    }

    pub(super) fn on_take(&mut self, from: u64, events: &mut Vec<Event>) {
        if from != 0 {
            return self.protocol("TAKE asks for a resume", events);
        }
        self.flight = None;
        self.next_data(events);
    }

    /// The sender asks its caller for the bytes at its count, or sends
    /// `DONE` after the last one.
    fn next_data(&mut self, events: &mut Vec<Event>) {
        let size = self.size();
        if self.count == size {
            let sha256 = self.hash.clone().finalize().into();
            self.fly(&Msg::Done { size, sha256 });
            self.phase = Phase::AwaitKept;
            self.wait_peer(None);
        } else {
            // The chunk, as the design's vectors give it: the caller reads up
            // to it, and `supply` takes no byte past the size.
            let max = self.chunk.min(self.limit());
            events.push(Event::NeedData { offset: self.count, max });
            self.phase = Phase::Supply { offset: self.count, max };
            self.wait_caller();
        }
    }

    pub(super) fn on_ack(&mut self, of: u64, count: u64, now: Instant, events: &mut Vec<Event>) {
        let Some((offset, handed)) = self.flight.as_ref().map(|f| (f.offset, f.handed.filter(|_| !f.resent))) else {
            return;
        };
        if of != offset {
            return; // Stale: an ack of a frame before.
        }
        // The count can pass the frame in flight: a longer frame of this
        // offset, cut at its resend, came late.
        if count < of || count > self.sent || count > self.size() {
            return self.protocol("ACKN counts bytes that were not sent", events);
        }
        if count == of {
            return self.resend();
        }
        self.flight = None;
        if let Some(ack) = handed.map(|h| now.saturating_duration_since(h)) {
            self.last_ack = Some(ack);
            if ack <= FAST_ACK {
                self.chunk = (self.chunk * 2).min(self.limit());
            } else if ack > SLOW_ACK {
                self.chunk = (self.chunk / 2).max(self.floor());
            }
        }
        self.count = count;
        events.push(Event::Progress { done: count, size: self.size() });
        self.next_data(events);
    }
}
