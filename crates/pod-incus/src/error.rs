//! The error of a call. Each error names the call (`POST
//! /1.0/instances/web/exec`) and what went wrong: the error of incus in its
//! own words, a background operation that failed, a socket where no incus
//! answers, an answer that breaks the API, or a value of the caller that
//! cannot go into a request. [`Error::class`] gives the class of
//! `pod_core`; a [`pod_core::Failure`] made from an error is at
//! [`Stage::Host`] and keeps the error as its cause.

use std::fmt;
use std::io;
use std::path::PathBuf;

use pod_core::text::one_line;
use pod_core::{Class, Failure, Stage};

/// The result of a call.
pub type Result<T> = std::result::Result<T, Error>;

/// The longest text of the far end that an error keeps.
pub(crate) const MAX_TEXT: usize = 512;

/// The error of one call.
#[derive(Debug)]
pub struct Error {
    call: String,
    kind: ErrorKind,
}

/// What went wrong.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// No incus answers at the socket: the path does not exist, it is not a
    /// socket, the connection was refused, or this system has no Unix
    /// socket.
    NoServer { socket: PathBuf, cause: io::Error },
    /// incus answered with an error: the HTTP status, the `error_code` and
    /// the `error` of its envelope.
    Incus { status: u16, code: u16, error: String },
    /// A background operation ended with no success: its id, its
    /// `status_code` (400 failure, 401 canceled) and its `err`.
    Operation { id: String, code: u16, err: String },
    /// A wait ended at its time limit; the operation still runs.
    Timeout { id: String, status: String },
    /// The answer of incus breaks the API: no JSON, no envelope, a type that
    /// the call does not take, a field that is not there.
    Protocol(String),
    /// The server does not have what the call needs: an API extension, the
    /// API version.
    Unsupported { what: String },
    /// A value of the caller cannot go into a request.
    Usage(String),
    /// A read or a write failed after the connection opened.
    Io(io::Error),
    /// A websocket of an operation failed.
    WebSocket(String),
}

impl Error {
    pub(crate) fn new(call: &str, kind: ErrorKind) -> Error {
        Error { call: call.to_string(), kind }
    }

    pub(crate) fn protocol(call: &str, why: impl Into<String>) -> Error {
        Error::new(call, ErrorKind::Protocol(why.into()))
    }

    pub(crate) fn usage(call: &str, why: impl Into<String>) -> Error {
        Error::new(call, ErrorKind::Usage(why.into()))
    }

    pub(crate) fn io(call: &str, e: io::Error) -> Error {
        Error::new(call, ErrorKind::Io(e))
    }

    /// The call, as its method and its path: `GET /1.0`.
    pub fn call(&self) -> &str {
        &self.call
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    /// The `error` of incus, when incus answered with one.
    pub fn incus_error(&self) -> Option<&str> {
        match &self.kind {
            ErrorKind::Incus { error, .. } => Some(error),
            ErrorKind::Operation { err, .. } => Some(err),
            _ => None,
        }
    }

    /// The HTTP status of an error answer of incus.
    pub fn status(&self) -> Option<u16> {
        match &self.kind {
            ErrorKind::Incus { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// What a caller can do about the error.
    pub fn class(&self) -> Class {
        match &self.kind {
            ErrorKind::NoServer { cause, .. } => match cause.kind() {
                io::ErrorKind::PermissionDenied => Class::Refused,
                io::ErrorKind::TimedOut => Class::Timeout,
                io::ErrorKind::Unsupported => Class::Unsupported,
                _ => Class::Unavailable,
            },
            ErrorKind::Incus { status, .. } => match status {
                400 => Class::Usage,
                401 | 403 => Class::Refused,
                404 => Class::NotFound,
                409 | 412 => Class::TempFail,
                501 => Class::Unsupported,
                503 => Class::Unavailable,
                _ => Class::Protocol,
            },
            ErrorKind::Operation { code: 401, .. } => Class::TempFail,
            ErrorKind::Operation { .. } => Class::Unavailable,
            ErrorKind::Timeout { .. } => Class::Timeout,
            ErrorKind::Protocol(_) | ErrorKind::WebSocket(_) => Class::Protocol,
            ErrorKind::Unsupported { .. } => Class::Unsupported,
            ErrorKind::Usage(_) => Class::Usage,
            ErrorKind::Io(e) => Class::of_io(e.kind()),
        }
    }

    /// The stage of a [`Failure`] of this crate: the local incus.
    pub fn stage(&self) -> Stage {
        Stage::Host
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let call = &self.call;
        match &self.kind {
            ErrorKind::NoServer { socket, cause } => {
                write!(f, "{call}: no incus answers at {}: {cause}", socket.display())
            }
            ErrorKind::Incus { status, code, error } => {
                write!(f, "{call}: incus refused the call ({status}, error_code {code}): {}", one_line(error))
            }
            ErrorKind::Operation { id, code, err } => {
                let how = if *code == 401 { "was canceled" } else { "failed" };
                write!(f, "{call}: the operation {id} {how} ({code}): {}", one_line(err))
            }
            ErrorKind::Timeout { id, status } => {
                write!(f, "{call}: the wait for the operation {id} ended at its limit; it is {status}")
            }
            ErrorKind::Protocol(why) => write!(f, "{call}: the answer of incus breaks the API: {why}"),
            ErrorKind::Unsupported { what } => write!(f, "{call}: the incus server does not have {what}"),
            ErrorKind::Usage(why) => write!(f, "{call}: {why}"),
            ErrorKind::Io(e) => write!(f, "{call}: {e}"),
            ErrorKind::WebSocket(why) => write!(f, "{call}: a websocket of the operation failed: {why}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::NoServer { cause, .. } | ErrorKind::Io(cause) => Some(cause),
            _ => None,
        }
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        Failure::caused(e.stage(), e.class(), e.to_string(), e)
    }
}

/// `text` of the far end on one line, cut to [`MAX_TEXT`] characters.
pub(crate) fn far_text(text: &str) -> String {
    let line = one_line(text);
    match line.char_indices().nth(MAX_TEXT) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}
