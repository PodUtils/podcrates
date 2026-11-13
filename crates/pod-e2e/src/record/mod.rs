//! The record layer: records sealed with keys that only the two ends of a
//! channel hold, so that each hop between them (a relay, a peer of a mesh)
//! carries ciphertext only. It is sans-IO: the caller carries the bytes,
//! and gives the time.
//!
//! A record is a header of [`HEADER`] bytes (big-endian: `version` u8,
//! `type` u8, `flags` u16, `stream` u32, `counter` u64, `length` u32, `pad`
//! u16, `reserved` u16), then a body of `length` bytes (the payload, then
//! `pad` zero bytes), sealed, then the tag of [`TAG`] bytes. The header is
//! the associated data. The nonce is the 4-byte prefix of the sender's
//! direction, then the counter.
//!
//! - The suite is one that the two ends agree on after the handshake
//!   ([`crate::Channel::agree`], [`agree`]); its name is in the context of
//!   each key, so that two ends of two suites share no key.
//! - The counter of each direction starts at 0 and never goes back. The
//!   sender takes a new key after [`Settings::rekey_records`] records,
//!   [`Settings::rekey_bytes`] bytes or [`Settings::rekey_age`], and flips
//!   the bit [`PHASE`] of the flags; the receiver keeps the key before it
//!   while a late record of it can still open.
//! - A body pads to the next of [`BUCKETS`] when [`Settings::pad`] is on.
//! - The receiver refuses a record at or below its highest counter less
//!   [`WINDOW`], and counts and refuses a repeat inside the window.
//! - After a lost carrier, each end sends a RESUME record ([`Records::resume`]):
//!   its token used once, the counter that it waits for next, a fresh prefix
//!   for its records after it, and the epoch of its key. The body is the
//!   token (32 bytes), the counter (u64), the prefix (4 bytes) and the epoch
//!   (u64); the header has the type [`RESUME`], the number of the RESUME as
//!   its counter, and no flags, stream or padding. The counters never go
//!   back: the caller sends again, in new records, the payloads from the
//!   counter that the peer waits for. A record sealed before a RESUME that
//!   comes after it does not open.

mod header;
mod keys;
#[cfg(test)]
mod tests;
mod window;

use std::fmt;
use std::time::{Duration, Instant};

pub use header::{BUCKETS, HEADER, Header, MAX_LENGTH, PHASE, VERSION, bucket};
pub use window::WINDOW;

use crate::TAG;
use crate::exporter::Exporter;
use keys::{Epoch, Start};
use window::Window;

/// The type of the record that the layer keeps for itself.
pub const RESUME: u8 = 0xff;
/// The bytes of the body of a RESUME record: the token, the next counter,
/// the fresh prefix and the sender's epoch.
const RESUME_BODY: usize = 32 + 8 + 4 + 8;
/// The most epochs that one RESUME moves the receiver on: each is a step of
/// the schedule.
const MAX_JUMP: u64 = 1 << 16;

/// A suite of the records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Suite {
    /// The default.
    ChaCha20Poly1305,
    Aes256Gcm,
}

impl Suite {
    /// Each suite, the default first.
    pub const ALL: [Suite; 2] = [Suite::ChaCha20Poly1305, Suite::Aes256Gcm];

    /// Its name, in the context of its keys.
    pub fn name(self) -> &'static str {
        match self {
            Suite::ChaCha20Poly1305 => "chacha20-poly1305",
            Suite::Aes256Gcm => "aes-256-gcm",
        }
    }

    /// Its byte in an offer.
    pub fn id(self) -> u8 {
        match self {
            Suite::ChaCha20Poly1305 => 1,
            Suite::Aes256Gcm => 2,
        }
    }

    /// The suite of `id`; none for an id that this build does not have.
    pub fn of_id(id: u8) -> Option<Suite> {
        Suite::ALL.into_iter().find(|s| s.id() == id)
    }
}

impl fmt::Display for Suite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The suite of two offers: the first of the initiator's that the responder
/// offers too; none when they share none.
pub fn agree(initiator: &[Suite], responder: &[Suite]) -> Option<Suite> {
    initiator.iter().copied().find(|s| responder.contains(s))
}

/// The settings of the record layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Pad each body to its bucket.
    pub pad: bool,
    /// The records of one key.
    pub rekey_records: u64,
    /// The bytes of the bodies of one key.
    pub rekey_bytes: u64,
    /// The age of one key.
    pub rekey_age: Duration,
    /// How long after the last record that opened a RESUME can come.
    pub resume_window: Duration,
    /// The failed RESUME records that end the layer.
    pub resume_failures: u32,
}

