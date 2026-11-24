//! The verdict that a receiver keeps after `KEPT`: the pty can lose that
//! frame, and the sender then sends `DONE` again to an end whose session
//! ended. A `DONE` frame has no id, so its size and its SHA-256 name the
//! file.

use std::time::Instant;

use super::LINGER;
use super::reader::{Got, Reader};
use super::wire::Msg;

#[derive(Debug, Clone)]
pub(super) struct Verdict {
    size: u64,
    sha256: [u8; 32],
    /// The bytes of the `KEPT` frame, sent again as they went.
    kept: Vec<u8>,
    /// The end of the linger: [`LINGER`] after the hand-over of `KEPT`.
    until: Option<Instant>,
    /// The far end's reader of its input after the end.
    reader: Reader,
}

impl Verdict {
    pub(super) fn new(size: u64, sha256: [u8; 32], kept: Vec<u8>) -> Verdict {
        Verdict { size, sha256, kept, until: None, reader: Reader::new() }
    }

    /// Whether a frame is the `DONE` of the kept file.
    pub(super) fn answers(&self, kind: &[u8; 4], payload: &[u8]) -> bool {
        kind == b"DONE" && Msg::decode(kind, payload) == Ok(Msg::Done { size: self.size, sha256: self.sha256 })
    }

    pub(super) fn kept(&self) -> &[u8] {
        &self.kept
    }

    /// The bytes of `KEPT` went to the peer: the linger runs from the first
    /// hand-over, as each wait of the codec does.
    pub(super) fn handed(&mut self, now: Instant) {
        self.until.get_or_insert(now + LINGER);
    }

    pub(super) fn until(&self) -> Option<Instant> {
        self.until
    }

    pub(super) fn over(&self, now: Instant) -> bool {
        self.until.is_some_and(|u| now >= u)
    }

    /// The far end's input after the end: a `DONE` of the kept file gets
    /// the same `KEPT` in `out`. Each other frame and byte is dropped, as
    /// the far program is a receiver that ended.
    pub(super) fn feed(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        for &b in bytes {
            if let Some(Got::Frame { kind, payload }) = self.reader.push(b)
                && self.answers(&kind, &payload)
            {
                out.extend_from_slice(&self.kept);
            }
        }
    }
}
