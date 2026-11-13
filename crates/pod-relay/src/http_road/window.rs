//! The receiver's side of the batches up, as the relay role holds them, and
//! the refusals of the road: the invariants 6 to 10 (PodUtils/podssh#221)
//! that the role's server keeps. The role's server takes this window, so
//! that its answers follow from one place:
//!
//! - 6: a batch stored before, sent again, gets the same acknowledgement
//!   (`204` with `X-Seq`), never silence.
//! - 7: the acknowledgement goes only after the batch is in the window that
//!   the target reads; a full window is the refusal `full`, and the window
//!   does not move.
//! - 8: each refusal of the role names its reason in `X-Refuse`.
//! - 9: the last batch carries the total of the bytes up (`total=`); a total
//!   that differs from the bytes stored fails the session. The role's end
//!   of a target that closed carries the total of the bytes down
//!   (`X-Total`), which the client compares with its offset.
//! - 10: the window holds each batch by its number in the session, so a
//!   batch sent again replaces nothing and the target reads in number
//!   order. The road keeps no retry log on disk; a log that comes later
//!   is keyed by (session, `seq`) too.

use std::collections::VecDeque;

/// The reasons of a refusal of the role, in `X-Refuse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The window holds no room for the batch: the same batch again later.
    Full,
    /// The role does not know the session, or its key is wrong.
    UnknownSession,
    /// The token or the policy refuses the request.
    NotAllowed,
}

impl Refusal {
    /// The word of the reason, as `X-Refuse` carries it.
    pub fn word(self) -> &'static str {
        match self {
            Refusal::Full => "full",
            Refusal::UnknownSession => "unknown session",
            Refusal::NotAllowed => "not allowed",
        }
    }

    /// The reason that `word` names.
    pub fn parse(word: &str) -> Option<Refusal> {
        match word.trim() {
            "full" => Some(Refusal::Full),
            "unknown session" => Some(Refusal::UnknownSession),
            "not allowed" => Some(Refusal::NotAllowed),
            _ => None,
        }
    }

    /// The status of the refusal's answer.
    pub fn status(self) -> u16 {
        match self {
            Refusal::Full => 503,
            Refusal::UnknownSession => 404,
            Refusal::NotAllowed => 403,
        }
    }
}

/// What became of a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    /// Stored now: `204` with `X-Seq` of this number.
    Taken(u64),
    /// Stored before: the same `204` with `X-Seq` again (invariant 6).
    Again(u64),
    /// No room: the refusal `full`, and nothing is acknowledged (invariant 7).
    Full,
    /// Not the next number, or a batch after the last: the next number
    /// (`409`).
    Conflict(u64),
    /// The last batch says another total than the bytes stored with it: the
    /// session fails (invariant 9).
    Mismatch { said: u64, stored: u64 },
}

/// The batches up of one session that the target has not read, in number
/// order.
#[derive(Debug)]
pub struct Up {
    /// The number of the next batch to store.
    next: u64,
    /// The bytes of each batch stored.
    bytes: u64,
    queue: VecDeque<Vec<u8>>,
    /// The most batches that wait for the target.
    room: usize,
    /// The last batch is stored.
    fin: bool,
}

impl Up {
    /// A window of `room` batches, one at least.
    pub fn new(room: usize) -> Up {
        Up { next: 0, bytes: 0, queue: VecDeque::new(), room: room.max(1), fin: false }
    }

    /// Store the batch `seq`, the last when `total` is given: the bytes up
    /// of the whole session, which the bytes stored must equal.
    pub fn store(&mut self, seq: u64, batch: Vec<u8>, total: Option<u64>) -> Stored {
        // A number stored before: the same answer again, so a client whose
        // acknowledgement was lost stops at once.
        if seq < self.next {
            return Stored::Again(seq);
        }
        if seq > self.next || self.fin {
            return Stored::Conflict(self.next);
        }
        if self.queue.len() >= self.room {
            return Stored::Full;
        }
        let stored = self.bytes + batch.len() as u64;
        if let Some(said) = total.filter(|said| *said != stored) {
            return Stored::Mismatch { said, stored };
        }
        self.bytes = stored;
        self.queue.push_back(batch);
        self.next += 1;
        self.fin = total.is_some();
        Stored::Taken(seq)
    }

