//! The frames of a link and their codec, with no I/O.
//!
//! Each end first writes the [`PREFACE`]. Then each frame is a header of
//! 8 bytes and a body:
//!
//! ```text
//! 0      1           5            8
//! +------+-----------+------------+----------------+
//! | type | stream id | body bytes | body ...       |
//! | u8   | u32, BE   | u24, BE    | 0 to 1 MiB     |
//! +------+-----------+------------+----------------+
//! ```
//!
//! A frame is the record that a link gives its carrier. A body has at most
//! [`MAX_RECORD`] bytes; a carrier with a smaller unit splits a record
//! itself. Stream id 0 is the link: `WINDOW` 0 gives credit to the link,
//! and `PING`, `PONG` and `GOAWAY` use only id 0.

use std::fmt;

/// The first bytes of each end: the name and the version of the protocol.
/// A peer that speaks another protocol or version fails at once.
pub const PREFACE: &[u8; 8] = b"PODLINK\x01";

/// The bytes of a frame header.
pub const HEADER: usize = 8;

/// The most bytes in the body of one frame (a record): 1 MiB.
pub const MAX_RECORD: usize = 1 << 20;

/// The type byte of each frame.
pub mod kind {
    pub const OPEN: u8 = 0x01;
    pub const DATA: u8 = 0x02;
    pub const WINDOW: u8 = 0x03;
    pub const CLOSE: u8 = 0x04;
    pub const RESET: u8 = 0x05;
    pub const PING: u8 = 0x06;
    pub const PONG: u8 = 0x07;
    pub const GOAWAY: u8 = 0x08;
}

/// One frame of a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// A new stream. The id is odd from the dialer and even from the
    /// listener, and larger than each id that the same end opened before.
    Open(u32),
    /// Bytes of a stream, never empty.
    Data(u32, Vec<u8>),
    /// More credit for a stream, or for the link (id 0). Never 0.
    Window(u32, u32),
    /// The sender writes no more bytes on the stream (a half close).
    Close(u32),
    /// The stream ends in both directions, with a code ([`crate::ResetCode`]).
    Reset(u32, u32),
    /// A keepalive probe with an opaque value.
    Ping(u64),
    /// The answer to a [`Frame::Ping`], with its value.
    Pong(u64),
    /// The end of the link, with a code ([`crate::GoAwayCode`]).
    GoAway(u32),
}

impl Frame {
    /// The stream id of the frame (0 for the link).
    pub fn stream(&self) -> u32 {
        match self {
            Frame::Open(id) | Frame::Data(id, _) | Frame::Window(id, _) | Frame::Close(id) | Frame::Reset(id, _) => *id,
            Frame::Ping(_) | Frame::Pong(_) | Frame::GoAway(_) => 0,
        }
    }

    /// The bytes of the frame on the carrier: header and body.
    pub fn wire_len(&self) -> usize {
        HEADER
            + match self {
                Frame::Open(_) | Frame::Close(_) => 0,
                Frame::Data(_, body) => body.len(),
                Frame::Window(..) | Frame::Reset(..) | Frame::GoAway(_) => 4,
                Frame::Ping(_) | Frame::Pong(_) => 8,
            }
    }

    /// Append the frame to `out`. A frame that breaks a rule of
    /// [`Decoder`] (an empty or too long body, id 0 for a stream, a zero
    /// credit) is an error, and `out` stays as it was.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), FrameError> {
        self.check()?;
        let (t, id) = (self.kind(), self.stream());
        let body: &[u8] = match self {
            Frame::Data(_, body) => body,
            _ => &[],
        };
        let len = self.wire_len() - HEADER;
        out.push(t);
        out.extend_from_slice(&id.to_be_bytes());
        out.extend_from_slice(&(len as u32).to_be_bytes()[1..]);
        match self {
            Frame::Window(_, v) | Frame::Reset(_, v) | Frame::GoAway(v) => out.extend_from_slice(&v.to_be_bytes()),
            Frame::Ping(v) | Frame::Pong(v) => out.extend_from_slice(&v.to_be_bytes()),
            _ => out.extend_from_slice(body),
        }
        Ok(())
    }

    fn kind(&self) -> u8 {
        match self {
            Frame::Open(_) => kind::OPEN,
            Frame::Data(..) => kind::DATA,
            Frame::Window(..) => kind::WINDOW,
            Frame::Close(_) => kind::CLOSE,
            Frame::Reset(..) => kind::RESET,
            Frame::Ping(_) => kind::PING,
            Frame::Pong(_) => kind::PONG,
            Frame::GoAway(_) => kind::GOAWAY,
        }
    }

    fn check(&self) -> Result<(), FrameError> {
        match self {
            Frame::Open(0) | Frame::Data(0, _) | Frame::Close(0) | Frame::Reset(0, _) => {
                Err(FrameError::LinkId(self.kind()))
            }
            Frame::Data(_, body) if body.is_empty() => Err(FrameError::EmptyData),
            Frame::Data(_, body) if body.len() > MAX_RECORD => Err(FrameError::TooLong(body.len())),
            Frame::Window(_, 0) => Err(FrameError::ZeroCredit),
            _ => Ok(()),
        }
    }
}

