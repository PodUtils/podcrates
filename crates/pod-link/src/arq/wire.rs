//! The datagrams of the ARQ and their codec, with no I/O.
//!
//! Each datagram is one of four forms. Each number is big-endian.
//!
//! ```text
//! DATA   | 1 | sequence u32 | index u16 | count u16 | payload, 1 to 1024 bytes |
//! FIN    | 2 | sequence u32 |
//! ACK    | 3 | next u32 | window u16 | bitmap, 32 bytes |
//! PROBE  | 4 |
//! ```
//!
//! A `DATA` frame is one fragment of a record: the fragment `index` of
//! `count`. Each fragment has its own sequence. `FIN` is the end of the
//! sender's bytes, in sequence after the last record. An `ACK` gives the
//! next sequence that the receiver waits for (each one before it came), the
//! frames that the receiver takes from `next` on (its window), and a bit for
//! each of the 256 frames from `next` on: bit `i` (byte `i / 8`, bit
//! `i % 8` from the low bit) is set when the frame `next + i` came. A
//! `PROBE` asks for an `ACK`.

use std::fmt;

/// The type byte of each datagram.
pub mod kind {
    pub const DATA: u8 = 1;
    pub const FIN: u8 = 2;
    pub const ACK: u8 = 3;
    pub const PROBE: u8 = 4;
}

/// The bytes of a `DATA` header.
pub const DATA_HEADER: usize = 9;

/// The bytes of a `FIN`.
pub const FIN_LEN: usize = 5;

/// The bytes of an `ACK`.
pub const ACK_LEN: usize = 39;

/// The frames that one `ACK` covers, and the most frames in flight.
pub const SACK_BITS: usize = 256;

/// The largest payload of one fragment: 1 KiB.
pub const MAX_FRAGMENT: usize = 1 << 10;

/// The smallest payload of one fragment that a setting can give.
pub const MIN_FRAGMENT: usize = 32;

/// The bitmap of an `ACK`: one bit for each of [`SACK_BITS`] frames.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Bitmap(pub [u8; SACK_BITS / 8]);

impl Bitmap {
    /// Whether the frame `i` after `next` came.
    pub fn get(&self, i: usize) -> bool {
        i < SACK_BITS && self.0[i / 8] & (1 << (i % 8)) != 0
    }

    /// Mark the frame `i` after `next` as come.
    pub fn set(&mut self, i: usize) {
        if i < SACK_BITS {
            self.0[i / 8] |= 1 << (i % 8);
        }
    }

    /// The frames marked.
    pub fn count(&self) -> usize {
        self.0.iter().map(|b| b.count_ones() as usize).sum()
    }
}

impl fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries((0..SACK_BITS).filter(|i| self.get(*i))).finish()
    }
}

/// One datagram of the ARQ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Datagram {
    /// The fragment `index` of `count` of a record, at `seq`.
    Data { seq: u32, index: u16, count: u16, payload: Vec<u8> },
    /// The end of the sender's bytes, at `seq`.
    Fin { seq: u32 },
    /// The state of the receiver.
    Ack { next: u32, window: u16, bitmap: Bitmap },
    /// A request for an `ACK`.
    Probe,
}

impl Datagram {
    /// The bytes of the datagram on the carrier.
    pub fn wire_len(&self) -> usize {
        match self {
            Datagram::Data { payload, .. } => DATA_HEADER + payload.len(),
            Datagram::Fin { .. } => FIN_LEN,
            Datagram::Ack { .. } => ACK_LEN,
            Datagram::Probe => 1,
        }
    }

    /// Append the datagram to `out`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Datagram::Data { seq, index, count, payload } => {
                out.push(kind::DATA);
                out.extend_from_slice(&seq.to_be_bytes());
                out.extend_from_slice(&index.to_be_bytes());
                out.extend_from_slice(&count.to_be_bytes());
                out.extend_from_slice(payload);
            }
            Datagram::Fin { seq } => {
                out.push(kind::FIN);
                out.extend_from_slice(&seq.to_be_bytes());
            }
            Datagram::Ack { next, window, bitmap } => {
                out.push(kind::ACK);
                out.extend_from_slice(&next.to_be_bytes());
                out.extend_from_slice(&window.to_be_bytes());
                out.extend_from_slice(&bitmap.0);
            }
            Datagram::Probe => out.push(kind::PROBE),
        }
    }

    /// The datagram as bytes.
    pub fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.wire_len());
        self.encode(&mut out);
        out
    }

    /// Read one datagram. A datagram of another length, an empty payload,
    /// an index not below its count or a payload over [`MAX_FRAGMENT`] is
    /// an error.
    pub fn decode(bytes: &[u8]) -> Result<Datagram, WireError> {
        let (&t, rest) = bytes.split_first().ok_or(WireError::Empty)?;
        let bad = || WireError::BadLength { kind: t, len: bytes.len() };
        let u32_at = |at: usize| u32::from_be_bytes(rest[at..at + 4].try_into().expect("four bytes"));
        let u16_at = |at: usize| u16::from_be_bytes(rest[at..at + 2].try_into().expect("two bytes"));
        match t {
            kind::DATA => {
                if bytes.len() <= DATA_HEADER || bytes.len() > DATA_HEADER + MAX_FRAGMENT {
                    return Err(bad());
                }
                let (index, count) = (u16_at(4), u16_at(6));
                if index >= count {
                    return Err(WireError::Index { index, count });
                }
                Ok(Datagram::Data { seq: u32_at(0), index, count, payload: rest[8..].to_vec() })
            }
            kind::FIN if bytes.len() == FIN_LEN => Ok(Datagram::Fin { seq: u32_at(0) }),
            kind::ACK if bytes.len() == ACK_LEN => {
                let bitmap = Bitmap(rest[6..].try_into().expect("32 bytes"));
                Ok(Datagram::Ack { next: u32_at(0), window: u16_at(4), bitmap })
            }
            kind::PROBE if bytes.len() == 1 => Ok(Datagram::Probe),
            kind::FIN | kind::ACK | kind::PROBE => Err(bad()),
            other => Err(WireError::UnknownKind(other)),
        }
    }
}

/// A datagram that the ARQ does not take. The ARQ drops it and counts it:
/// a lossy carrier can give bytes of another sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// A datagram with no byte.
    Empty,
    /// A type byte that the protocol does not have.
    UnknownKind(u8),
    /// A datagram whose length does not fit its type.
    BadLength { kind: u8, len: usize },
    /// A fragment index that is not below its count.
    Index { index: u16, count: u16 },
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Empty => f.write_str("an empty datagram"),
            WireError::UnknownKind(k) => write!(f, "a datagram of the unknown type 0x{k:02x}"),
            WireError::BadLength { kind, len } => write!(f, "a datagram of the type {kind} with {len} bytes"),
            WireError::Index { index, count } => write!(f, "the fragment {index} of a count of {count}"),
        }
    }
}

impl std::error::Error for WireError {}

/// The distance from `from` to `to` in the sequence space: the frames
/// after `from` up to `to`, with the wrap. Below 2^31 when `to` is after
/// `from`.
pub(crate) fn after(from: u32, to: u32) -> u32 {
    to.wrapping_sub(from)
}

/// Whether `a` comes before `b` in the sequence space (serial numbers of
/// RFC 1982, within 2^31).
pub(crate) fn before(a: u32, b: u32) -> bool {
    a != b && b.wrapping_sub(a) < 1 << 31
}
