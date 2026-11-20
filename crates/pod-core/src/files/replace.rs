//! One replace of a file with a durable commit (T-192, PodUtils/podnet#437):
//! a new file beside the target, written and synced, renamed over the
//! target, then the directory synced, so that a power cut leaves the old
//! file or the new one whole, never an empty one under the name.
//!
//! The outcomes. `Ok(Commit::Durable)`: the new file is in place and on
//! disk. `Ok(Commit::InstalledUnconfirmed(e))`: the new file is in place,
//! but the sync of its directory failed; a caller writes one warning, never
//! a failure. `Ok(Commit::InPlace(e))`: the rename failed and the caller
//! asked for a write in place, which is not atomic. `Err`: nothing changed at
//! the path, and no temporary file stays.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// What a replace that put the new file in place could prove.
#[derive(Debug)]
pub enum Commit {
    /// The file and its directory entry are on disk.
    Durable,
    /// The file is in place; the sync of its directory failed with this
    /// error, so the new name may not survive a power cut.
    InstalledUnconfirmed(io::Error),
    /// The rename failed with this error (a bind-mounted file), and the file
    /// was written in place and synced: a reader could see it half written.
    InPlace(io::Error),
}

impl Commit {
    /// The error of an unconfirmed commit.
    pub fn unconfirmed(&self) -> Option<&io::Error> {
        match self {
            Commit::Durable => None,
            Commit::InstalledUnconfirmed(e) | Commit::InPlace(e) => Some(e),
        }
    }
}

/// Replace `path` with `body`, with the mode `mode` (Unix; on Windows a mode
/// with no bit for others gives a private file). A symlink at `path` is
/// replaced, not followed. With `in_place_fallback`, a rename that fails
/// (a bind-mounted file, as `/etc/resolv.conf` in a container) gives a write
/// in place, which is never atomic ([`Commit::InPlace`]).
pub fn replace_private(path: &Path, body: &[u8], mode: u32, in_place_fallback: bool) -> io::Result<Commit> {
    replace_with(path, body, mode, in_place_fallback, rename)
}

/// [`replace_private`] with the rename `rename` (a test fails it).
fn replace_with(
    path: &Path,
    body: &[u8],
    mode: u32,
    in_place_fallback: bool,
    rename: fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<Commit> {
    let (dir, name) = parts(path)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{unique}.tmp", std::process::id()));
    let written = (|| {
        let mut file = create(&tmp, mode)?;
        file.write_all(body)?;
        file.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    match rename(&tmp, path) {
        Ok(()) => Ok(sync_dir(&dir).map_or_else(Commit::InstalledUnconfirmed, |()| Commit::Durable)),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            if !in_place_fallback {
                return Err(e);
            }
            write_in_place(path, body)?;
            Ok(Commit::InPlace(e))
        }
    }
}

/// The directory of `path` and its file name.
fn parts(path: &Path) -> io::Result<(PathBuf, String)> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{} has no file name", path.display())))?;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    Ok((dir, name.to_string()))
}

#[cfg(unix)]
fn create(path: &Path, mode: u32) -> io::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    // The umask can remove bits: set the mode that was asked, which is no
    // wider than it.
    file.set_permissions(std::fs::Permissions::from_mode(mode))?;
    Ok(file)
}

#[cfg(not(unix))]
fn create(path: &Path, mode: u32) -> io::Result<std::fs::File> {
    if mode & 0o077 == 0 { super::create_new_private(path) } else { super::create_new_public(path) }
}

#[cfg(not(windows))]
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// A rename with POSIX semantics where the file system takes it, which
/// returns when the rename is on disk.
#[cfg(windows)]
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    win::rename(from, to)
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// The rename wrote through: there is no directory to sync.
#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// The file at `path` truncated and written, then synced; a symlink is
/// not followed.
fn write_in_place(path: &Path, body: &[u8]) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NOFOLLOW);
    let mut file = options.open(path)?;
    file.write_all(body)?;
    file.sync_all()
}

#[cfg(windows)]
#[path = "replace_win.rs"]
mod win;

#[cfg(test)]
#[path = "replace_tests.rs"]
mod tests;
