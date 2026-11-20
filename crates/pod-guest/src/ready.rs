//! The wait for a guest that is ready, in place of a fixed sleep.
//!
//! Four signs, each one read from the host with no help from the guest
//! but the one it gives:
//!
//! * [`Sign::Marker`]: the agent wrote [`crate::agent::READY`] to the
//!   mailbox (a provisioned image).
//! * [`Sign::Serial`]: the serial log of the plan holds a text.
//! * [`Sign::Screen`]: the screen changed after the first dump of QMP
//!   `screendump`, then stayed the same for `settle` but for a cursor (an
//!   image with no agent yet: the shell of a provision boot is at its
//!   prompt).
//! * [`Sign::After`]: a fixed time passed since the start of the wait.
//!
//! [`wait`] polls each sign until one holds, the emulator exits, or the
//! deadline comes; each end but the first is a failure with the cause
//! [`Unready`].

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

use pod_core::{Class, Failure, Stage};

use crate::{Fat16, Plan, Result, agent, io_at};

mod screen;

pub use screen::CURSOR;
use screen::Screen;

/// The time that a screen must stay the same after it changed.
pub const SETTLE: Duration = Duration::from_secs(5);
/// The time between two polls.
pub const EVERY: Duration = Duration::from_millis(500);
/// The most bytes of the serial log that one poll reads.
const SERIAL_STEP: u64 = 1 << 20;

/// One sign of a ready guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sign {
    /// The agent wrote `WQREADY.TXT` to the mailbox of the plan.
    Marker,
    /// The serial log of the plan ([`Plan::serial`]) holds this text.
    Serial(String),
    /// The screen changed after the first dump, then stayed the same for
    /// `settle`. A change in a box of [`CURSOR`] (a cursor that blinks, the
    /// pointer) is no change. The settled screen must differ from the first
    /// dump, and must show more than one colour outside such a box, so that
    /// a black boot phase is not a ready guest.
    Screen { settle: Duration },
    /// This time passed since the start of [`wait`]: the fixed wait for a
    /// guest that gives no other sign. The provision starts the wait right
    /// after the spawn of the emulator. A time past the deadline never
    /// holds.
    After(Duration),
}

impl std::fmt::Display for Sign {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Sign::Marker => write!(f, "the marker {} of the agent", agent::READY),
            Sign::Serial(text) => write!(f, "the text {text:?} on the serial line"),
            Sign::Screen { settle } => write!(
                f,
                "a screen with content that changed and then stayed the same for {settle:?}, but for a box of {}x{} pixels",
                CURSOR.0, CURSOR.1
            ),
            Sign::After(time) => write!(f, "the end of {time:?} after the start of the wait"),
        }
    }
}

/// What [`wait`] waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    /// The signs; the first one that holds ends the wait.
    pub signs: Vec<Sign>,
    /// The limit of the whole wait.
    pub deadline: Duration,
    /// The time between two polls.
    pub every: Duration,
}

impl Ready {
    /// A wait for a screen that settles ([`SETTLE`]), polled each
    /// [`EVERY`]: the readiness of a provision boot.
    pub fn screen(deadline: Duration) -> Ready {
        Ready { signs: vec![Sign::Screen { settle: SETTLE }], deadline, every: EVERY }
    }
}

/// Why [`wait`] ended with no ready guest: the cause of its failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unready {
    /// No sign held within the deadline.
    Deadline { after: Duration, signs: Vec<Sign> },
    /// The emulator exited first.
    Exited { status: String },
}

impl std::fmt::Display for Unready {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unready::Deadline { after, signs } => {
                let signs: Vec<String> = signs.iter().map(Sign::to_string).collect();
                write!(f, "the guest was not ready within {}s; no sign held: {}", after.as_secs(), signs.join("; "))
            }
            Unready::Exited { status } => write!(f, "the emulator exited ({status}) before the guest was ready"),
        }
    }
}

impl std::error::Error for Unready {}

