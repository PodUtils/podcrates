//! Small private files that a program keeps between runs, and files that a
//! user names: each file is read only when it is a regular file of this
//! user that nobody else can read (as ssh reads a key), and is written
//! through a private temporary file and a rename.
//!
//! The places of the files come from the caller's [`App`]: the directory
//! that its variable names, the user's cache directory, the user's runtime
//! directory, the temporary directory, and the working directory
//! ([`places_from`]); `none` keeps no file. A usual place in another user's
//! tree is passed over, and only read. The crate reads no variable itself:
//! the caller gives a lookup of its environment.

use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(any(windows, test))]
mod closed;
#[cfg(windows)]
mod closed_win;
mod dirs;
mod own;
#[cfg(unix)]
mod owner;
#[cfg(unix)]
pub use dirs::places_with;
pub use dirs::{App, Chosen, Passed, Places, candidate_dirs_from, chosen_from, places_from};
pub use own::{Others, create_file_in_first, create_private, read_own};
#[cfg(unix)]
pub use owner::{Host, make_dirs as make_dirs_with, passwd_home};

/// The largest cache file read (a pool of a few hundred host names fits).
/// A larger one is not read: a cache file is ignored, and made again.
pub const MAX_FILE: u64 = 64 * 1024;

/// The contents of the private file `name`, from the first directory that has
/// a trusted one.
pub fn load_file_from(dirs: &[PathBuf], name: &str) -> Option<String> {
    dirs.iter().find_map(|dir| read_trusted(&dir.join(name)))
}

/// Save `body` as the private file `name` in the first directory that accepts
/// it.
pub fn store_file_in_first(dirs: &[PathBuf], name: &str, body: &[u8]) -> Result<PathBuf, String> {
    store_noting(dirs, name, body).map(|stored| stored.path)
}

/// Where a file went, and why each directory before it refused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub path: PathBuf,
    /// `DIR: reason`, for each directory that refused the file first.
    pub refused: Vec<String>,
}

/// [`store_file_in_first`], with the refusals of the directories that came
/// first: a directory that the user named and that refused says why.
pub fn store_noting(dirs: &[PathBuf], name: &str, body: &[u8]) -> Result<Stored, String> {
    if dirs.is_empty() {
        return Err(format!("the cache is off (none), so no file such as {name} is kept"));
    }
    let mut refused = Vec::new();
    for dir in dirs {
        match write_private(dir, name, body) {
            Ok(path) => return Ok(Stored { path, refused }),
            Err(e) => refused.push(format!("{}: {e}", dir.display())),
        }
    }
    Err(format!("no directory would take {name} ({})", refused.join("; ")))
}

/// What to say of the variable `dir_env` once a file went to `stored`: a
/// relative path that was not used, or the named directory's refusal.
/// Nothing when the choice was followed, or there was none.
pub fn chosen_note(dir_env: &str, chosen: &Chosen, stored: &Stored) -> Option<String> {
    match chosen {
        Chosen::Relative(value) => Some(format!(
            "{dir_env}={value} is not an absolute path, so it was not used; the file went to {}",
            stored.path.display()
        )),
        Chosen::Dir(dir) if !stored.path.starts_with(dir) => Some(format!(
            "{dir_env} could not be used ({}); the file went to {}",
            stored.refused.first().map_or("refused", String::as_str),
            stored.path.display()
        )),
        _ => None,
    }
}

/// The names of the files that start with `prefix` and end with `suffix`, in
/// each directory, that are ours as [`load_file_from`] takes them; sorted, once
/// each.
pub fn names_from(dirs: &[PathBuf], prefix: &str, suffix: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else { continue };
            let fits = name.len() > prefix.len() + suffix.len() && name.starts_with(prefix) && name.ends_with(suffix);
            if fits && !names.contains(&name) && read_trusted(&entry.path()).is_some() {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

/// A private file that the user names, such as a pair file: read only when it
/// is a regular file of this user that nobody else can read, as ssh reads a
/// key. A symbolic link that the user gives is followed.
pub fn read_private(path: &Path) -> Result<String, String> {
    read_private_up_to(path, MAX_FILE)
}

/// [`read_private`] of a file that may hold `most` bytes: a role's own file
/// of pairs or of counts, which a full role makes larger than a file of the
/// cache.
pub fn read_private_up_to(path: &Path, most: u64) -> Result<String, String> {
    let shown = path.display();
    let file = std::fs::File::open(path).map_err(|e| format!("{shown}: {e}"))?;
    let meta = file.metadata().map_err(|e| format!("{shown}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{shown} is not a regular file"));
    }
    if meta.len() > most {
        return Err(match most {
            MAX_FILE => format!("{shown} is larger than a file of the cache can be"),
            _ => format!("{shown} holds more than {} KiB", most >> 10),
        });
    }
    closed(path, &file, Others::NoAccess)?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut &file, &mut text).map_err(|e| format!("{shown}: {e}"))?;
    Ok(text)
}