/// A frame that breaks the rules of the protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// The first bytes are not [`PREFACE`].
    Preface,
    /// A type byte that the protocol does not have.
    UnknownKind(u8),
    /// A body longer than [`MAX_RECORD`].
    TooLong(usize),
    /// A body whose length does not fit its type.
    BadLength { kind: u8, len: usize },
    /// A stream frame with id 0.
    LinkId(u8),
    /// A link frame with a stream id.
    StreamId { kind: u8, id: u32 },
    /// A `DATA` frame with no byte.
    EmptyData,
    /// A `WINDOW` frame of no credit.
    ZeroCredit,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Preface => f.write_str("the peer did not start with the preface of pod-link version 1"),
            FrameError::UnknownKind(k) => write!(f, "a frame of the unknown type 0x{k:02x}"),
            FrameError::TooLong(n) => write!(f, "a frame body of {n} bytes, over the limit of {MAX_RECORD}"),
            FrameError::BadLength { kind, len } => write!(f, "a frame of type 0x{kind:02x} with a body of {len} bytes"),
            FrameError::LinkId(k) => write!(f, "a stream frame of type 0x{k:02x} with the stream id 0"),
            FrameError::StreamId { kind, id } => write!(f, "a link frame of type 0x{kind:02x} with the stream id {id}"),
            FrameError::EmptyData => f.write_str("a DATA frame with no byte"),
            FrameError::ZeroCredit => f.write_str("a WINDOW frame of no credit"),
        }
    }
}

impl std::error::Error for FrameError {}

/// The decoder of the frames of one direction. Push the bytes as they
/// come, then take each frame with [`Decoder::next`]. A header gives the
/// length of its body, so a body over [`MAX_RECORD`] fails at its header,
/// before its bytes come. After an error the decoder gives the same error
/// again and reads nothing more.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
    start: usize,
    preface: bool,
    failed: Option<FrameError>,
}

impl Decoder {
    /// A decoder of frames with no preface.
    pub fn new() -> Decoder {
        Decoder::default()
    }

    /// A decoder that first reads the [`PREFACE`].
    pub fn with_preface() -> Decoder {
        Decoder { preface: true, ..Decoder::default() }
    }

    /// Add bytes from the carrier.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.failed.is_some() {
            return;
        }
        if self.start > 0 && self.start == self.buf.len() {
            self.buf.clear();
            self.start = 0;
        } else if self.start > MAX_RECORD {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        self.buf.extend_from_slice(bytes);
    }

    /// The bytes that wait for a frame.
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.start
    }

    /// The next whole frame, `None` when more bytes are necessary. Not an
    /// iterator: an error is not the end.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<Frame>, FrameError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        match self.take() {
            Err(e) => {
                self.failed = Some(e.clone());
                self.buf = Vec::new();
                self.start = 0;
                Err(e)
            }
            ok => ok,
        }
    }

    fn take(&mut self) -> Result<Option<Frame>, FrameError> {
        let have = &self.buf[self.start..];
        if self.preface {
            let n = have.len().min(PREFACE.len());
            if have[..n] != PREFACE[..n] {
                return Err(FrameError::Preface);
            }
            if n < PREFACE.len() {
                return Ok(None);
            }
            self.preface = false;
            self.start += PREFACE.len();
            return self.take();
        }
        if have.len() < HEADER {
            // A type byte that no frame has fails before its header is whole.
            return match have.first() {
                Some(&k) if !(kind::OPEN..=kind::GOAWAY).contains(&k) => Err(FrameError::UnknownKind(k)),
                _ => Ok(None),
            };
        }
        let (t, id) = (have[0], u32::from_be_bytes([have[1], have[2], have[3], have[4]]));
        let len = u32::from_be_bytes([0, have[5], have[6], have[7]]) as usize;
        shape(t, id, len)?;
        if have.len() < HEADER + len {
            return Ok(None);
        }
        let body = &have[HEADER..HEADER + len];
        let word = || u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
        let long = || u64::from_be_bytes([body[0], body[1], body[2], body[3], body[4], body[5], body[6], body[7]]);
        let frame = match t {
            kind::OPEN => Frame::Open(id),
            kind::DATA => Frame::Data(id, body.to_vec()),
            kind::WINDOW if word() == 0 => return Err(FrameError::ZeroCredit),
            kind::WINDOW => Frame::Window(id, word()),
            kind::CLOSE => Frame::Close(id),
            kind::RESET => Frame::Reset(id, word()),
            kind::PING => Frame::Ping(long()),
            kind::PONG => Frame::Pong(long()),
            _ => Frame::GoAway(word()),
        };
        self.start += HEADER + len;
        Ok(Some(frame))
    }
}

/// Check the type, the id and the body length of a header.
fn shape(t: u8, id: u32, len: usize) -> Result<(), FrameError> {
    let want = match t {
        kind::OPEN | kind::CLOSE => Some(0),
        kind::DATA => None,
        kind::WINDOW | kind::RESET | kind::GOAWAY => Some(4),
        kind::PING | kind::PONG => Some(8),
        other => return Err(FrameError::UnknownKind(other)),
    };
    let stream_frame = matches!(t, kind::OPEN | kind::DATA | kind::CLOSE | kind::RESET);
    let link_only = matches!(t, kind::PING | kind::PONG | kind::GOAWAY);
    if stream_frame && id == 0 {
        return Err(FrameError::LinkId(t));
    }
    if link_only && id != 0 {
        return Err(FrameError::StreamId { kind: t, id });
    }
    match want {
        Some(w) if w != len => Err(FrameError::BadLength { kind: t, len }),
        None if len == 0 => Err(FrameError::EmptyData),
        None if len > MAX_RECORD => Err(FrameError::TooLong(len)),
        _ => Ok(()),
    }
}