impl Settings {
    pub const DEFAULT: Settings = Settings {
        pad: false,
        rekey_records: 1 << 31,
        rekey_bytes: 1 << 30,
        rekey_age: Duration::from_secs(3600),
        resume_window: Duration::from_secs(600),
        resume_failures: 3,
    };
}

impl Default for Settings {
    fn default() -> Self {
        Settings::DEFAULT
    }
}

/// Why a record did not seal or open. Only [`Fault::ResumeExpired`] and
/// [`Fault::Exhausted`] end the layer; after another, the caller can drop
/// the record and go on, or end the link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// A header or a value that the layer does not have.
    Malformed(String),
    /// The record failed its check: changed on its way, or not of these
    /// keys.
    Tampered,
    /// A counter that opened already: counted ([`Records::replayed`]).
    Replayed(u64),
    /// A counter at or below the window.
    Stale(u64),
    /// A RESUME record that failed: why.
    Resume(String),
    /// The RESUME came too late, or too many failed: the layer is ended.
    ResumeExpired,
    /// The counter of this direction is at its end.
    Exhausted,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Malformed(why) => write!(f, "a record that the layer does not have: {why}"),
            Fault::Tampered => f.write_str("a record failed its check: changed on its way, or not of these keys"),
            Fault::Replayed(c) => write!(f, "the record {c} came again"),
            Fault::Stale(c) => write!(f, "the record {c} is below the replay window"),
            Fault::Resume(why) => write!(f, "a RESUME record failed: {why}"),
            Fault::ResumeExpired => f.write_str("the resume expired: no valid RESUME record came in time"),
            Fault::Exhausted => f.write_str("the counter of the records is at its end"),
        }
    }
}

impl std::error::Error for Fault {}

impl From<Fault> for pod_core::Failure {
    fn from(e: Fault) -> pod_core::Failure {
        use pod_core::{Class, Failure, Stage};
        let class = match e {
            Fault::ResumeExpired | Fault::Exhausted => Class::Unavailable,
            _ => Class::Protocol,
        };
        Failure::caused(Stage::Session, class, e.to_string(), e)
    }
}

/// A record that opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub kind: u8,
    /// The caller's flags: [`PHASE`] is cleared.
    pub flags: u16,
    pub stream: u32,
    pub counter: u64,
    pub payload: Vec<u8>,
}

/// What [`Records::open`] gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    Record(Record),
    /// The peer resumed: it did not get this end's records from the counter
    /// `next` on, which the caller sends again in new records.
    Resume {
        next: u64,
    },
}

/// The bytes of a whole record whose header starts `header`: what the caller
/// reads from a stream before [`Records::open`].
pub fn record_len(header: &[u8]) -> Result<usize, Fault> {
    Ok(HEADER + Header::parse(header)?.length as usize + TAG)
}

/// The records of one end of a channel: what it seals, and what it opens.
pub struct Records {
    suite: Suite,
    settings: Settings,
    out: Sending,
    inn: Receiving,
}

struct Sending {
    start: Start,
    epoch: Epoch,
    prefix: [u8; 4],
    counter: u64,
    /// The records and the bytes of the epoch, and when it started.
    records: u64,
    bytes: u64,
    since: Instant,
    /// The number of the next RESUME.
    resume: u64,
}

struct Receiving {
    start: Start,
    epoch: Epoch,
    /// The first counter that opened in the epoch.
    first: u64,
    next: Epoch,
    previous: Option<Epoch>,
    prefix: [u8; 4],
    window: Window,
    heard: Instant,
    /// The lowest number of a RESUME that can open.
    resume: u64,
    failures: u32,
    ended: bool,
    replayed: u64,
}

impl fmt::Debug for Records {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Records {{ suite: {}, sent: {}, next in: {} }}",
            self.suite,
            self.out.counter,
            self.inn.window.next()
        )
    }
}

impl Exporter {
    /// The records of `suite` with `settings` at this end, from `now`.
    pub fn records(&self, suite: Suite, settings: Settings, now: Instant) -> Result<Records, crate::Error> {
        let (out_epoch, mine) = Start::of(self, suite, self.is_initiator())?;
        let (in_epoch, theirs) = Start::of(self, suite, !self.is_initiator())?;
        let next = in_epoch.next(suite);
        let out = Sending {
            prefix: mine.prefix,
            start: mine,
            epoch: out_epoch,
            counter: 0,
            records: 0,
            bytes: 0,
            since: now,
            resume: 0,
        };
        let inn = Receiving {
            prefix: theirs.prefix,
            start: theirs,
            epoch: in_epoch,
            first: 0,
            next,
            previous: None,
            window: Window::default(),
            heard: now,
            resume: 0,
            failures: 0,
            ended: false,
            replayed: 0,
        };
        Ok(Records { suite, settings, out, inn })
    }
}

mod open;
mod seal;
