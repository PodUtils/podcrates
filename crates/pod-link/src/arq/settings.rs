//! The settings of one end of the ARQ, and their limits.

use std::fmt;
use std::time::Duration;

use super::wire::{MAX_FRAGMENT, MIN_FRAGMENT, SACK_BITS};
use crate::frame::MAX_RECORD;

/// The RTO floor of podnet's row E2-2: 200 ms.
pub const RTO_FLOOR: Duration = Duration::from_millis(200);

/// The RTO ceiling of podnet's row E2-2: 8 s.
pub const RTO_CEILING: Duration = Duration::from_secs(8);

/// The retransmissions of one frame before the ARQ ends (podnet: 8).
pub const RETRIES: u8 = 8;

/// The most retransmissions that a setting can give.
pub const MAX_RETRIES: u8 = 16;

/// The smallest receive buffer: a full window of the largest fragments.
pub const MIN_RECEIVE_BUFFER: usize = SACK_BITS * MAX_FRAGMENT;

/// What one end of the ARQ sends and keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArqSettings {
    /// The most frames in flight: sent, and not acknowledged. 1 to 256.
    pub window: u16,
    /// The largest payload of a fragment. The ARQ also keeps each datagram
    /// within the carrier's unit. 32 to 1024 bytes.
    pub fragment: usize,
    /// The largest record: a write of the byte stream is one record of
    /// this size or less, and a record has at most 65535 fragments.
    /// 1 KiB to 1 MiB.
    pub record: usize,
    /// The bytes that the writer can queue before it waits: the records
    /// that are not acknowledged. At least `record`.
    pub send_buffer: usize,
    /// The bytes in order that this end holds for its reader. The window
    /// that this end gives the peer shrinks as it fills. At least two
    /// records, and at least a full window of fragments (256 KiB): the
    /// peer sends a full window before its first `ACK`.
    pub receive_buffer: usize,
    /// The RTO before the first sample of the round trip.
    pub rto_initial: Duration,
    /// The smallest RTO.
    pub rto_floor: Duration,
    /// The largest RTO, also after the backoff.
    pub rto_ceiling: Duration,
    /// The retransmissions of one frame. When the last one has no
    /// acknowledgement within its RTO, the ARQ ends (`arq-timeout`).
    /// 1 to 16.
    pub retries: u8,
    /// The sequence of the first frame of each direction. The two ends
    /// must give the same value.
    pub start: u32,
    /// The frames before the wrap of the sequence to 0 at which the ARQ
    /// asks the caller for a rekey. At least `window`.
    pub rekey_ahead: u32,
}

impl Default for ArqSettings {
    /// podnet's values: 256 frames in flight, fragments of 1 KiB, an RTO of
    /// 200 ms to 8 s (1 s before the first sample), 8 retries. Records of
    /// 64 KiB, 1 MiB to send and 4 MiB to receive, the first sequence 0, a
    /// rekey asked 2^20 frames before the wrap.
    fn default() -> ArqSettings {
        ArqSettings {
            window: SACK_BITS as u16,
            fragment: MAX_FRAGMENT,
            record: 64 << 10,
            send_buffer: 1 << 20,
            receive_buffer: 4 << 20,
            rto_initial: Duration::from_secs(1),
            rto_floor: RTO_FLOOR,
            rto_ceiling: RTO_CEILING,
            retries: RETRIES,
            start: 0,
            rekey_ahead: 1 << 20,
        }
    }
}

impl ArqSettings {
    /// Each value against its limits.
    pub fn check(&self) -> Result<(), ArqSettingsError> {
        use ArqSettingsError as E;
        if !(1..=SACK_BITS as u16).contains(&self.window) {
            return Err(E::Window(self.window));
        }
        if !(MIN_FRAGMENT..=MAX_FRAGMENT).contains(&self.fragment) {
            return Err(E::Fragment(self.fragment));
        }
        if !(MAX_FRAGMENT..=MAX_RECORD).contains(&self.record) {
            return Err(E::Record(self.record));
        }
        if self.send_buffer < self.record {
            return Err(E::SendBuffer(self.send_buffer));
        }
        if self.receive_buffer < (2 * self.record).max(MIN_RECEIVE_BUFFER) {
            return Err(E::ReceiveBuffer(self.receive_buffer));
        }
        if self.rto_floor.is_zero()
            || self.rto_ceiling < self.rto_floor
            || !(self.rto_floor..=self.rto_ceiling).contains(&self.rto_initial)
        {
            return Err(E::Rto { initial: self.rto_initial, floor: self.rto_floor, ceiling: self.rto_ceiling });
        }
        if !(1..=MAX_RETRIES).contains(&self.retries) {
            return Err(E::Retries(self.retries));
        }
        if self.rekey_ahead < u32::from(self.window) {
            return Err(E::RekeyAhead(self.rekey_ahead));
        }
        Ok(())
    }
}

/// A setting of the ARQ out of its limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArqSettingsError {
    Window(u16),
    Fragment(usize),
    Record(usize),
    SendBuffer(usize),
    ReceiveBuffer(usize),
    Rto {
        initial: Duration,
        floor: Duration,
        ceiling: Duration,
    },
    Retries(u8),
    RekeyAhead(u32),
    /// The carrier's unit holds no fragment of the smallest size.
    Mtu(usize),
}

impl fmt::Display for ArqSettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ArqSettingsError as E;
        match self {
            E::Window(v) => write!(f, "a window of {v} frames: give 1 to {SACK_BITS}"),
            E::Fragment(v) => write!(f, "a fragment of {v} bytes: give {MIN_FRAGMENT} to {MAX_FRAGMENT}"),
            E::Record(v) => write!(f, "a record of {v} bytes: give {MAX_FRAGMENT} to {MAX_RECORD}"),
            E::SendBuffer(v) => write!(f, "a send buffer of {v} bytes: give one record or more"),
            E::ReceiveBuffer(v) => {
                write!(f, "a receive buffer of {v} bytes: give two records or more, and {MIN_RECEIVE_BUFFER} or more")
            }
            E::Rto { initial, floor, ceiling } => write!(
                f,
                "an RTO of {initial:?} with a floor of {floor:?} and a ceiling of {ceiling:?}: the floor is more than 0, and the first RTO is from the floor to the ceiling"
            ),
            E::Retries(v) => write!(f, "{v} retries: give 1 to {MAX_RETRIES}"),
            E::RekeyAhead(v) => write!(f, "a rekey {v} frames before the wrap: give the window or more"),
            E::Mtu(v) => write!(f, "a carrier unit of {v} bytes holds no fragment of {MIN_FRAGMENT} bytes"),
        }
    }
}

impl std::error::Error for ArqSettingsError {}
