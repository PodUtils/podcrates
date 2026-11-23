//! One atomic replace of a file at a path that the caller names, with a
//! durable commit (T-192, T-1066; PodUtils/podnet#437, PodUtils/podnet#677):
//! a temporary file beside the target, written and synced, renamed over the
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

pub use super::replace_mode::{Failed, Mode, Step};

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

/// Replace the file at `path` with `body`, atomically, with the mode `mode`:
/// [`Replace::new`] with no other option. A symbolic link at `path` is
/// replaced, not followed. The error names the step and the path
/// ([`Failed`]).
pub fn replace(path: &Path, body: &[u8], mode: Mode) -> io::Result<Commit> {
    Replace::new(mode).write(path, body)
}

/// The options of an atomic replace of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Replace {
    mode: Mode,
    follow_link: bool,
    make_dir: bool,
    in_place_fallback: bool,
}

impl Replace {
    /// A replace with the mode `mode`. A link at the path is replaced, not
    /// followed; a missing directory is an error; a failed rename is an
    /// error.
    pub const fn new(mode: Mode) -> Replace {
        Replace { mode, follow_link: false, make_dir: false, in_place_fallback: false }
    }

    /// With `true`, a symbolic link at the path is followed to its target
    /// (40 links at most), and the target is replaced: a config file that
    /// is a link into a dotfiles repository stays a link. A dangling link
    /// gives its target a new file.
    pub const fn follow_link(self, follow: bool) -> Replace {
        Replace { follow_link: follow, ..self }
    }

    /// With `true`, a missing directory of the target and each missing
    /// directory above it are made with mode 0700 ([`super::make_dirs`]),
    /// and synced with the file.
    pub const fn make_dir(self, make: bool) -> Replace {
        Replace { make_dir: make, ..self }
    }

    /// With `true`, a rename that fails (a bind-mounted file, as
    /// `/etc/resolv.conf` in a container) gives a write in place, which is
    /// never atomic ([`Commit::InPlace`]).
    pub const fn in_place_fallback(self, fallback: bool) -> Replace {
        Replace { in_place_fallback: fallback, ..self }
    }

    /// Replace the file at `path` with `body`. The error is an [`io::Error`]
    /// of the kind of its cause, whose text names the step and the path
    /// ("replace PATH: STEP: CAUSE"); [`Failed::of`] gives its parts.
    pub fn write(&self, path: &Path, body: &[u8]) -> io::Result<Commit> {
        self.checked(path, body, REAL, |_| Ok(())).map_err(io::Error::from)
    }

    /// [`Replace::write`] with a check of the new file before the rename:
    /// `check` gets the path of the temporary file, written, synced and
    /// closed (a program reads a key back and opens it). An error of
    /// `check` removes the temporary file and leaves the file at `path` as
    /// it was; the error is of the step [`Step::Check`], with the kind and
    /// the text of the error of `check`.
    pub fn write_checked<F>(&self, path: &Path, body: &[u8], check: F) -> io::Result<Commit>
    where
        F: FnOnce(&Path) -> io::Result<()>,
    {
        self.checked(path, body, REAL, check).map_err(io::Error::from)
    }

    fn run(&self, path: &Path, body: &[u8], hooks: Hooks) -> Result<Commit, Failed> {
        self.checked(path, body, hooks, |_| Ok(()))
    }

    fn checked<F>(&self, path: &Path, body: &[u8], hooks: Hooks, check: F) -> Result<Commit, Failed>
    where
        F: FnOnce(&Path) -> io::Result<()>,
    {
        let target = match self.follow_link {
            true => follow(path).map_err(|e| Failed::new(Step::FollowLink, path, e))?,
            false => path.to_path_buf(),
        };
        let at = |step: Step| {
            let target = &target;
            move |e: io::Error| Failed::new(step, target, e)
        };
        let (dir, name) = parts(&target).map_err(at(Step::Name))?;
        let made = match self.make_dir {
            true => super::make_dirs(&dir).map_err(at(Step::MakeDir))?,
            false => Vec::new(),
        };
        let mode = self.mode.bits(&target).map_err(at(Step::ReadMode))?;
        let tmp = (hooks.temp)(&dir, &name);
        // A file at the temporary name is not this replace's: it stays.
        let mut file = create(&tmp, mode).map_err(at(Step::Create))?;
        let written = (hooks.write)(&mut file, body)
            .map_err(at(Step::Write))
            .and_then(|()| file.sync_all().map_err(at(Step::Sync)));
        drop(file);
        let written = written.and_then(|()| check(&tmp).map_err(at(Step::Check)));
        if let Err(f) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(f);
        }
        match (hooks.rename)(&tmp, &target) {
            Ok(()) => {
                // The entry of each directory made, and the entry of the file.
                let parents = made.iter().filter_map(|d| d.parent());
                let synced = parents.chain(Some(dir.as_path())).try_for_each(sync_dir);
                Ok(synced.map_or_else(Commit::InstalledUnconfirmed, |()| Commit::Durable))
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                if !self.in_place_fallback {
                    return Err(at(Step::Rename)(e));
                }
                write_in_place(&target, body).map_err(at(Step::InPlace))?;
                Ok(Commit::InPlace(e))
            }
        }
    }
}

/// Replace `path` with `body`, with the mode `mode` (Unix; on Windows a mode
/// with no bit for others gives a private file): [`Replace`] with
/// [`Mode::Exact`], and the error of the failed step as it came. A symlink
/// at `path` is replaced, not followed. With `in_place_fallback`, a rename
/// that fails (a bind-mounted file, as `/etc/resolv.conf` in a container)
/// gives a write in place, which is never atomic ([`Commit::InPlace`]).
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
    let options = Replace::new(Mode::Exact(mode)).in_place_fallback(in_place_fallback);
    options.run(path, body, Hooks { rename, ..REAL }).map_err(Failed::into_cause)
}

/// The parts of a replace that a test changes.
#[derive(Clone, Copy)]
struct Hooks {
    rename: fn(&Path, &Path) -> io::Result<()>,
    temp: fn(&Path, &str) -> PathBuf,
    write: fn(&mut std::fs::File, &[u8]) -> io::Result<()>,
}

const REAL: Hooks = Hooks { rename, temp: temp_name, write: |file, body| file.write_all(body) };

/// A name in `dir` that no other replace of this process takes, and that a
/// replace of another process takes only with the same pid.
fn temp_name(dir: &Path, name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{name}.{}.{unique}.tmp", std::process::id()))
}

/// The target of the links at `path`, one link after the other; `path`
/// itself when it is no link. A relative link is relative to its directory.
fn follow(path: &Path) -> io::Result<PathBuf> {
    const MOST: usize = 40;
    let mut at = path.to_path_buf();
    for _ in 0..MOST {
        match std::fs::symlink_metadata(&at) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let to = std::fs::read_link(&at)?;
                at = match at.parent() {
                    Some(dir) if to.is_relative() => dir.join(to),
                    _ => to,
                };
            }
            Ok(_) => return Ok(at),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(at),
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, format!("more than {MOST} symbolic links")))
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

/// A new file at `path` with the mode `mode` before its first byte: never
/// one that exists, never through a link. A file that it made and could not
/// give its mode is removed.
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
    if let Err(e) = file.set_permissions(std::fs::Permissions::from_mode(mode)) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
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
