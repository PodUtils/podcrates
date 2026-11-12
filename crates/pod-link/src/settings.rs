//! The settings of one end of a link, and their limits.

use std::fmt;
use std::time::Duration;

use crate::frame::MAX_RECORD;

/// The credit that each end has for each new stream and for the link
/// before the first `WINDOW` frame: 16 KiB. It is part of the protocol.
pub const INITIAL_WINDOW: u32 = 16 << 10;

/// The smallest window that a setting can give: [`INITIAL_WINDOW`].
pub const MIN_WINDOW: u32 = INITIAL_WINDOW;

/// The largest window of a stream or of the link: 8 MiB.
pub const MAX_WINDOW: u32 = 8 << 20;

/// The most streams that one end lets a link hold.
pub const MAX_STREAMS: usize = 4096;

/// The smallest `DATA` body that a setting can give: 1 KiB.
pub const MIN_FRAME: usize = 1 << 10;

/// What one end of a link allows and sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// The largest receive window of a stream. A stream starts at
    /// [`INITIAL_WINDOW`], and its window doubles each time its reader
    /// empties the buffer when credit goes back, up to this value. A
    /// stream that stops reading keeps a small window. 16 KiB to 8 MiB.
    pub stream_window: u32,
    /// The receive window of the link: the most bytes of all streams that
    /// this end holds unread. It bounds the memory of a link. It is at
    /// least `stream_window`; a stream that stops reading holds at most
    /// its own window of it, and the other streams use the rest.
    /// 16 KiB to 8 MiB.
    pub link_window: u32,
    /// The most streams of the link at this end, from both ends: an
    /// [`crate::Link::open`] over it fails, and an `OPEN` of the peer over
    /// it gets a reset with [`crate::ResetCode::REFUSED`]. 1 to 4096.
    pub max_streams: usize,
    /// The largest `DATA` body that this end sends. A write is split into
    /// frames of this size or less. 1 KiB to [`MAX_RECORD`].
    pub frame_size: usize,
    /// How long the link is silent before this end sends a `PING`.
    pub keepalive: Duration,
    /// How long the link is silent before this end ends it as dead
    /// ([`crate::Error::Dead`]). Longer than `keepalive`.
    pub dead_after: Duration,
}

impl Default for Settings {
    /// 1 MiB for each stream, 8 MiB for the link, 256 streams, frames of
    /// 16 KiB, a `PING` after 10 s of silence, dead after 30 s.
    fn default() -> Settings {
        Settings {
            stream_window: 1 << 20,
            link_window: MAX_WINDOW,
            max_streams: 256,
            frame_size: 16 << 10,
            keepalive: Duration::from_secs(10),
            dead_after: Duration::from_secs(30),
        }
    }
}

impl Settings {
    /// Each value against its limits. [`crate::Link::new`] calls it.
    pub fn check(&self) -> Result<(), SettingsError> {
        let window = MIN_WINDOW..=MAX_WINDOW;
        if !window.contains(&self.stream_window) {
            return Err(SettingsError::StreamWindow(self.stream_window));
        }
        if !window.contains(&self.link_window) || self.link_window < self.stream_window {
            return Err(SettingsError::LinkWindow(self.link_window));
        }
        if !(1..=MAX_STREAMS).contains(&self.max_streams) {
            return Err(SettingsError::MaxStreams(self.max_streams));
        }
        if !(MIN_FRAME..=MAX_RECORD).contains(&self.frame_size) {
            return Err(SettingsError::FrameSize(self.frame_size));
        }
        if self.keepalive.is_zero() || self.dead_after <= self.keepalive {
            return Err(SettingsError::Keepalive { keepalive: self.keepalive, dead_after: self.dead_after });
        }
        Ok(())
    }
}

/// A setting out of its limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    StreamWindow(u32),
    LinkWindow(u32),
    MaxStreams(usize),
    FrameSize(usize),
    Keepalive { keepalive: Duration, dead_after: Duration },
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::StreamWindow(v) => {
                write!(f, "a stream window of {v} bytes: give {MIN_WINDOW} to {MAX_WINDOW}")
            }
            SettingsError::LinkWindow(v) => {
                write!(
                    f,
                    "a link window of {v} bytes: give {MIN_WINDOW} to {MAX_WINDOW}, and not less than the stream window"
                )
            }
            SettingsError::MaxStreams(v) => write!(f, "a limit of {v} streams: give 1 to {MAX_STREAMS}"),
            SettingsError::FrameSize(v) => write!(f, "a frame size of {v} bytes: give {MIN_FRAME} to {MAX_RECORD}"),
            SettingsError::Keepalive { keepalive, dead_after } => write!(
                f,
                "a keepalive of {keepalive:?} and a dead time of {dead_after:?}: the keepalive is more than 0, and the dead time is longer"
            ),
        }
    }
}

impl std::error::Error for SettingsError {}
