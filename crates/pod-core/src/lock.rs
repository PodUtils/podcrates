//! An advisory lock of a file, held for the life of a value, on each system
//! of PodUtils: `flock(2)` on Linux and the BSDs, `LockFileEx` on Windows
//! (through `std::fs::File::lock`). One shared lock in place of the copies
//! of the consumers (the owner's ruling "Shared code").
//!
//! The rules of each lock:
//!
//! * The file opens with no link at its last component (`O_NOFOLLOW` on
//!   Unix): a link there never sends the lock to another file.
//! * The descriptor is close-on-exec: a child never holds the lock.
//! * A lock held by another process is a refusal that names the file and
//!   says that another process holds it; a wait has a bound.
//! * On Linux a lock of `flock(2)` belongs to the open file: two values of
//!   one process on one file exclude each other as two processes do.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::failure::{Class, Failure, Stage};

/// The pause between two tries of a bounded wait.
const PAUSE: Duration = Duration::from_millis(20);

/// Exclusive or shared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Exclusive,
    Shared,
}

/// A lock of a file. Dropping it releases the lock.
#[derive(Debug)]
pub struct FileLock {
    file: File,
    path: PathBuf,
    mode: Mode,
}

impl FileLock {
    /// The lock of `path` in `mode`, made where it is absent; `Ok(None)`
    /// where another process holds a lock that excludes it.
    pub fn try_lock(path: &Path, mode: Mode) -> Result<Option<FileLock>, Failure> {
        let file = open(path)?;
        let got = match mode {
            Mode::Exclusive => file.try_lock(),
            Mode::Shared => file.try_lock_shared(),
        };
        match got {
            Ok(()) => Ok(Some(FileLock { file, path: path.to_path_buf(), mode })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(io(path, "taking the lock of", e)),
        }
    }

    /// The lock of `path` in `mode`, waiting at most `bound` for another
    /// process to release it. At the bound, a refusal of class
    /// `Unavailable` that names the file.
    pub fn lock_within(path: &Path, mode: Mode, bound: Duration) -> Result<FileLock, Failure> {
        let deadline = Instant::now() + bound;
        loop {
            if let Some(lock) = FileLock::try_lock(path, mode)? {
                return Ok(lock);
            }
            if Instant::now() >= deadline {
                return Err(Failure::new(
                    Stage::Host,
                    Class::Unavailable,
                    format!(
                        "{}: another process holds its lock, and it was not released within {} ms",
                        path.display(),
                        bound.as_millis()
                    ),
                ));
            }
            std::thread::sleep(PAUSE.min(deadline.saturating_duration_since(Instant::now())));
        }
    }

    /// The path of the locked file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The mode of the lock.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The open file, for a caller that writes into it (a pid, a state).
    pub fn file(&self) -> &File {
        &self.file
    }
}

/// The file of a lock: read and write, made where absent, mode 0600 on
/// Unix, no link at the last component, close-on-exec (the default of
/// `std`).
fn open(path: &Path) -> Result<File, Failure> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(|e| io(path, "opening the lock file", e))
}

fn io(path: &Path, what: &str, e: std::io::Error) -> Failure {
    let class = Class::of_io(e.kind());
    Failure::caused(Stage::Host, class, format!("{what} {}: {e}", path.display()), e)
}

#[cfg(test)]
#[path = "lock_tests.rs"]
mod tests;