/// Delete the private file `name` from each directory where it is ours: a
/// file that another user owns, or that others can read, is not touched.
pub fn remove_named_from(dirs: &[PathBuf], name: &str) {
    for dir in dirs {
        let path = dir.join(name);
        if read_trusted(&path).is_some() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// `host` lower-cased, with anything but `[a-z0-9.-]` replaced, for file names.
pub fn safe_name(host: &str) -> String {
    host.to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect()
}

/// Read a cache file only if it is a regular file of ours that nobody else can
/// read; anything else is ignored, as a cache file can be made again.
pub fn read_trusted(path: &Path) -> Option<String> {
    read_own(path, Others::NoAccess).ok().flatten()
}

/// Write `body` to `dir/name` via a private temporary file and a rename, so a
/// reader never sees a half-written file and the file is never readable by
/// others, not even for a moment.
fn write_private(dir: &Path, name: &str, body: &[u8]) -> std::io::Result<PathBuf> {
    make_private_dir(dir)?;
    let final_path = dir.join(name);
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let result = (|| {
        let mut file = create_new_private(&tmp)?;
        file.write_all(body)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, &final_path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| final_path)
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path)
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> std::io::Result<std::fs::File> {
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "a symlink"));
    }
    std::fs::File::open(path)
}

#[cfg(unix)]
/// A new file that only its owner can read (mode 0600), never one that exists
/// and never through a symlink.
pub fn create_new_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(path)
}

/// A new file that only its owner, SYSTEM and the Administrators may open
/// (an access list of its own), never one that exists.
#[cfg(windows)]
pub fn create_new_private(path: &Path) -> std::io::Result<std::fs::File> {
    closed_win::create_new_private(path)
}

#[cfg(not(any(unix, windows)))]
pub fn create_new_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(unix)]
/// A new file that each may read and only its owner may change (mode 0644),
/// never one that exists and never through a symlink.
pub fn create_new_public(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).custom_flags(libc::O_NOFOLLOW).open(path)
}

/// A new file that each may read and only its owner, SYSTEM and the
/// Administrators may change, as OpenSSH for Windows makes a `.pub` file,
/// never one that exists.
#[cfg(windows)]
pub fn create_new_public(path: &Path) -> std::io::Result<std::fs::File> {
    closed_win::create_new_public(path)
}

#[cfg(not(any(unix, windows)))]
pub fn create_new_public(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

/// Make `dir` a private directory of this user, as the cache's are: for the
/// tests of each crate, whose scratch directories hold files that the crates
/// check.
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    make_private_dir(dir)
}

/// Make `dir` and each missing directory above it, as the cache makes its
/// own (on Unix, one above it only in a directory of this user's or root's):
/// the directories made, the highest first, for a check that removes them
/// again.
pub fn make_dirs(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    #[cfg(unix)]
    {
        owner::make_dirs(dir, &owner::Host::this())
    }
    #[cfg(not(unix))]
    {
        let missing: Vec<PathBuf> = dir.ancestors().take_while(|at| !at.exists()).map(Path::to_path_buf).collect();
        std::fs::create_dir_all(dir)?;
        Ok(missing.into_iter().rev().collect())
    }
}

/// Create `dir` (mode 0700) if needed and make sure it is a real directory of
/// ours that nobody else can write to.
#[cfg(unix)]
fn make_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    owner::make_dirs(dir, &owner::Host::this())?;
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() || meta.uid() != euid() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the directory is a symlink or belongs to another user",
        ));
    }
    if meta.mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// `dir` made, and given an access list of its own when its list lets
/// anyone else in.
#[cfg(windows)]
fn make_private_dir(dir: &Path) -> std::io::Result<()> {
    closed_win::make_private_dir(dir)
}

#[cfg(not(any(unix, windows)))]
fn make_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// Whether `file`, opened at `path`, is this user's and closed to the other
/// users beyond `others`: the refusal, with its remedy, when it is not. On
/// Unix the owner and the mode say it; on Windows the owner and the access
/// list, as OpenSSH for Windows reads them.
pub fn closed(path: &Path, file: &std::fs::File, others: Others) -> Result<(), String> {
    let shown = path.display();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata().map_err(|e| format!("{shown}: {e}"))?;
        let (open, why) = match others {
            Others::NoAccess => (0o077, "others can read it; make it private (chmod 600)"),
            Others::Read => (0o022, "others can change it; chmod go-w"),
        };
        if meta.uid() != euid() || meta.mode() & open != 0 {
            return Err(format!("{shown} is another user's, or {why}"));
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        closed_win::closed(file, others).map_err(|why| {
            format!("{shown}: {why}; make it private: icacls \"{shown}\" /inheritance:r /grant:r \"%USERNAME%:F\"")
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (shown, file, others);
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}
