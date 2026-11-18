//! The steps of the client's session that its user hears of.

use std::time::Duration;

/// A step of the session across links, each worth one line to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// The link was lost; the session waits for a new one.
    Lost { why: String },
    /// An attempt failed; the next comes after `wait`.
    Retry { why: String, wait: Duration },
    /// A new link carries the session, `resent` bytes sent again.
    Resumed { resent: u64, after: Duration },
    /// Before the relay's limits, the session moved to a new link: the old
    /// one had carried `bytes` in `age`.
    Moved { bytes: u64, age: Duration, resent: u64 },
    /// A move failed; the old link goes on until it ends, then a resume.
    MoveFailed { why: String },
    /// The link said nothing for `quiet`: a spare link opens beside it.
    Spare { quiet: Duration },
    /// The spare link closed, as the old link spoke first, or it failed;
    /// the old link goes on.
    SpareClosed { why: String },
    /// A wait of this side's link held for [`crate::pump::STALL_AFTER`]: the
    /// application took no byte while bytes wait for it, or the reader of
    /// the application waits for room or for the writer of the link. The
    /// words name the wait and the offsets each way, as the far end's
    /// `session.stall` does; one note for each wait.
    Stall { words: String },
}
