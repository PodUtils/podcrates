//! The receiver's half of a session: the offer, the bytes in order, the
//! check of `DONE`, the frame `HOLD` while its caller acts, and `KEPT`.

use sha2::Digest;

use super::{Phase, Session};
use crate::transfer::linger::Verdict;
use crate::transfer::wire::{Msg, reason_text};
use crate::transfer::{Ended, Event, stop};

impl Session {
    pub(in crate::transfer) fn take(&mut self) {
        if self.phase == Phase::Offered {
            self.answer = Some(self.send(&Msg::Take { from: 0 }));
            self.phase = Phase::Receiving;
            self.wait_peer(None);
        }
    }

    pub(in crate::transfer) fn skip(&mut self, reason: &str, events: &mut Vec<Event>) {
        if self.phase == Phase::Offered {
            self.send(&Msg::Skip { reason: reason.as_bytes().to_vec() });
            let reason = reason_text(reason.as_bytes());
            self.end(Ended::Stopped { code: stop::REFUSED, by_peer: false, reason }, events);
        }
    }

    /// `KEPT`, and the verdict that answers a `DONE` that comes again after
    /// the end.
    pub(in crate::transfer) fn kept(&mut self, events: &mut Vec<Event>) {
        if self.phase == Phase::Verify {
            let frame = self.send(&Msg::Kept { size: self.count });
            self.verdict = Some(Verdict::new(self.count, self.hash.clone().finalize().into(), frame));
            self.end(Ended::Done, events);
        }
    }

    /// A frame of the sender came again. While the caller acts (an offer,
    /// a rename), `HOLD` tells the sender that this end is there: the
    /// sender counts its `WAIT` from the last frame of any kind, so a person
    /// can take minutes. After `TAKE`, the same `TAKE` goes again.
    pub(super) fn answer_again(&mut self) {
        match self.phase {
            Phase::Offered | Phase::Verify => {
                self.send(&Msg::Hold);
            }
            _ => {
                if let Some(answer) = &self.answer {
                    self.out.extend_from_slice(answer);
                }
            }
        }
    }

    pub(super) fn on_data(&mut self, offset: u64, bytes: &[u8], events: &mut Vec<Event>) {
        if bytes.is_empty() || bytes.len() > self.cap || offset > self.count {
            return self.protocol("DATA out of order or out of size", events);
        }
        let skip = usize::try_from(self.count - offset).unwrap_or(usize::MAX);
        if let Some(new) = bytes.get(skip..).filter(|n| !n.is_empty()) {
            if self.count + new.len() as u64 > self.size() {
                return self.protocol("DATA past the size", events);
            }
            self.hash.update(new);
            events.push(Event::Write { offset: self.count, bytes: new.to_vec() });
            self.count += new.len() as u64;
            events.push(Event::Progress { done: self.count, size: self.size() });
        }
        self.send(&Msg::Ackn { of: offset, count: self.count });
    }

    pub(super) fn on_done(&mut self, size: u64, sha256: [u8; 32], events: &mut Vec<Event>) {
        if self.phase == Phase::Verify {
            return self.answer_again();
        }
        if size != self.count || size != self.size() {
            return self.stop(stop::DIFFERS, "the size differs; nothing was kept", events);
        }
        let ours: [u8; 32] = self.hash.clone().finalize().into();
        if ours != sha256 {
            return self.stop(stop::DIFFERS, "SHA-256 differs; nothing was kept", events);
        }
        events.push(Event::Verified { size, sha256 });
        self.phase = Phase::Verify;
        self.wait_caller();
    }
}
