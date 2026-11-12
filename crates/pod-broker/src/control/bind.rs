//! The bind of a Unix socket with its mode from the first instant (T-196,
//! PodUtils/podnet#444). A socket that `bind(2)` makes at its path has the
//! mode of the umask until a `chmod`; in a shared directory another user
//! can connect in that window, and a listener of one job takes no token.
//! So the socket is bound in a new directory of mode 0700 beside the path,
//! gets its mode there, and moves to the path by `link(2)`, which refuses a
//! path that is there. The socket keeps the name of its bind
//! (`getsockname`): a caller that needs that name binds in a directory of
//! mode 0700 of its own.

use std::hash::{BuildHasher, Hasher};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// `sun_path` holds 108 bytes on Linux and 104 on the BSDs: the longest
/// staged path that each takes, with its NUL.
const SUN_PATH_MOST: usize = 103;

/// At most this many names are tried for the directory of the stage.
const NAMES: usize = 8;

/// A name that another user cannot guess: the random keys of the standard
/// hasher, which the operating system seeds. The name is short: each byte
/// of it is a byte less for the parent in `sun_path`, and a parent that
/// does not fit loses the stage (macOS gives 104 bytes, and its temporary
/// directory takes about 50).
fn random_name() -> String {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    format!(".pb-{:016x}", h.finish())
}

/// True when the stage of a socket in `parent` fits in `sun_path`: the
/// parent, the name of the stage, and the leaf `s`.
fn stage_fits(parent: &Path) -> bool {
    parent.as_os_str().len() + 1 + random_name().len() + 2 <= SUN_PATH_MOST
}

/// The listener at `path` with `mode`, bound through a stage of mode 0700.
/// A path whose stage does not fit in `sun_path` is bound at the path, and
/// then only a directory of mode 0700 keeps the window shut.
pub(super) fn bind_staged(path: &Path, mode: u32) -> std::io::Result<std::os::unix::net::UnixListener> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    if !stage_fits(&parent) {
        let listener = std::os::unix::net::UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        return Ok(listener);
    }
    let stage = make_stage(&parent)?;
    let staged = stage.join("s");
    let bound = (|| {
        let listener = std::os::unix::net::UnixListener::bind(&staged)?;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(mode))?;
        std::fs::hard_link(&staged, path).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("{} is there: another bind took the path", path.display()),
            ),
            _ => e,
        })?;
        Ok(listener)
    })();
    let _ = std::fs::remove_file(&staged);
    let _ = std::fs::remove_dir(&stage);
    bound
}

/// A new directory of mode 0700 beside the path. `mkdir(2)` refuses a name
/// that is there, so a directory of another user is never taken.
fn make_stage(parent: &Path) -> std::io::Result<PathBuf> {
    let mut last = None;
    for _ in 0..NAMES {
        let stage = parent.join(random_name());
        match std::fs::DirBuilder::new().mode(0o700).create(&stage) {
            Ok(()) => {
                // The umask can take bits away from 0700; set it whole.
                std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700))?;
                return Ok(stage);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no name for the stage of the bind")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each socket directory of a test (at most 80 bytes, `pod_testkit`)
    /// takes the stage under the 104 bytes of macOS.
    #[test]
    fn a_parent_of_80_bytes_takes_the_stage() {
        let parent = PathBuf::from(format!("/{}", "d".repeat(79)));
        assert!(stage_fits(&parent));
        let longer = PathBuf::from(format!("/{}", "d".repeat(80)));
        assert!(!stage_fits(&longer));
    }
}
