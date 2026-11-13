//! The counters of one end of the ARQ.

/// What one end of the ARQ sent and took. Each count starts at 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Frames sent the first time (`DATA` and `FIN`).
    pub sent: u64,
    /// Frames sent again: after a timeout, or after an `ACK` that shows a
    /// gap.
    pub retransmitted: u64,
    /// The timeouts of frames in flight.
    pub timeouts: u64,
    /// `ACK` datagrams sent and taken.
    pub acks_sent: u64,
    pub acks_taken: u64,
    /// `PROBE` datagrams sent to a peer whose window is closed.
    pub probes: u64,
    /// Frames that came the first time, and again.
    pub received: u64,
    pub duplicates: u64,
    /// Datagrams dropped: of a form that the ARQ does not take, out of the
    /// window, or after the end.
    pub dropped: u64,
    /// Records sent and given whole to the reader.
    pub records_sent: u64,
    pub records_received: u64,
    /// The wraps of the send sequence to 0, each after a rekey.
    pub rekeys: u64,
}