impl Unready {
    /// [`Class::Timeout`] for the deadline, [`Class::Unavailable`] for an
    /// emulator that exited.
    pub fn failure(self) -> Failure {
        let class = match self {
            Unready::Deadline { .. } => Class::Timeout,
            Unready::Exited { .. } => Class::Unavailable,
        };
        Failure::caused(Stage::Host, class, self.to_string(), self)
    }
}

/// The [`Unready`] in the chain of `f`.
pub fn unready(f: &Failure) -> Option<&Unready> {
    crate::cause_of::<Unready>(f)
}

/// Poll the signs of `ready` for the guest of `plan`, whose emulator is
/// `child`, and return the index of the first sign that held.
///
/// The emulator that exits first, and the deadline, are failures with the
/// cause [`Unready`]. The wait does not stop the emulator: the caller does.
/// A sign that cannot be read yet (no QMP socket, no serial log) does not
/// hold; it is no failure.
pub fn wait(plan: &Plan, child: &mut Child, ready: &Ready) -> Result<usize> {
    let start = Instant::now();
    let mut watches: Vec<Watch> = ready.signs.iter().map(Watch::new).collect();
    loop {
        for (i, w) in watches.iter_mut().enumerate() {
            if w.holds(plan) {
                return Ok(i);
            }
        }
        if let Some(status) = child.try_wait().map_err(|e| io_at(&plan.emulator, e))? {
            return Err(Unready::Exited { status: status.to_string() }.failure());
        }
        let spent = start.elapsed();
        if spent >= ready.deadline {
            return Err(Unready::Deadline { after: ready.deadline, signs: ready.signs.clone() }.failure());
        }
        std::thread::sleep(ready.every.min(ready.deadline - spent));
    }
}

/// The state of the poll of one sign.
enum Watch {
    Marker,
    Serial(Scan),
    Screen(Box<Screen>),
    /// The time at which the sign holds.
    After(Instant),
}

impl Watch {
    fn new(sign: &Sign) -> Watch {
        match sign {
            Sign::Marker => Watch::Marker,
            Sign::Serial(text) => Watch::Serial(Scan::new(text.as_bytes())),
            Sign::Screen { settle } => Watch::Screen(Box::new(Screen::new(*settle))),
            Sign::After(time) => Watch::After(Instant::now() + *time),
        }
    }

    fn holds(&mut self, plan: &Plan) -> bool {
        match self {
            Watch::Marker => {
                std::fs::read(&plan.mailbox).ok().and_then(Fat16::from_image).is_some_and(|m| agent::ready(&m))
            }
            Watch::Serial(scan) => scan.poll(&plan.serial),
            Watch::Screen(screen) => screen.poll(&plan.monitor),
            Watch::After(at) => Instant::now() >= *at,
        }
    }
}

/// A search for a text in a file that grows: each poll reads the new
/// bytes, and keeps the end of the last ones, so that a text that two
/// polls cut is found.
pub struct Scan {
    text: Vec<u8>,
    offset: u64,
    carry: Vec<u8>,
}

impl Scan {
    pub fn new(text: &[u8]) -> Scan {
        Scan { text: text.to_vec(), offset: 0, carry: Vec::new() }
    }

    /// Read the new bytes of `path`, at most 1 MiB, and say if the text is
    /// in the bytes so far. A file that became shorter is read again from
    /// its start.
    pub fn poll(&mut self, path: &Path) -> bool {
        let Ok(mut file) = std::fs::File::open(path) else { return false };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            self.offset = 0;
            self.carry.clear();
        }
        let mut fresh = Vec::new();
        if file.seek(SeekFrom::Start(self.offset)).is_err() || file.take(SERIAL_STEP).read_to_end(&mut fresh).is_err() {
            return false;
        }
        self.offset += fresh.len() as u64;
        self.feed(&fresh)
    }

    /// Take `fresh` bytes after the ones before.
    pub fn feed(&mut self, fresh: &[u8]) -> bool {
        self.carry.extend_from_slice(fresh);
        if self.text.is_empty() || self.carry.windows(self.text.len()).any(|w| w == self.text) {
            return true;
        }
        let keep = self.text.len() - 1;
        if self.carry.len() > keep {
            self.carry.drain(..self.carry.len() - keep);
        }
        false
    }
}

#[cfg(test)]
mod tests;
