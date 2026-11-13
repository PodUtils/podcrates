//! The files of a WARP identity: the check of the owner and of the mode,
//! the write of a private file, and the lock of a registration. The crate
//! chooses no path: each path is the caller's.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};

/// The consent of the caller to read an identity that another user owns
/// (PodUtils/podnet#413, ask 9; podnet's `PODNET_FOREIGN_IDENTITY=allow`).
/// The caller gives [`Foreign::Allow`] only for a path that the user named:
/// never for a default path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Foreign {
    /// Refuse a file of another user: a configuration error (exit 78).
    #[default]
    Refuse,
    /// Read a file of another user. The check of the mode stays, and the
    /// crate writes nothing to that file.
    Allow,
}

/// The user of this process.
#[cfg(unix)]
pub fn this_uid() -> u32 {
    // SAFETY: geteuid has no precondition and cannot fail.
    unsafe { libc::geteuid() }
}

/// Refuse `path` when it is a directory, when its owner is another user
/// (with no consent), or when its group or other users may read or write
/// it. True when the owner is another user and the caller allowed it.
pub fn check_private(path: &Path, foreign: Foreign) -> Result<bool, Failure> {
    #[cfg(unix)]
    {
        check_private_as(path, this_uid(), foreign)
    }
    #[cfg(not(unix))]
    {
        let _ = foreign;
        let meta = std::fs::metadata(path).map_err(|e| read_failure(path, e))?;
        if meta.is_dir() {
            return Err(directory(path));
        }
        Ok(false)
    }
}

fn read_failure(path: &Path, e: std::io::Error) -> Failure {
    let text = format!("reading the WARP identity from {}", path.display());
    Failure::caused(Stage::Warp, Class::of_io(e.kind()), text, e)
}

fn directory(path: &Path) -> Failure {
    let text = format!("{} is a directory; give the identity file", path.display());
    Failure::new(Stage::Warp, Class::Usage, text)
}

/// [`check_private`] for the user `uid`. An owner that is another user is a
/// configuration error (PodUtils/podnet@T-173) with no consent: no
/// registration follows it.
#[cfg(unix)]
pub(crate) fn check_private_as(path: &Path, uid: u32, foreign: Foreign) -> Result<bool, Failure> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).map_err(|e| read_failure(path, e))?;
    if meta.is_dir() {
        return Err(directory(path));
    }
    let other = meta.uid() != uid;
    if other && foreign == Foreign::Refuse {
        let text = format!(
            "{} belongs to user {}, not to this user ({uid}); it is not used. Run as that user, or give an identity file of this user",
            path.display(),
            meta.uid()
        );
        return Err(Failure::new(Stage::Warp, Class::Config, text));
    }
    let mode = meta.mode() & 0o777;
    if mode & 0o077 != 0 {
        let text = format!(
            "{} has mode {mode:04o}: other users may read the WARP key. Run: chmod 600 {}",
            path.display(),
            path.display()
        );
        return Err(Failure::new(Stage::Warp, Class::Config, text));
    }
    Ok(other)
}

/// Write `bytes` to `path` with mode 0600: to a new file beside it, then a
/// rename. No other user can read the file at any time, and a reader never
/// sees half a file. A missing parent is made with mode 0700.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let mut dirs = std::fs::DirBuilder::new();
        dirs.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut dirs, 0o700);
        dirs.create(parent)?;
    }
    let name = path.file_name().ok_or_else(|| std::io::Error::other("the path has no file name"))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    let result = (|| {
        let mut file = pod_core::files::create_new_private(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The lock file of `path`: `PATH.lock`.
pub fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

/// A held lock: the lock ends when it is dropped (the file closes).
#[derive(Debug)]
pub struct Held {
    _file: std::fs::File,
}

/// Take the lock of `path` (a file `PATH.lock` of mode 0600 that stays
/// after use), and wait at most `wait` for another process. A lock that
/// stays held is `TempFail` (exit 75).
pub async fn lock(path: &Path, wait: Duration) -> Result<Held, Failure> {
    let file = lock_path(path);
    let end = tokio::time::Instant::now() + wait;
    loop {
        if let Some(held) = try_lock(&file)? {
            return Ok(held);
        }
        if tokio::time::Instant::now() >= end {
            let text = format!("another process is registering a WARP device (the lock {} is held)", file.display());
            return Err(Failure::new(Stage::Warp, Class::TempFail, text));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub(crate) fn try_lock(file: &Path) -> Result<Option<Held>, Failure> {
    let failure = |e: std::io::Error| {
        let text = format!("the lock {}", file.display());
        Failure::caused(Stage::Warp, Class::of_io(e.kind()), text, e)
    };
    if let Some(parent) = file.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(failure)?;
    }
    let mut open = std::fs::OpenOptions::new();
    open.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    let handle = open.open(file).map_err(failure)?;
    match handle.try_lock() {
        Ok(()) => Ok(Some(Held { _file: handle })),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(failure(e)),
    }
}

#[cfg(all(test, unix))]
mod tests;
