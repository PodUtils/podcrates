//! The errors of a link and of its streams, the codes of a reset and of
//! the end of a link, and each error as a [`Failure`] of `pod-core`.

use std::fmt;
use std::io;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};

use crate::frame::FrameError;
use crate::settings::SettingsError;

/// The code of a `RESET` frame. The protocol uses 0 to 15; a caller gives
/// its own codes from 16 with [`crate::Stream::reset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResetCode(pub u32);

impl ResetCode {
    /// The end of the stream at its owner's wish: its handle was dropped
    /// before the stream ended, or the caller reset it with no reason.
    pub const CANCEL: ResetCode = ResetCode(0);
    /// The peer did not take the stream: its limit of streams, or the end
    /// of its link. A later open can work.
    pub const REFUSED: ResetCode = ResetCode(1);
    /// The stream broke the protocol.
    pub const PROTOCOL: ResetCode = ResetCode(2);
}

/// The code of a `GOAWAY` frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GoAwayCode(pub u32);

impl GoAwayCode {
    /// The end of the link at its owner's wish.
    pub const NORMAL: GoAwayCode = GoAwayCode(0);
    /// The peer broke the protocol (a frame, a credit, an id).
    pub const PROTOCOL: GoAwayCode = GoAwayCode(1);
}

/// Why a link or a stream failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A setting out of its limits.
    Settings(SettingsError),
    /// The link holds its limit of streams ([`crate::Settings::max_streams`]).
    TooManyStreams(usize),
    /// This end used each stream id of its parity: open a new link.
    IdsExhausted,
    /// The link ended with a `GOAWAY`: this end or the peer closed it.
    Closed { by_peer: bool },
    /// The peer ended the link with a code other than `NORMAL`.
    PeerGoAway(GoAwayCode),
    /// The carrier ended with no `GOAWAY`.
    CarrierEnded,
    /// The link was silent for this time: the peer or the carrier is gone.
    Dead(Duration),
    /// The peer sent a frame that breaks the protocol.
    Frame(FrameError),
    /// The peer broke a rule of the link: a credit, an id, an order.
    Protocol(String),
    /// The carrier failed.
    Io { kind: io::ErrorKind, text: String },
    /// The stream ended with a reset: by the peer, or by this end.
    Reset { id: u32, code: ResetCode, by_peer: bool },
    /// This end closed its side of the stream: no more writes.
    WriteClosed(u32),
}

impl Error {
    pub(crate) fn io(e: &io::Error) -> Error {
        Error::Io { kind: e.kind(), text: e.to_string() }
    }

    /// The class of the error, for the caller's exit code.
    pub fn class(&self) -> Class {
        match self {
            Error::Settings(_) => Class::Usage,
            Error::TooManyStreams(_) => Class::TempFail,
            Error::IdsExhausted | Error::Closed { .. } | Error::CarrierEnded | Error::WriteClosed(_) => {
                Class::Unavailable
            }
            Error::Dead(_) => Class::Timeout,
            Error::PeerGoAway(_) | Error::Frame(_) | Error::Protocol(_) => Class::Protocol,
            Error::Io { kind, .. } => Class::of_io(*kind),
            Error::Reset { code, .. } if *code == ResetCode::REFUSED => Class::TempFail,
            Error::Reset { code, .. } if *code == ResetCode::PROTOCOL => Class::Protocol,
            Error::Reset { .. } => Class::Unavailable,
        }
    }

    /// The kind of the I/O error of a stream that fails with this error.
    pub fn io_kind(&self) -> io::ErrorKind {
        match self {
            Error::Reset { .. } => io::ErrorKind::ConnectionReset,
            Error::WriteClosed(_) => io::ErrorKind::BrokenPipe,
            Error::Dead(_) => io::ErrorKind::TimedOut,
            Error::Frame(_) | Error::Protocol(_) | Error::PeerGoAway(_) => io::ErrorKind::InvalidData,
            Error::Io { kind, .. } => *kind,
            _ => io::ErrorKind::ConnectionAborted,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Settings(e) => write!(f, "link settings: {e}"),
            Error::TooManyStreams(n) => write!(f, "the link holds its limit of {n} streams"),
            Error::IdsExhausted => f.write_str("the link used each stream id: open a new link"),
            Error::Closed { by_peer: true } => f.write_str("the peer closed the link"),
            Error::Closed { by_peer: false } => f.write_str("the link is closed"),
            Error::PeerGoAway(c) => write!(f, "the peer ended the link with the code {}", c.0),
            Error::CarrierEnded => f.write_str("the carrier of the link ended"),
            Error::Dead(t) => write!(f, "the link was silent for {} s: the peer is gone", t.as_secs_f64()),
            Error::Frame(e) => write!(f, "the peer broke the link protocol: {e}"),
            Error::Protocol(text) => write!(f, "the peer broke the link protocol: {text}"),
            Error::Io { text, .. } => write!(f, "the carrier of the link failed: {text}"),
            Error::Reset { id, code, by_peer: true } => write!(f, "the peer reset the stream {id} (code {})", code.0),
            Error::Reset { id, code, by_peer: false } => write!(f, "the stream {id} was reset (code {})", code.0),
            Error::WriteClosed(id) => write!(f, "the stream {id} is closed for writes"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for io::Error {
    fn from(e: Error) -> io::Error {
        io::Error::new(e.io_kind(), e)
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        Failure::caused(Stage::Session, e.class(), e.to_string(), e)
    }
}

impl From<SettingsError> for Error {
    fn from(e: SettingsError) -> Error {
        Error::Settings(e)
    }
}

impl From<FrameError> for Error {
    fn from(e: FrameError) -> Error {
        Error::Frame(e)
    }
}
