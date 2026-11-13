//! The opening side of the records: the check of the header, the replay
//! window, the key of the record's epoch, and the RESUME record.

use std::time::Instant;

use super::header::{HEADER, Header, PHASE};
use super::keys::{nonce, same};
use super::{Fault, MAX_JUMP, Opened, RESUME, RESUME_BODY, Record, Records, WINDOW};
use crate::TAG;

/// Which key of the direction opens a record.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Current,
    Previous,
    Next,
}

impl Records {
    /// Open `record`, a whole record of the peer, at `now`. A record that
    /// fails leaves the layer as it was, but for the count of repeats and
    /// of failed RESUME records.
    pub fn open(&mut self, now: Instant, record: &[u8]) -> Result<Opened, Fault> {
        if self.inn.ended {
            return Err(Fault::ResumeExpired);
        }
        let header = Header::parse(record)?;
        let length = header.length as usize;
        if record.len() != HEADER + length + TAG {
            return Err(Fault::Malformed(format!(
                "a record of {} bytes, where its header says {length}",
                record.len()
            )));
        }
        if header.kind == RESUME {
            return self.open_resume(now, &header, record);
        }
        let inn = &mut self.inn;
        if let Err(fault) = inn.window.check(header.counter) {
            if matches!(fault, Fault::Replayed(_)) {
                inn.replayed += 1;
            }
            return Err(fault);
        }
        let which = match (header.flags & PHASE != 0) == inn.epoch.phase() {
            true => Which::Current,
            // The epochs of a direction are in the order of its counters: a
            // record of the other phase below the first of this epoch is of
            // the one before, and above it of the next.
            false if inn.previous.is_some() && header.counter < inn.first => Which::Previous,
            false => Which::Next,
        };
        let epoch = match which {
            Which::Current => &inn.epoch,
            Which::Previous => inn.previous.as_ref().unwrap_or(&inn.epoch),
            Which::Next => &inn.next,
        };
        let (aad, rest) = record.split_at(HEADER);
        let mut body = rest[..length].to_vec();
        let tag: [u8; TAG] = rest[length..].try_into().map_err(|_| Fault::Tampered)?;
        if !epoch.cipher.open(&nonce(inn.prefix, header.counter), aad, &mut body, &tag) {
            return Err(Fault::Tampered);
        }
        inn.window.mark(header.counter);
        inn.heard = now;
        if which == Which::Next {
            let next = inn.next.next(self.suite);
            let current = std::mem::replace(&mut inn.epoch, std::mem::replace(&mut inn.next, next));
            inn.previous = Some(current);
            inn.first = header.counter;
        }
        // The key before goes once no record of it can open: each is below
        // the first of this epoch, and the window passed it.
        if inn.window.highest().is_some_and(|h| h >= inn.first.saturating_add(WINDOW)) {
            inn.previous = None;
        }
        body.truncate(length - usize::from(header.pad));
        Ok(Opened::Record(Record {
            kind: header.kind,
            flags: header.flags & !PHASE,
            stream: header.stream,
            counter: header.counter,
            payload: body,
        }))
    }

    /// The count of the records that came again inside the window.
    pub fn replayed(&self) -> u64 {
        self.inn.replayed
    }

    /// The epoch of the key that this end seals with, and of the one that it
    /// opens with.
    pub fn epochs(&self) -> (u64, u64) {
        (self.out.epoch.number, self.inn.epoch.number)
    }

    /// The counter of the next record that this end seals, and of the next
    /// one that it waits for.
    pub fn counters(&self) -> (u64, u64) {
        (self.out.counter, self.inn.window.next())
    }

    /// A RESUME of the peer: its resume key, a number not used yet, its
    /// token, within the window after the last record that opened. Each
    /// failure counts; the last of [`super::Settings::resume_failures`], or
    /// a resume too late, ends the layer.
    fn open_resume(&mut self, now: Instant, header: &Header, record: &[u8]) -> Result<Opened, Fault> {
        let (suite, limit, window) = (self.suite, self.settings.resume_failures, self.settings.resume_window);
        let inn = &mut self.inn;
        let fail = |inn: &mut super::Receiving, why: &str| {
            inn.failures += 1;
            if inn.failures >= limit {
                inn.ended = true;
                return Fault::ResumeExpired;
            }
            Fault::Resume(why.into())
        };
        if header.length as usize != RESUME_BODY || header.pad != 0 || header.flags != 0 || header.stream != 0 {
            return Err(fail(inn, "a RESUME record of another form"));
        }
        let index = header.counter;
        if index < inn.resume {
            return Err(fail(inn, "a RESUME record used already"));
        }
        let (aad, rest) = record.split_at(HEADER);
        let mut body = rest[..RESUME_BODY].to_vec();
        let tag: [u8; TAG] = rest[RESUME_BODY..].try_into().map_err(|_| Fault::Tampered)?;
        if !inn.start.resume.open(&nonce([0; 4], index), aad, &mut body, &tag) {
            return Err(fail(inn, "a RESUME record that failed its check"));
        }
        if !same(&body[..32], &inn.start.token(index)[..]) {
            return Err(fail(inn, "a RESUME record with another token"));
        }
        if now.saturating_duration_since(inn.heard) > window {
            inn.ended = true;
            return Err(Fault::ResumeExpired);
        }
        let word = |at: usize| u64::from_be_bytes(body[at..at + 8].try_into().unwrap_or_default());
        let (next, epoch) = (word(32), word(44));
        let ahead = epoch.wrapping_sub(inn.epoch.number);
        if epoch < inn.epoch.number || ahead > MAX_JUMP {
            return Err(fail(inn, "a RESUME record of an epoch that this end cannot take"));
        }
        // The records of the epochs between went with the lost carrier.
        if ahead > 0 {
            let mut at = inn.epoch.next(suite);
            for _ in 1..ahead {
                at = at.next(suite);
            }
            inn.next = at.next(suite);
            inn.epoch = at;
            inn.previous = None;
            inn.first = inn.window.next();
        }
        inn.prefix.copy_from_slice(&body[40..44]);
        inn.resume = index + 1;
        inn.failures = 0;
        inn.heard = now;
        Ok(Opened::Resume { next })
    }
}
