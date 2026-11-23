//! One error of the harness: what failed, as one line of text.

use std::fmt;

/// An error of a file, a capture, a model or an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Error {
        Error { message: message.into() }
    }

    /// An error of I/O with the path of the file.
    pub fn io(what: &str, path: &std::path::Path, e: std::io::Error) -> Error {
        Error::new(format!("{what} {}: {e}", path.display()))
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Error {
        Error::new(format!("JSON: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
