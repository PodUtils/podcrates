//! The sizes that a session takes in: a frame, and a message reassembled
//! from fragments. A setting of the caller: the relay's by default, or the
//! defaults of tokio-tungstenite, which podnet used.

use crate::frame::FORWARD_MAX_FRAME;

/// The largest message reassembled from fragments, by the relay's limits.
/// The relay's own frames are at most 262144 bytes; this leaves room and
/// still bounds memory.
pub const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// The largest frame and the largest message that a session takes in. A
/// larger frame closes the session with 1002, a larger message with 1009.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// A message reassembled from fragments.
    pub message: usize,
    /// One frame, as its header gives its length.
    pub frame: usize,
}

impl Limits {
    /// The relay's: a frame of the forward road ([`FORWARD_MAX_FRAME`]) and
    /// a message of 16 MiB.
    pub const RELAY: Limits = Limits { message: MAX_MESSAGE, frame: FORWARD_MAX_FRAME };

    /// The defaults of tokio-tungstenite 0.30: a message of 64 MiB, a frame
    /// of 16 MiB.
    pub const TUNGSTENITE: Limits = Limits { message: 64 << 20, frame: 16 << 20 };
}

impl Default for Limits {
    fn default() -> Limits {
        Limits::RELAY
    }
}
