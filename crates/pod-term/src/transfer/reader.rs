//! The reader of frames in the output of a pty that repaints them.
//!
//! First it drops what a terminal does not paint as text: each escape
//! sequence and string (read with the states of [`crate::grammar`]), each
//! UTF-8 character, and each control byte but CR and LF. Then it frames:
//! bytes are junk until `#`, 4 uppercase letters and `:`; CR and LF inside
//! a frame are dropped, so a wrapped frame is one frame; a frame is whole
//! at the eighth hex digit of its CRC. A byte that does not belong damages
//! the frame, and when that byte is `#` a new frame starts at it.

use std::collections::VecDeque;

use base64::Engine;

use super::wire::{BASE64, frame_crc};
use super::{DAMAGED_MAX, FRAME_MAX, JUNK_MAX};
use crate::grammar::{self, State};

const CAN: u8 = 0x18;
/// The CAN bytes in a row from the peer that end the session.
const CANS: u8 = 5;
/// The last junk bytes that a reason of `STOP` names.
const JUNK_SHOWN: usize = 32;

/// What the reader found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Got {
    /// A frame whose base64 and CRC hold.
    Frame { kind: [u8; 4], payload: Vec<u8> },
    /// A frame that was dropped.
    Damaged,
    /// More than [`DAMAGED_MAX`] damaged frames in a row.
    TooDamaged,
    /// A frame longer than [`FRAME_MAX`].
    TooLong,
    /// More than [`JUNK_MAX`] bytes with no whole frame; the last of them.
    Junk(Vec<u8>),
    /// Five CAN in a row from the peer.
    Cancelled,
}

/// Where the framer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Outside,
    /// `#` and this many letters of the kind.
    Head(usize),
    Body,
    /// `!` and this many hex digits.
    Crc(usize),
}

#[derive(Debug, Clone)]
pub(super) struct Reader {
    state: State,
    cans: u8,
    part: Part,
    kind: [u8; 4],
    text: Vec<u8>,
    crc: u32,
    /// The bytes of the frame from `#`, CR and LF not counted.
    len: usize,
    junk: usize,
    last_junk: VecDeque<u8>,
    damaged: usize,
}

impl Reader {
    pub(super) fn new() -> Self {
        Reader {
            state: State::Ground,
            cans: 0,
            part: Part::Outside,
            kind: [0; 4],
            text: Vec::new(),
            crc: 0,
            len: 0,
            junk: 0,
            last_junk: VecDeque::new(),
            damaged: 0,
        }
    }

    /// One byte of the peer's output.
    pub(super) fn push(&mut self, b: u8) -> Option<Got> {
        if b == CAN {
            self.cans += 1;
            if self.cans == CANS {
                return Some(Got::Cancelled);
            }
        } else {
            self.cans = 0;
        }
        let old = self.state;
        self.state = grammar::next(old, b);
        let painted = old == State::Ground && self.state == State::Ground;
        if painted && ((0x20..=0x7e).contains(&b) || b == b'\r' || b == b'\n') { self.frame(b) } else { None }
    }

    fn frame(&mut self, b: u8) -> Option<Got> {
        let line_end = b == b'\r' || b == b'\n';
        match self.part {
            Part::Outside if b == b'#' => self.start(),
            Part::Outside => return self.junk(&[b]),
            Part::Head(_) | Part::Body | Part::Crc(_) if line_end => return None,
            Part::Head(n) if n < 4 && b.is_ascii_uppercase() => {
                self.kind[n] = b;
                self.part = Part::Head(n + 1);
                self.len += 1;
            }
            Part::Head(4) if b == b':' => {
                self.part = Part::Body;
                self.len += 1;
            }
            Part::Head(n) => {
                // Not a frame: the head was junk.
                let mut head = vec![b'#'];
                head.extend_from_slice(&self.kind[..n]);
                self.part = Part::Outside;
                if b == b'#' {
                    self.start();
                } else {
                    head.push(b);
                }
                return self.junk(&head);
            }
            Part::Body if b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=' => {
                self.text.push(b);
                self.len += 1;
            }
            Part::Body if b == b'!' => {
                self.part = Part::Crc(0);
                self.crc = 0;
                self.len += 1;
            }
            Part::Crc(n) if b.is_ascii_hexdigit() => {
                self.crc = self.crc << 4 | (b as char).to_digit(16).unwrap_or(0);
                self.len += 1;
                if n + 1 == 8 {
                    return Some(self.whole());
                }
                self.part = Part::Crc(n + 1);
            }
            Part::Body | Part::Crc(_) => {
                self.part = Part::Outside;
                if b == b'#' {
                    self.start();
                }
                return Some(self.damage());
            }
        }
        (self.len > FRAME_MAX).then_some(Got::TooLong)
    }

    fn start(&mut self) {
        self.part = Part::Head(0);
        self.text.clear();
        self.len = 1;
    }

    fn junk(&mut self, bytes: &[u8]) -> Option<Got> {
        self.junk += bytes.len();
        for &b in bytes {
            if self.last_junk.len() == JUNK_SHOWN {
                self.last_junk.pop_front();
            }
            self.last_junk.push_back(b);
        }
        (self.junk > JUNK_MAX).then(|| Got::Junk(self.last_junk.iter().copied().collect()))
    }

    fn damage(&mut self) -> Got {
        self.damaged += 1;
        if self.damaged > DAMAGED_MAX { Got::TooDamaged } else { Got::Damaged }
    }

    /// The eighth hex digit of the CRC.
    fn whole(&mut self) -> Got {
        self.part = Part::Outside;
        let Ok(payload) = BASE64.decode(&self.text) else { return self.damage() };
        if frame_crc(&self.kind, &payload) != self.crc {
            return self.damage();
        }
        self.junk = 0;
        self.last_junk.clear();
        self.damaged = 0;
        Got::Frame { kind: self.kind, payload }
    }
}
