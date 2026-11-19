//! The new binary in place of the running one: written beside it, in its
//! directory, so that a rename moves it; its mode copied; run once there
//! ([`crate::trial`]), so that a file that does not start on this host
//! never takes the old one's place; checked again after its run, so that a
//! file that another process put at its name in the meantime is not
//! installed; then renamed over the running file, which keeps running from
//! the old inode, while the old file stays as `NAME.prev`
//! ([`crate::keep`]). Windows refuses to write over a running program and
//! allows its rename, so there the running file is renamed to
//! `NAME.prev.exe` first, else to `NAME.exe.old`, which a later run removes
//! ([`remove_aside`]). A failure at any step leaves the old binary as it
//! was, and removes what this step wrote.
//!
//! The base is podssh's `update/replace.rs`; the check of the file after
//! its trial run is podnet's `same_file` (its issue 236).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::keep;

/// The path of the running binary, through its links: the file that an
/// update replaces. Linux names the file of a binary that an update
/// replaced while it ran `PATH (deleted)`: its path is still the file.
pub fn running() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("the path of this binary: {e}"))?;
    fs::canonicalize(&exe).or_else(|e| {
        let text = exe.to_string_lossy();
        match text.strip_suffix(" (deleted)") {
            Some(path) => fs::canonicalize(path).map_err(|e| format!("{path}: {e}")),
            None => Err(format!("{}: {e}", exe.display())),
        }
    })
}

/// `path` as a user reads it: on Windows without the `\\?\` that the
/// canonical form of a path on a drive begins with.
pub fn shown(path: &Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => text,
    }
}

/// Where a running binary is moved on Windows when its kept place refuses
/// it: `NAME.exe.old`.
pub fn aside(exe: &Path) -> PathBuf {
    let mut name = exe.as_os_str().to_owned();
    name.push(".old");
    PathBuf::from(name)
}

/// The old binary that an update on Windows moved aside, removed once it
/// runs no more: a consumer calls this at the start of each run. Elsewhere
/// there is never one. The kept one, `NAME.prev.exe`, stays.
pub fn remove_aside(exe: &Path) {
    if cfg!(windows) {
        let _ = fs::remove_file(aside(exe));
    }
}

/// A temporary name beside `exe` for the step `what`, which no other run
/// takes: `.NAME.WHAT-RANDOM`, and `.exe` after it where `exe` has it, so
/// that Windows runs it as a program.
pub fn beside(exe: &Path, what: &str) -> Result<PathBuf, String> {
    let dir = exe.parent().ok_or_else(|| format!("{} has no directory", exe.display()))?;
    let (stem, ext) = name_parts(exe);
    Ok(dir.join(format!(".{stem}.{what}-{:016x}{ext}", rand::random::<u64>())))
}

/// `exe`'s name without `.exe`, and `.exe` when it has it: a dot in another
/// name, as in `podssh-1.0.0`, is no extension of a program.
pub fn name_parts(exe: &Path) -> (String, &'static str) {
    let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "binary".into());
    let is_exe = exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    match (is_exe, exe.file_stem()) {
        (true, Some(stem)) => (stem.to_string_lossy().into_owned(), ".exe"),
        _ => (name, ""),
    }
}

/// `bytes` in a new file at `path`, on the disk before this returns; the
/// identity of the file.
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<Written> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o700);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Written::of(&file.metadata()?)
}

/// The identity of a file before its trial run: its device and its inode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Written {
    #[cfg(unix)]
    id: (u64, u64),
}

impl Written {
    /// The identity of the file at `path`, not of a symlink's target.
    pub(crate) fn at(path: &Path) -> Result<Written, String> {
        let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", shown(path)))?;
        Written::of(&meta).map_err(|e| format!("{}: {e}", shown(path)))
    }

    #[cfg(unix)]
    fn of(meta: &fs::Metadata) -> std::io::Result<Written> {
        use std::os::unix::fs::MetadataExt;
        Ok(Written { id: (meta.dev(), meta.ino()) })
    }

    #[cfg(not(unix))]
    fn of(_meta: &fs::Metadata) -> std::io::Result<Written> {
        Ok(Written {})
    }

