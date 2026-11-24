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
            events.push(Event::NeedData { offset: self.count, max: self.chunk });
            self.phase = Phase::Supply { offset: self.count, max: self.chunk };
            self.wait_caller();
        }
    }

    pub(super) fn on_ack(&mut self, of: u64, count: u64, now: Instant, events: &mut Vec<Event>) {
        let Some((offset, len, handed)) =
            self.flight.as_ref().map(|f| (f.offset, f.data.len() as u64, f.handed.filter(|_| !f.resent)))
        else {
            return;
        };
        if of != offset {
            return; // Stale: an ack of a frame before.
        }
        if count < of || count > of + len || count > self.size() {
            return self.protocol("ACKN counts bytes that were not sent", events);
        }
        if count == of {
            return self.resend();
        }
        if let Some(flight) = self.flight.take() {
            let acked = usize::try_from(count - of).unwrap_or(usize::MAX);
            self.hash.update(&flight.data[..acked]);
        }
        if let Some(ack) = handed.map(|h| now.saturating_duration_since(h)) {
            self.last_ack = Some(ack);
            if ack <= FAST_ACK {
                self.chunk = (self.chunk * 2).min(self.cap);
            } else if ack > SLOW_ACK {
                self.chunk = (self.chunk / 2).max(self.floor());
            }
        }
        self.count = count;
        events.push(Event::Progress { done: count, size: self.size() });
        self.next_data(events);
    }
}
