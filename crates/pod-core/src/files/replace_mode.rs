//! The mode of a replaced file, and the error of a replace that names its
//! step and its path (T-1066).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// The mode of the new file of a replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 0600: only the owner can read and write the file.
    Private,
    /// The permission bits of the old file at the path (of the target of a
    /// followed link), without setuid, setgid and sticky; 0600 when there
    /// is no regular file. On Windows: a private file when the old file is
    /// missing or closed to the other users, else a file that each may read.
    Keep,
    /// These permission bits (Unix). On Windows a mode with no bit for the
    /// other users gives a private file.
    Exact(u32),
}

impl Mode {
    /// The permission bits for the new file at `path`.
    pub(super) fn bits(self, path: &Path) -> io::Result<u32> {
        match self {
            Mode::Private => Ok(0o600),
            Mode::Exact(mode) => Ok(mode),
            Mode::Keep => kept(path),
        }
    }
}

#[cfg(unix)]
fn kept(path: &Path) -> io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(meta.permissions().mode() & 0o777),
        Ok(_) => Ok(0o600),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0o600),
        Err(e) => Err(e),
    }
}

#[cfg(windows)]
fn kept(path: &Path) -> io::Result<u32> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => {
            let file = std::fs::File::open(path)?;
            let open = super::closed(path, &file, super::Others::NoAccess).is_err();
            Ok(if open { 0o644 } else { 0o600 })
        }
        Ok(_) => Ok(0o600),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0o600),
        Err(e) => Err(e),
    }
}

#[cfg(not(any(unix, windows)))]
fn kept(_path: &Path) -> io::Result<u32> {
    Ok(0o600)
}

/// The step of a replace that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// The follow of the link at the path.
    FollowLink,
    /// The path has no file name.
    Name,
    /// The make of a missing directory.
    MakeDir,
    /// The read of the mode of the old file ([`Mode::Keep`]).
    ReadMode,
    /// The create of the temporary file.
    Create,
    /// The write of the body into the temporary file.
    Write,
    /// The sync of the temporary file.
    Sync,
    /// The check of the temporary file ([`super::Replace::write_checked`]).
    Check,
    /// The rename of the temporary file onto the path.
    Rename,
    /// The write in place after a failed rename.
    InPlace,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Step::FollowLink => "the follow of its symbolic link",
            Step::Name => "the file name",
            Step::MakeDir => "the make of its directory",
            Step::ReadMode => "the read of the mode of the old file",
            Step::Create => "the create of the temporary file",
            Step::Write => "the write of the temporary file",
            Step::Sync => "the sync of the temporary file",
            Step::Check => "the check of the temporary file",
            Step::Rename => "the rename of the temporary file onto it",
            Step::InPlace => "the write in place",
        })
    }
}

/// A replace that failed: its step, the path of the step (the target of a
/// followed link after the follow), and the cause. Its text is
/// "replace PATH: STEP: CAUSE".
#[derive(Debug)]
pub struct Failed {
    step: Step,
    path: PathBuf,
    cause: io::Error,
}

impl Failed {
    pub(super) fn new(step: Step, path: &Path, cause: io::Error) -> Failed {
        Failed { step, path: path.to_path_buf(), cause }
    }

    /// The step that failed.
    pub fn step(&self) -> Step {
        self.step
    }

    /// The path of the step.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The error of the system.
    pub fn cause(&self) -> &io::Error {
        &self.cause
    }

    /// The error of the system, with no step and no path.
    pub fn into_cause(self) -> io::Error {
        self.cause
    }

    /// The [`Failed`] in an error of [`super::replace`] or
    /// [`super::Replace::write`].
    pub fn of(error: &io::Error) -> Option<&Failed> {
        error.get_ref().and_then(|inner| inner.downcast_ref::<Failed>())
    }
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "replace {}: {}: {}", self.path.display(), self.step, self.cause)
    }
}

// No source: the text holds the cause, and a chain of errors would print it
// twice.
impl std::error::Error for Failed {}

impl From<Failed> for io::Error {
    fn from(failed: Failed) -> io::Error {
        io::Error::new(failed.cause.kind(), failed)
    }
}