    /// The next batch for the target, in number order.
    pub fn take(&mut self) -> Option<Vec<u8>> {
        self.queue.pop_front()
    }

    /// The number of the next batch to store.
    pub fn next(&self) -> u64 {
        self.next
    }

    /// The bytes stored so far.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Whether the last batch is stored.
    pub fn fin(&self) -> bool {
        self.fin
    }

    /// Whether the last batch is stored and the target read each batch.
    pub fn done(&self) -> bool {
        self.fin && self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_sent_again_gets_the_same_acknowledgement_and_is_stored_once() {
        let mut up = Up::new(4);
        assert_eq!(up.store(0, b"ab".to_vec(), None), Stored::Taken(0));
        assert_eq!(up.store(0, b"ab".to_vec(), None), Stored::Again(0));
        assert_eq!(up.store(1, b"cd".to_vec(), None), Stored::Taken(1));
        assert_eq!(up.store(0, b"ab".to_vec(), None), Stored::Again(0));
        assert_eq!((up.next(), up.bytes()), (2, 4));
        assert_eq!(up.take(), Some(b"ab".to_vec()));
        assert_eq!(up.take(), Some(b"cd".to_vec()));
        assert_eq!(up.take(), None);
        assert_eq!(up.store(1, b"cd".to_vec(), None), Stored::Again(1), "a batch read by the target too");
        assert_eq!(up.store(3, b"ef".to_vec(), None), Stored::Conflict(2));
    }

    #[test]
    fn a_full_window_refuses_and_does_not_move() {
        let mut up = Up::new(1);
        assert_eq!(up.store(0, b"a".to_vec(), None), Stored::Taken(0));
        assert_eq!(up.store(1, b"b".to_vec(), None), Stored::Full);
        assert_eq!(up.store(1, b"b".to_vec(), None), Stored::Full);
        assert_eq!((up.next(), up.bytes()), (1, 1));
        up.take();
        assert_eq!(up.store(1, b"b".to_vec(), None), Stored::Taken(1));
    }

    #[test]
    fn the_last_batch_s_total_is_held_to_the_bytes_stored() {
        let mut up = Up::new(4);
        assert_eq!(up.store(0, b"abc".to_vec(), None), Stored::Taken(0));
        assert_eq!(up.store(1, b"de".to_vec(), Some(6)), Stored::Mismatch { said: 6, stored: 5 });
        assert!(!up.fin(), "a mismatch stores nothing");
        assert_eq!(up.store(1, b"de".to_vec(), Some(5)), Stored::Taken(1));
        assert!(up.fin() && !up.done());
        assert_eq!(up.store(1, b"de".to_vec(), Some(5)), Stored::Again(1));
        assert_eq!(up.store(2, b"f".to_vec(), None), Stored::Conflict(2), "no batch after the last");
        up.take();
        up.take();
        assert!(up.done());
        let mut empty = Up::new(1);
        assert_eq!(empty.store(0, Vec::new(), Some(0)), Stored::Taken(0));
    }

    #[test]
    fn each_reason_has_its_word_and_its_status() {
        for reason in [Refusal::Full, Refusal::UnknownSession, Refusal::NotAllowed] {
            assert_eq!(Refusal::parse(reason.word()), Some(reason));
        }
        assert_eq!(Refusal::parse(" full "), Some(Refusal::Full));
        assert_eq!(Refusal::parse("busy"), None);
        let statuses: Vec<u16> =
            [Refusal::Full, Refusal::UnknownSession, Refusal::NotAllowed].iter().map(|r| r.status()).collect();
        assert_eq!(statuses, [503, 404, 403]);
    }
}
