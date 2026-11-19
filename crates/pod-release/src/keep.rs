//! The binary before an update, kept beside the new one: `NAME.prev`
//! (`NAME.prev.exe` on Windows), the file to go back to when the new one
//! fails on this host. [`rollback`] puts it back, once it ran here, and
//! keeps the one that it replaces in its place, so that a second rollback
//! goes forward again. [`fallback`] names it for a supervisor that starts a
//! run again, with no file changed.
//!
//! The base is podssh's `update/keep.rs`; the check of the kept file after
//! its trial run is podnet's `rollback`. The lines that tell a user the way
//! back (podssh's `way_back`) name the consumer's own command, and stay
//! with the consumer.

use std::fs;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::replace::{Written, beside, name_parts, shown};
use crate::trial::{self, Expect};

/// Where the binary before an update is kept, beside `exe`: its name and
/// `.prev`, before `.exe`, so that Windows still runs it.
pub fn prev_of(exe: &Path) -> PathBuf {
    let (stem, ext) = name_parts(exe);
    exe.with_file_name(format!("{stem}.prev{ext}"))
}

/// The file at `exe` kept as `prev`, in one rename over an older one: a
/// hard link to the same file, which costs no space and no write, else a
/// copy, with its mode. Windows moves the running file aside instead.
pub fn keep(exe: &Path, prev: &Path) -> Result<PathBuf, String> {
    let temp = beside(exe, "prev")?;
    let made = fs::hard_link(exe, &temp).or_else(|_| fs::copy(exe, &temp).map(|_| ()));
    if let Err(e) = made {
        let _ = fs::remove_file(&temp);
        return Err(format!("{}: {e}", shown(&temp)));
    }
    if let Err(e) = fs::rename(&temp, prev) {
        let _ = fs::remove_file(&temp);
        return Err(format!("{}: {e}", shown(prev)));
    }
    Ok(prev.to_path_buf())
}

/// The binary that the last update kept beside `exe`, when it is there and
/// answers `--version` as `expect` asks: what the supervisor of a detached
/// run starts after the new binary failed to start again and again. It
/// changes no file: [`rollback`] does, when a person asks.
pub fn fallback(exe: &Path, expect: &Expect<'_>) -> Option<PathBuf> {
    let prev = prev_of(exe);
    (prev.is_file() && trial::check(&prev, expect).is_ok()).then_some(prev)
}

/// What a rollback did.
#[derive(Debug)]
pub struct RolledBack {
    /// What the binary now in place said when it ran here.
    pub said: String,
    /// The binary that it replaced, now kept as `NAME.prev`; or why it is
    /// not kept.
    pub kept: Result<PathBuf, String>,
}

/// The kept binary back at `exe`, once it ran here as `expect` asks and
/// did not change after its run, and the one that it replaces kept in its
/// place. No network.
pub fn rollback(exe: &Path, expect: &Expect<'_>) -> Result<RolledBack, Error> {
    let prev = prev_of(exe);
    if !prev.is_file() {
        return Err(Error::NotFound(format!("there is no {}: no update kept a binary before this one", shown(&prev))));
    }
    let refused =
        |why: String| Error::Refused(format!("{} does not run here: {why}; nothing was changed", shown(&prev)));
    let id = Written::at(&prev).map_err(refused)?;
    let said = trial::check(&prev, expect).map_err(refused)?;
    id.still(&prev).map_err(refused)?;
    let kept = swap(exe, &prev)?;
    Ok(RolledBack { said, kept })
}

/// `prev` in `exe`'s place, and `exe`'s file kept as `prev`: an error when
/// `exe` is as it was; else whether the second part held.
fn swap(exe: &Path, prev: &Path) -> Result<Result<PathBuf, String>, Error> {
    let cant = |why: String| Error::CantWrite(format!("{why}; {} is as it was", shown(exe)));
    let aside = beside(exe, "rollback").map_err(cant)?;
    if cfg!(windows) {
        // A running program may be renamed on Windows, not replaced.
        fs::rename(exe, &aside).map_err(|e| cant(format!("{} aside: {e}", shown(exe))))?;
        if let Err(e) = fs::rename(prev, exe) {
            let _ = fs::rename(&aside, exe);
            return Err(cant(format!("{}: {e}", shown(prev))));
        }
    } else {
        let made = fs::hard_link(exe, &aside).or_else(|_| fs::copy(exe, &aside).map(|_| ()));
        if let Err(e) = made {
            let _ = fs::remove_file(&aside);
            return Err(cant(format!("{}: {e}", shown(&aside))));
        }
        if let Err(e) = fs::rename(prev, exe) {
            let _ = fs::remove_file(&aside);
            return Err(cant(format!("{}: {e}", shown(prev))));
        }
    }
    sync_dir(exe);
    Ok(fs::rename(&aside, prev)
        .map(|()| prev.to_path_buf())
        .map_err(|e| format!("{}: {e}; it is {}", shown(prev), shown(&aside))))
}

/// The renames of `path`'s directory on the disk, where the system lets a
/// directory be opened; they are made either way.
pub fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent()
        && let Ok(dir) = fs::File::open(dir)
    {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests;
