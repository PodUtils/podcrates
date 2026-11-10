//! One error type, and every variant names what a caller can act on.
//!
//! ⛔ `Refused` carries the entry AND the layer digest. `TODO/extract.md`
//! T-0304: "Refuse, name the entry and the layer digest, and fail the
//! extraction rather than skipping the entry." A refusal that names neither is
//! a message whose reader has to guess which of a hundred layers to look in.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// An entry that resolves outside the destination. ⛔ Fatal for the
    /// extraction: T-0304 refuses rather than sanitizes, because a rewritten
    /// path produces an image that differs from its digest with nothing saying
    /// so, and the caller cannot then tell a repaired layer from a clean one.
    Refused {
        layer: String,
        entry: String,
        why: String,
    },
    /// The layer's media type names a compression this build cannot read.
    Compression(String),
    Extract(String),
    Io(std::io::Error),
    /// Passed through from `podvm-image`, so a caller has one error type for
    /// `pull` and `extract` rather than two.
    Image(podvm_image::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Refused { layer, entry, why } => write!(
                f,
                "refusing layer {layer}: the entry {entry:?} is not extractable \
                 because {why}. The layer is refused whole rather than with that \
                 entry skipped, because an image that differs from its digest \
                 with nothing saying so cannot be told from a clean one"
            ),
            Error::Compression(s) => write!(f, "{s}"),
            Error::Extract(s) => write!(f, "{s}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::Image(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

impl From<podvm_image::Error> for Error {
    fn from(e: podvm_image::Error) -> Error {
        Error::Image(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
