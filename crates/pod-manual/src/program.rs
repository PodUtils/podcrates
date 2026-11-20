//! The lookup of a program in a search path, as a shell finds it: the
//! first directory of `PATH` that holds the name as a file that can run.
//!
//! On Windows a program file has an extension (`git.exe`), and a command
//! names it without one (`git`): each name with no extension also matches
//! `NAME.EXT` for each extension of `PATHEXT`. A caller gives the
//! variables; this module reads no environment.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The extensions of `cmd.exe` when `PATHEXT` is not set.
pub const PATHEXT_DEFAULT: &str = ".COM;.EXE;.BAT;.CMD";

/// The extensions that a lookup tries for a name with none: `None` when
/// `windows` (`cfg!(windows)`) is false; else `pathext` (the value of
/// `PATHEXT`), or [`PATHEXT_DEFAULT`] when it is not set or empty.
pub fn extensions(windows: bool, pathext: Option<OsString>) -> Option<OsString> {
    windows.then(|| pathext.filter(|v| !v.is_empty()).unwrap_or_else(|| PATHEXT_DEFAULT.into()))
}

/// The file names that a lookup tries in each directory: `name`, and, for
/// a name with no extension, `name` with each extension of `exts` (a list
/// with `;`, as `PATHEXT`).
pub fn names(name: &str, exts: Option<&OsStr>) -> Vec<OsString> {
    let mut out = vec![OsString::from(name)];
    if let Some(exts) = exts
        && Path::new(name).extension().is_none()
    {
        for ext in exts.to_string_lossy().split(';').filter(|e| !e.is_empty()) {
            out.push(OsString::from(format!("{name}{ext}")));
        }
    }
    out
}

/// A name that is a path, not a name to look up: it has a `/`, or a `\`
/// where `exts` is set (Windows).
pub fn is_path(name: &str, exts: Option<&OsStr>) -> bool {
    name.contains('/') || (exts.is_some() && name.contains('\\'))
}

/// The first file of [`names`] in a directory of `path` (a search path
/// such as `PATH`) that this process may run.
pub fn find(name: &str, path: Option<OsString>, exts: Option<&OsStr>) -> Option<PathBuf> {
    let names = names(name, exts);
    std::env::split_paths(&path?).find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| runnable(p)))
}

/// A file that this process may run: a file with an execute bit on Unix,
/// any file elsewhere.
pub fn runnable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

#[cfg(test)]
mod tests;
