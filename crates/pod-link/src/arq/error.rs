//! The errors of the ARQ, and each error as a [`Failure`] of `pod-core`.

use std::fmt;
use std::io;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};

use super::settings::ArqSettingsError;

/// Why the ARQ ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArqError {
    /// A setting out of its limits.
    Settings(ArqSettingsError),
    /// `arq-timeout`: the frame `seq` had no acknowledgement after
    /// `retries` retransmissions, `waited` after its first send. The peer
    /// or the carrier is gone.
    Timeout { seq: u32, retries: u8, waited: Duration },
    /// The peer broke the protocol: the fragments of a record disagree.
    Protocol(String),
    /// The carrier failed.
    Carrier { kind: io::ErrorKind, text: String },
    /// This end closed its side: no more writes.
    WriteClosed,
}

impl ArqError {
    pub(crate) fn io(e: &io::Error) -> ArqError {
        ArqError::Carrier { kind: e.kind(), text: e.to_string() }
    }

    /// The class of the error, for the caller's exit code.
    pub fn class(&self) -> Class {
        match self {
            ArqError::Settings(_) => Class::Usage,
            ArqError::Timeout { .. } => Class::Timeout,
            ArqError::Protocol(_) => Class::Protocol,
            ArqError::Carrier { kind, .. } => Class::of_io(*kind),
            ArqError::WriteClosed => Class::Unavailable,
        }
    }

    /// The kind of the I/O error of the byte stream that fails with this
    /// error.
    pub fn io_kind(&self) -> io::ErrorKind {
        match self {
            ArqError::Settings(_) => io::ErrorKind::InvalidInput,
            ArqError::Timeout { .. } => io::ErrorKind::TimedOut,
            ArqError::Protocol(_) => io::ErrorKind::InvalidData,
            ArqError::Carrier { kind, .. } => *kind,
            ArqError::WriteClosed => io::ErrorKind::BrokenPipe,
        }
    }
}

impl fmt::Display for ArqError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArqError::Settings(e) => write!(f, "ARQ settings: {e}"),
            ArqError::Timeout { seq, retries, waited } => write!(
                f,
                "arq-timeout: the frame {seq} had no acknowledgement after {retries} retries ({} s): the peer is gone",
                waited.as_secs_f64()
            ),
            ArqError::Protocol(text) => write!(f, "the peer broke the ARQ protocol: {text}"),
            ArqError::Carrier { text, .. } => write!(f, "the datagram carrier failed: {text}"),
            ArqError::WriteClosed => f.write_str("the ARQ stream is closed for writes"),
        }
    }
}

impl std::error::Error for ArqError {}

impl From<ArqError> for io::Error {
    fn from(e: ArqError) -> io::Error {
        io::Error::new(e.io_kind(), e)
    }
}

impl From<ArqError> for Failure {
    fn from(e: ArqError) -> Failure {
        Failure::caused(Stage::Session, e.class(), e.to_string(), e)
    }
}

impl From<ArqSettingsError> for ArqError {
    fn from(e: ArqSettingsError) -> ArqError {
        ArqError::Settings(e)
    }
}
