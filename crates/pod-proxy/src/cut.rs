//! How a connection ended when a dial or a TLS handshake failed: its kind,
//! read from the `std::io::ErrorKind` of the error that ended it, kept beside
//! the words. The words are the system's and differ between systems
//! (Windows says os error 10054 for a reset), so a caller that tells a reset
//! from a timeout reads the kind, never the words.

use std::error::Error;
use std::fmt;
use std::io::ErrorKind;

use pod_core::{Class, Failure};
use pod_tls::Words;

use crate::error::DialError;

/// How a connection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// The far end reset it, or aborted it.
    Reset,
    /// The far end refused it: nothing listens, or a firewall answered so.
    Refused,
    /// Nothing came within the bound.
    TimedOut,
    /// The far end closed it with nothing sent.
    Closed,
    /// Anything else that I/O said.
    Other,
}

impl Cut {
    /// The cut of an error of I/O of this kind.
    pub fn of(kind: ErrorKind) -> Cut {
        match kind {
            ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => Cut::Reset,
            ErrorKind::ConnectionRefused => Cut::Refused,
            ErrorKind::TimedOut => Cut::TimedOut,
            // A write after the far end went, and a read of its end.
            ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe => Cut::Closed,
            _ => Cut::Other,
        }
    }

    /// How the connection of a failure ended, where an error of I/O or a
    /// bound ended it: a dial's ([`DialError::cut`]), a handshake's error of
    /// I/O in its causes (rustls's own refusal, `InvalidData`, says nothing
    /// of the path), or a bound with no error of I/O. A failure that refused
    /// (a status, a certificate) has none.
    pub fn of_failure(failure: &Failure) -> Option<Cut> {
        let mut next = failure.source();
        while let Some(e) = next {
            if let Some(dial) = e.downcast_ref::<DialError>() {
                return dial.cut();
            }
            if let Some(io) = e.downcast_ref::<std::io::Error>() {
                return (io.kind() != ErrorKind::InvalidData).then(|| Cut::of(io.kind()));
            }
            next = e.source();
        }
        (failure.class == Class::Timeout).then_some(Cut::TimedOut)
    }

    /// The cut as a report names it.
    pub fn words(self) -> &'static str {
        match self {
            Cut::Reset => "reset",
            Cut::Refused => "refused",
            Cut::TimedOut => "timed out",
            Cut::Closed => "closed with nothing",
            Cut::Other => "other",
        }
    }
}

/// Why a TLS handshake failed: the words, and the cut when an error of I/O
/// ended it rather than the handshake's own refusal (a certificate, a
/// protocol), whose kind says nothing of the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsFailure {
    pub why: String,
    pub cut: Option<Cut>,
}

impl TlsFailure {
    /// The failure of a handshake that `error` ended, with rustls's words
    /// and what to do about an unknown issuer, in the `words` of the
    /// consumer.
    pub fn of(error: &std::io::Error, words: &Words) -> TlsFailure {
        let cut = match error.kind() {
            // rustls's own refusal: a certificate, a name, a protocol.
            ErrorKind::InvalidData => None,
            kind => Some(Cut::of(kind)),
        };
        TlsFailure { why: pod_tls::handshake_failure(error, words), cut }
    }
}

impl fmt::Display for TlsFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.why)
    }
}

/// Why a question got no answer, as a doctor counts it: the words, and how
/// its connection ended where an error of I/O or a bound ended it. An
/// answer that refused, and a failure after the handshake, have none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missed {
    pub why: String,
    pub cut: Option<Cut>,
}

impl Missed {
    /// Words alone: a failure whose kind says nothing of the path.
    pub fn words(why: impl Into<String>) -> Missed {
        Missed { why: why.into(), cut: None }
    }

    /// A TLS handshake's failure, `TLS: ` and its words, with its cut.
    pub fn tls(error: &std::io::Error, words: &Words) -> Missed {
        let failure = TlsFailure::of(error, words);
        Missed { why: format!("TLS: {}", failure.why), cut: failure.cut }
    }
}

impl fmt::Display for Missed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.why)
    }
}

/// Words alone: a failure whose kind is not known.
impl From<String> for TlsFailure {
    fn from(why: String) -> TlsFailure {
        TlsFailure { why, cut: None }
    }
}

impl From<&str> for TlsFailure {
    fn from(why: &str) -> TlsFailure {
        TlsFailure { why: why.to_string(), cut: None }
    }
}

impl DialError {
    /// How the dial ended, where its error says: an address's connection,
    /// or the bound.
    pub fn cut(&self) -> Option<Cut> {
        match self {
            DialError::Connect { cut, .. } => *cut,
            DialError::Timeout { .. } => Some(Cut::TimedOut),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "cut_tests.rs"]
mod tests;
