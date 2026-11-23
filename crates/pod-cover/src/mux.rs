//! One cover page load and one session over one connection, with one volume
//! budget (MUX-1 and MUX-2).
//!
//! The caller pushes frames, each tagged cover or session, and asks for the
//! next bytes to write. The multiplexer gives each cover frame before a
//! session frame, so the page completes first and a tunnel never starves the
//! cover site. Both lanes spend the same budget. A frame that does not fit
//! is cut at the budget, and the rest waits.
//!
//! The state machine has no socket and no clock: the caller writes the bytes
//! that [`Mux::next`] gives, in the order that it gives them.

use std::collections::VecDeque;

/// The lane of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// The page load of the cover site.
    Cover,
    /// The session of the client.
    Session,
}

/// A frame that the caller pushes, tagged with its lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub lane: Lane,
    pub bytes: Vec<u8>,
}

impl Frame {
    pub fn cover(bytes: impl Into<Vec<u8>>) -> Frame {
        Frame { lane: Lane::Cover, bytes: bytes.into() }
    }

    pub fn session(bytes: impl Into<Vec<u8>>) -> Frame {
        Frame { lane: Lane::Session, bytes: bytes.into() }
    }
}

/// The bytes to write next, and their lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub lane: Lane,
    pub bytes: Vec<u8>,
}

/// The schedule of two lanes over one budget.
#[derive(Clone, Debug)]
pub struct Mux {
    budget: u64,
    used: u64,
    cover: VecDeque<Vec<u8>>,
    session: VecDeque<Vec<u8>>,
}

impl Mux {
    /// A multiplexer with `budget` bytes for both lanes.
    pub fn new(budget: u64) -> Mux {
        Mux { budget, used: 0, cover: VecDeque::new(), session: VecDeque::new() }
    }

    /// Add a frame. An empty frame is kept as a frame of no bytes and is
    /// passed over.
    pub fn push(&mut self, frame: Frame) {
        match frame.lane {
            Lane::Cover => self.cover.push_back(frame.bytes),
            Lane::Session => self.session.push_back(frame.bytes),
        }
    }

    /// The bytes that the budget still holds.
    pub fn remaining(&self) -> u64 {
        self.budget.saturating_sub(self.used)
    }

    /// The bytes that the budget spent.
    pub fn used(&self) -> u64 {
        self.used
    }

    /// The next bytes to write, at most `limit` of them. A cover frame comes
    /// before a session frame (MUX-2). `None` when the budget is spent or
    /// both lanes are empty.
    pub fn next(&mut self, limit: usize) -> Option<Chunk> {
        while self.cover.front().is_some_and(|bytes| bytes.is_empty()) {
            self.cover.pop_front();
        }
        if self.cover.is_empty() {
            while self.session.front().is_some_and(|bytes| bytes.is_empty()) {
                self.session.pop_front();
            }
        }
        let lane = if self.cover.is_empty() { Lane::Session } else { Lane::Cover };
        let allowed = self.remaining().min(limit as u64) as usize;
        if allowed == 0 {
            return None;
        }
        let queue = match lane {
            Lane::Cover => &mut self.cover,
            Lane::Session => &mut self.session,
        };
        let frame = queue.front_mut()?;
        let take = allowed.min(frame.len());
        if take == 0 {
            return None;
        }
        let bytes: Vec<u8> = frame.drain(..take).collect();
        if frame.is_empty() {
            queue.pop_front();
        }
        self.used = self.used.saturating_add(take as u64);
        Some(Chunk { lane, bytes })
    }
}
