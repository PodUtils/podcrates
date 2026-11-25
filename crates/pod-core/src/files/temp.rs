//! A new temporary directory and a new temporary file with a random name
//! (T-1343, PodUtils/podnet#711): never a path that is there, never a name
//! from the pid or the clock alone.
//!
//! In a PID namespace two processes that share a directory can have the
//! same pid, so a name from the pid (and a counter) can be the name of
//! another process. Each name here has 16 hex digits from the random
//! source of the system, and the create is exclusive: `mkdir(2)`, or
//! `O_CREAT | O_EXCL` with `O_NOFOLLOW`. A name that is there gives a new
//! name, up to [`NAMES`] tries; a path that is there is never opened,
//! changed or removed.

use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::entropy::{Entropy, OsEntropy};

/// At most this many names are tried for one path.
pub(super) const NAMES: usize = 8;

/// The number of hex digits of the random part of a name.
pub(super) const RANDOM_DIGITS: usize = 16;

/// A new directory `PREFIX-RANDOM` in `parent`, with mode 0700 (on
/// Windows, an access list of the user, SYSTEM and the Administrators
/// alone). `RANDOM` is 16 hex digits from the random source of the system.
/// A name that is there gives a new name, up to 8 tries; then the error
/// has the kind `AlreadyExists`. A prefix with a path separator, a NUL byte
/// or `..` is `InvalidInput`.
pub fn new_temp_dir(parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    new_named(parent, prefix, "", &mut random_part, &mut create_dir).map(|(path, ())| path)
}

/// A new file `PREFIX-RANDOMSUFFIX` in `parent`, opened for writing, with
/// mode 0600 (on Windows, an access list of the user, SYSTEM and the
/// Administrators alone). The name follows the rules of [`new_temp_dir`];
/// a suffix with a path separator, a NUL byte or `..` is `InvalidInput`.
/// A symbolic link at the name is not followed: the name is taken as there.
pub fn new_temp_file(parent: &Path, prefix: &str, suffix: &str) -> io::Result<(PathBuf, std::fs::File)> {
    new_named(parent, prefix, suffix, &mut random_part, &mut create_file)
}

/// The first path `parent/PREFIX-RANDOMSUFFIX` that `create` makes, with a
/// new `RANDOM` from `random` after each `AlreadyExists`, up to [`NAMES`]
/// tries. Another error of `create` ends the tries.
pub(super) fn new_named<T>(
    parent: &Path,
    prefix: &str,
    suffix: &str,
    random: &mut dyn FnMut() -> String,
    create: &mut dyn FnMut(&Path) -> io::Result<T>,
) -> io::Result<(PathBuf, T)> {
    check_part("prefix", prefix)?;
    check_part("suffix", suffix)?;
    let mut last = None;
    for _ in 0..NAMES {
        let path = parent.join(format!("{prefix}-{}{suffix}", random()));
        match create(&path) {
            Ok(made) => return Ok((path, made)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    let why = last.map(|e| format!(": {e}")).unwrap_or_default();
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{}: each of {NAMES} names {prefix}-RANDOM{suffix} was there{why}", parent.display()),
    ))
}

/// A part of a name that cannot leave the directory or cut the name.
fn check_part(what: &str, part: &str) -> io::Result<()> {
    if part.contains(std::path::is_separator) || part.contains('\0') || part.contains("..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the {what} {part:?} has a path separator, a NUL byte or \"..\""),
        ));
    }
    Ok(())
}

/// The random part of a name: [`RANDOM_DIGITS`] hex digits.
pub(super) fn random_part() -> String {
    random_from(&mut OsEntropy)
}

/// The random part from `source`. When the source gives no bytes (a
/// seccomp filter), the keys of the standard hasher with the pid, the
/// clock and a counter mixed in: the create is exclusive, so such a name is
/// never taken over, it can only be a name that is there.
fn random_from(source: &mut dyn Entropy) -> String {
    let mut bytes = [0u8; RANDOM_DIGITS / 2];
    let n = match source.fill(&mut bytes) {
        Ok(()) => u64::from_le_bytes(bytes),
        Err(_) => {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u32(std::process::id());
            let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
            h.write_u128(since.map_or(0, |d| d.as_nanos()));
            h.write_u64(NEXT.fetch_add(1, Ordering::Relaxed));
            h.finish()
        }
    };
    format!("{n:0width$x}", width = RANDOM_DIGITS)
}

/// A new directory with mode 0700. `mkdir(2)` refuses a name that is
/// there, also a dangling link.
#[cfg(unix)]
fn create_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    std::fs::DirBuilder::new().mode(0o700).create(path)?;
    // The umask can take bits away from 0700: set it whole, through a
    // descriptor, so that a link put at the name is not followed.
    let set = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .and_then(|dir| dir.set_permissions(std::fs::Permissions::from_mode(0o700)));
    if let Err(e) = set {
        let _ = std::fs::remove_dir(path);
        return Err(e);
    }
    Ok(())
}

#[cfg(windows)]
fn create_dir(path: &Path) -> io::Result<()> {
    super::closed_win::create_new_private_dir(path)
}

#[cfg(not(any(unix, windows)))]
fn create_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// A new file with mode 0600, never through a link.
#[cfg(unix)]
fn create_file(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::PermissionsExt;
    let file = super::create_new_private(path)?;
    // The umask can take bits away from 0600: set it whole.
    if let Err(e) = file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(file)
}

#[cfg(not(unix))]
fn create_file(path: &Path) -> io::Result<std::fs::File> {
    super::create_new_private(path)
}

#[cfg(test)]
#[path = "temp/tests.rs"]
mod tests;
