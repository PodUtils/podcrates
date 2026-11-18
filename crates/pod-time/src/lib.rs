//! Time zones and the cron of GitHub Actions' `on.schedule` (T-907, call
//! C779):
//!
//! * [`Zone`]: a zone of the system by its IANA name, from its TZif file
//!   (RFC 8536, versions 1 to 4, with the POSIX TZ string of the footer):
//!   the UTC offset at an instant, and the instants of a local time.
//! * [`Cron`]: the five fields of a cron, and its next time in UTC or in a
//!   zone. A local time in a gap runs once, at the first valid time after
//!   the gap; a local time that occurs twice runs once, at its first
//!   instance (the rule of cronie and Vixie cron).
//!
//! The crate uses std alone: no C library, no network. An instant is in
//! seconds since the epoch (1970-01-01T00:00:00Z).

pub mod civil;
pub mod cron;
mod posix;
mod tzif;
mod zone;

pub use cron::Cron;
pub use zone::{Local, ZONEINFO, Zone};

/// The kind of an [`Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The text is not a zone name.
    Name,
    /// The host has no directory of zone files.
    NoZoneFiles,
    /// The directory has no zone of the name.
    NotFound,
    /// The zone file could not be read.
    Read,
    /// The zone file is not a TZif file that the crate reads.
    Format,
    /// The text is not a cron.
    Cron,
}

/// A failure of the crate: its kind and one line of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    text: String,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, text: impl Into<String>) -> Error {
        Error { kind, text: text.into() }
    }

    /// The kind of the failure.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The text of the failure.
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl std::error::Error for Error {}