    /// `path` is still the file of the trial: a regular file (not a
    /// symlink) of this user that other users cannot write. A check by the
    /// path leaves a short window before the rename; a process that can
    /// write the directory can also rename its own file over the binary, so
    /// the check is for a file of this user in a shared directory.
    #[cfg(unix)]
    pub(crate) fn still(&self, path: &Path) -> Result<(), String> {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", shown(path)))?;
        // SAFETY: geteuid takes no argument and cannot fail.
        let uid = unsafe { libc::geteuid() };
        if !meta.file_type().is_file()
            || (meta.dev(), meta.ino()) != self.id
            || meta.uid() != uid
            || meta.mode() & 0o022 != 0
        {
            return Err(format!("{} changed after its trial run", shown(path)));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    pub(crate) fn still(&self, _path: &Path) -> Result<(), String> {
        Ok(())
    }
}

/// What a replacement did.
#[derive(Debug)]
pub struct Replaced {
    /// What the new binary said when it ran here.
    pub said: String,
    /// Where the binary before it is kept, or why it is not.
    pub prev: Result<PathBuf, String>,
}

/// Why a replacement stopped, the old binary as it was.
#[derive(Debug)]
pub enum Stopped {
    /// The new file did not run here, named another version, or changed
    /// after its run.
    Trial(String),
    /// A step of the disk failed.
    Disk(String),
}

/// The file at `exe` becomes `bytes`, with `exe`'s mode, once `trial`
/// passed on the new file; the file before it is kept beside it.
pub fn replace(exe: &Path, bytes: &[u8], trial: &dyn Fn(&Path) -> Result<String, String>) -> Result<Replaced, Stopped> {
    let old = fs::metadata(exe).map_err(|e| Stopped::Disk(format!("{}: {e}", shown(exe))))?;
    let temp = beside(exe, "update").map_err(Stopped::Disk)?;
    let dir = temp.parent().map(Path::to_path_buf).unwrap_or_default();
    let written = write_new(&temp, bytes).map_err(|e| {
        let _ = fs::remove_file(&temp);
        Stopped::Disk(format!("a new file in {}: {e}", shown(&dir)))
    })?;
    let undo = |why: String| {
        let _ = fs::remove_file(&temp);
        why
    };
    #[cfg(unix)]
    {
        // The mode of the old file, with no write for the group or others
        // (a binary that others can change is not installed) and no
        // set-user-ID or set-group-ID bit.
        use std::os::unix::fs::PermissionsExt;
        let mode = old.permissions().mode() & 0o755;
        fs::set_permissions(&temp, fs::Permissions::from_mode(mode))
            .map_err(|e| Stopped::Disk(undo(format!("the mode of {}: {e}", shown(&temp)))))?;
    }
    #[cfg(not(unix))]
    let _ = old;
    // Run where it will run, before anything of the old one moves.
    let said = trial(&temp).map_err(|why| Stopped::Trial(undo(why)))?;
    written.still(&temp).map_err(|why| Stopped::Trial(undo(why)))?;
    let prev = keep::prev_of(exe);
    let kept = if cfg!(windows) {
        let _ = fs::remove_file(&prev);
        let (moved, kept) = match fs::rename(exe, &prev) {
            Ok(()) => (prev.clone(), Ok(prev.clone())),
            // A kept binary that runs, as a supervisor may start one, stays.
            Err(e) => {
                let moved = aside(exe);
                let _ = fs::remove_file(&moved);
                fs::rename(exe, &moved).map_err(|e| Stopped::Disk(undo(format!("{} aside: {e}", shown(exe)))))?;
                (moved, Err(format!("{}: {e}", shown(&prev))))
            }
        };
        if let Err(e) = fs::rename(&temp, exe) {
            let _ = fs::rename(&moved, exe);
            return Err(Stopped::Disk(undo(format!("{}: {e}", shown(exe)))));
        }
        kept
    } else {
        // Before the rename, which unlinks the old file.
        let kept = keep::keep(exe, &prev);
        fs::rename(&temp, exe).map_err(|e| Stopped::Disk(undo(format!("{}: {e}", shown(exe)))))?;
        kept
    };
    keep::sync_dir(exe);
    Ok(Replaced { said, prev: kept })
}

#[cfg(test)]
mod tests;
