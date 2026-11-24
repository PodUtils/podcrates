//! The error of a command: a response code of the TPM, a response that
//! fails a bound, a value of the caller that cannot go into a command, a
//! type or an algorithm that the crate does not use, or a failure of the
//! [`crate::Transport`]. [`Error::class`] gives the class of `pod_core`; a
//! [`Failure`] made from an error is at [`Stage::Host`].

use std::fmt;

use pod_core::{Class, Failure, Stage};

use crate::keyfile::Refusal;
use crate::rc::ResponseCode;

/// The error of one command.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The TPM refused the command with this response code.
    Tpm(ResponseCode),
    /// The bytes are no answer of a TPM 2.0 to the command: a size, a count
    /// or a tag fails its bound.
    Malformed(String),
    /// A value of the caller cannot go into a command.
    Usage(String),
    /// The TPM gave a type, an algorithm or a curve that the crate does not
    /// use, or the caller asked for one.
    Unsupported(String),
    /// The transport did not give a response.
    Transport(Failure),
    /// A key file `TSS2 PRIVATE KEY` that the crate does not read or write.
    KeyFile(Refusal),
    /// A step of a sequence of [`crate::flow`] failed: what it means, its
    /// class, and the error of the command under it, when there is one.
    Step { class: Class, why: String, cause: Option<Box<Error>> },
}

impl Error {
    /// The class of `pod_core`: of the response code, `Protocol` for a
    /// malformed response, `Usage`, `Unsupported`, or the class of the
    /// failure of the transport.
    pub fn class(&self) -> Class {
        match self {
            Error::Tpm(code) => code.class(),
            Error::Malformed(_) => Class::Protocol,
            Error::Usage(_) => Class::Usage,
            Error::Unsupported(_) => Class::Unsupported,
            Error::Transport(failure) => failure.class,
            Error::KeyFile(refusal) => refusal.class(),
            Error::Step { class, .. } => *class,
        }
    }

    /// The response code of the TPM, when the TPM refused.
    pub fn code(&self) -> Option<ResponseCode> {
        match self {
            Error::Tpm(code) => Some(*code),
            Error::Step { cause: Some(cause), .. } => cause.code(),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Tpm(code) => code.fmt(f),
            Error::Malformed(why) => write!(f, "the answer of the TPM cannot be read: {why}"),
            Error::Usage(why) | Error::Unsupported(why) => f.write_str(why),
            Error::Transport(failure) => failure.fmt(f),
            Error::KeyFile(refusal) => write!(f, "the key file of a TPM: {refusal}"),
            Error::Step { why, cause: Some(cause), .. } => write!(f, "{why}: {cause}"),
            Error::Step { why, cause: None, .. } => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Transport(failure) => Some(failure),
            Error::Step { cause: Some(cause), .. } => Some(cause.as_ref()),
            _ => None,
        }
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        match e {
            Error::Transport(failure) => failure,
            other => Failure::new(Stage::Host, other.class(), other.to_string()),
        }
    }
}
