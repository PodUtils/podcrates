//! The failures of a lock, a store and a resolve. Each names its reference,
//! its commit or its file; the consumer gives each its class and its words
//! for the user (the flag of a mode, the name of its command).

use std::fmt;
use std::path::PathBuf;

use crate::store::Mode;

/// What is wrong with one line of a lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineFault {
    /// The line is not `OWNER/REPO@REF COMMIT sha256:HEX`.
    Form,
    /// The line of an image is not `docker://NAME:TAG sha256:HEX`.
    ImageForm,
    /// The reference or the image is on an earlier line too.
    Again(String),
}

/// What is wrong with a field of `with:` of a checkout of another
/// repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckoutFault {
    /// An expression: the host resolves each tree before the first job.
    Expression,
    /// Not a string.
    NotString,
    /// `repository` is not `OWNER/REPO`.
    NotRepository,
}

/// A failure of this crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A line of a lock: the file (when the lock came from one), the
    /// number of the line from 1, and the fault.
    Line { path: Option<PathBuf>, line: usize, fault: LineFault },
    /// A file of the lock or of the store could not be read or written.
    File { path: PathBuf, kind: std::io::ErrorKind, reason: String },
    /// A `uses:` that is not `OWNER/REPO[/PATH]@REF`.
    Uses { uses: String },
    /// The forge did not resolve a ref, or did not give a tar.
    Forge { repo: String, reason: String },
    /// A tarball that is not a tar.
    Tarball { key: String, reason: String },
    /// The tarball of a reference has no `action.yml` at its path.
    NoAction { uses: String, path: String },
    /// The `action.yml` of a reference does not parse.
    Action { uses: String, reason: String },
    /// A tag or a branch names another commit than the lock.
    Moved { key: String, lock: String, pinned: String, now: String },
    /// A reference by commit that the lock pins to another commit.
    Pinned { key: String, lock: String, commit: String, pinned: String },
    /// A reference that the lock does not have, in a mode that refuses a
    /// change of the lock.
    NotLocked { key: String, lock: String, mode: Mode },
    /// A commit that is not in the store, in the mode that fetches nothing.
    NotStored { key: String, commit: String },
    /// A tarball whose digest is not the digest of the lock.
    Digest { key: String, commit: String, lock: String, digest: String, pinned: String },
    /// A field (`repository` or `ref`) of `with:` of a checkout of another
    /// repository, with its value.
    Checkout { field: &'static str, value: String, fault: CheckoutFault },
    /// A tree of a checkout that is not `OWNER/REPO@REF`.
    Tree { tree: String },
    /// An image of a workflow that is an expression, with its line.
    ImageExpression { image: String, line: u32 },
    /// The source of images did not pull an image, or did not give a
    /// digest `sha256:HEX`.
    Registry { image: String, reason: String },
    /// An image tag that names another digest than the lock.
    ImageMoved { image: String, lock: String, pinned: String, now: String },
    /// An image that the lock does not have, in a mode that refuses a
    /// change of the lock.
    ImageNotLocked { image: String, lock: String, mode: Mode },
}

impl Error {
    pub(crate) fn file(path: &std::path::Path, e: &std::io::Error) -> Error {
        Error::File { path: path.to_path_buf(), kind: e.kind(), reason: e.to_string() }
    }
}

impl fmt::Display for LineFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LineFault::Form => f.write_str("not `OWNER/REPO@REF COMMIT sha256:HEX`"),
            LineFault::ImageForm => f.write_str("not `docker://NAME:TAG sha256:HEX`"),
            LineFault::Again(key) => write!(f, "`{key}` again"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Line { path: Some(p), line, fault } => write!(f, "{}: line {line}: {fault}", p.display()),
            Error::Line { path: None, line, fault } => write!(f, "line {line}: {fault}"),
            Error::File { path, reason, .. } => write!(f, "{}: {reason}", path.display()),
            Error::Uses { uses } => write!(f, "`uses: {uses}` is not OWNER/REPO[/PATH]@REF"),
            Error::Forge { reason, .. } => f.write_str(reason),
            Error::Tarball { key, reason } => write!(f, "the tarball of `{key}`: {reason}"),
            Error::NoAction { uses, path } => write!(f, "`uses: {uses}`: the tarball has no action.yml at `{path}`"),
            Error::Action { uses, reason } => write!(f, "the action.yml of `{uses}`: {reason}"),
            Error::Moved { key, lock, pinned, now } => {
                write!(
                    f,
                    "`{key}` moved: {lock} has {pinned}, the forge has {now}; remove its line from {lock} to take {now}"
                )
            }
            Error::Pinned { key, lock, commit, pinned } => write!(f, "`{key}` names {commit}, but {lock} has {pinned}"),
            Error::NotLocked { key, lock, mode } => {
                write!(f, "`{key}` is not in {lock}, and the {mode} mode refuses a change of {lock}")
            }
            Error::NotStored { key, commit } => {
                write!(f, "`{key}`: {commit} is not in the store, and the offline mode fetches nothing")
            }
            Error::Digest { key, commit, lock, digest, pinned } => {
                write!(f, "the tarball of `{key}` ({commit}) has {digest}, but {lock} has {pinned}")
            }
            Error::Checkout { field, value, fault: CheckoutFault::Expression } => {
                write!(f, "the `{field}` of a checkout of another repository must be plain, not {value:?}")
            }
            Error::Checkout { field, fault: CheckoutFault::NotString, .. } => {
                write!(f, "the `{field}` of a checkout must be a string")
            }
            Error::Checkout { field, value, fault: CheckoutFault::NotRepository } => {
                write!(f, "the `{field}` of a checkout is not OWNER/REPO: {value:?}")
            }
            Error::Tree { tree } => write!(f, "the checkout `{tree}` is not OWNER/REPO@REF"),
            Error::ImageExpression { image, line } => {
                write!(f, "line {line}: the image {image:?} is an expression; name it, or pin it by digest")
            }
            Error::Registry { image, reason } => write!(f, "the image `{image}`: {reason}"),
            Error::ImageMoved { image, lock, pinned, now } => write!(
                f,
                "the image `{image}` moved: {lock} has {pinned}, the registry has {now}; remove its line from {lock} to take {now}"
            ),
            Error::ImageNotLocked { image, lock, mode } => {
                write!(f, "the image `{image}` is not in {lock}, and the {mode} mode refuses a change of {lock}")
            }
        }
    }
}

impl std::error::Error for Error {}
