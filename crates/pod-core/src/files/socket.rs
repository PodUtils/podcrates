//! A Unix socket of one user: the bind with its mode from the first instant
//! (T-196, PodUtils/podnet#444), at a path of any length (T-1114,
//! PodUtils/podssh#343), and the ids of the peer of a stream.
//!
//! A socket that `bind(2)` makes at its path has the mode of the umask
//! until a `chmod`; in a shared directory another user
//! can connect in that window, and a listener of one job takes no token.
//! So the socket is bound in a new directory of mode 0700 beside the path,
//! gets its mode there, and moves to the path by `link(2)`, which refuses a
//! path that is there. The socket keeps the name of its bind
//! (`getsockname`): a caller that needs that name binds in a directory of
//! mode 0700 of its own.
//!
//! `sun_path` holds 104 bytes on macOS and 108 on Linux, with the NUL, and
//! a temporary directory of macOS takes about 50 of them. When the stage
//! does not fit, the bind goes through a short path to the stage
//! (`alias`), and the bind is checked to have made the socket in the
//! stage. `link(2)` takes a path of any length, so only a file name past
//! the 255 bytes of a name fails.

use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};

mod alias;
mod answers;

pub use answers::socket_answers;

/// `sun_path` holds 108 bytes on Linux and 104 on the BSDs: the longest
/// staged path that each takes, with its NUL.
const SUN_PATH_MOST: usize = 103;

/// The longest file name (`NAME_MAX`).
const NAME_MOST: usize = 255;

/// The prefix of the name of a stage, and of a link to it: short, as each
/// byte of the name is a byte less for the parent in `sun_path`.
const STAGE: &str = ".pb";

/// The length of the name of a stage: `.pb-RANDOM`
/// ([`super::new_temp_dir`]).
const STAGE_NAME: usize = STAGE.len() + 1 + super::temp::RANDOM_DIGITS;

/// True when the stage of a socket in `parent` fits in `sun_path`: the
/// parent, the name of the stage, and the leaf `s`.
fn stage_fits(parent: &Path) -> bool {
    parent.as_os_str().len() + 1 + STAGE_NAME + 2 <= SUN_PATH_MOST
}

/// The listener at `path` with `mode`, bound through a stage of mode 0700.
/// A stage that does not fit in `sun_path` is bound through a short path
/// to it. A file name past 255 bytes is `InvalidInput`.
pub fn bind_staged(path: &Path, mode: u32) -> std::io::Result<std::os::unix::net::UnixListener> {
    bind_by(path, mode, &alias::auto())
}

/// [`bind_staged`] with the ways to a short path of `route`.
fn bind_by(path: &Path, mode: u32, route: &alias::Route<'_>) -> std::io::Result<std::os::unix::net::UnixListener> {
    let name = path.file_name().map_or(0, |n| n.len());
    if name == 0 || name > NAME_MOST {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{}: give a file name of 1 to {NAME_MOST} bytes, not {name}", path.display()),
        ));
    }
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    // A new directory of mode 0700 beside the path: `mkdir(2)` refuses a
    // name that is there, so a directory of another user is never taken.
    let stage = super::new_temp_dir(&parent, STAGE)?;
    let staged = stage.join("s");
    let bound = (|| {
        let listener = if stage_fits(&parent) {
            std::os::unix::net::UnixListener::bind(&staged)?
        } else {
            let short = alias::short(&stage, route, SUN_PATH_MOST)?;
            let listener = std::os::unix::net::UnixListener::bind(&short.bind)?;
            let landed = std::fs::symlink_metadata(&staged).is_ok_and(|m| m.file_type().is_socket());
            if !landed {
                let _ = std::fs::remove_file(&short.bind);
                return Err(std::io::Error::other(format!(
                    "the bind through {} made no socket at {}",
                    short.bind.display(),
                    staged.display()
                )));
            }
            listener
        };
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

/// The ids of the peer of a Unix stream, as the kernel gave them at the
/// connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerIds {
    /// The effective uid.
    pub uid: u32,
    /// The effective gid.
    pub gid: u32,
    /// The process id, where the system gives it (Linux).
    pub pid: Option<u32>,
}

/// The ids of the peer of `stream`: `SO_PEERCRED` on Linux and Android,
/// `getpeereid(3)` on the other systems.
pub fn peer_ids(stream: &std::os::unix::net::UnixStream) -> std::io::Result<PeerIds> {
    use std::os::fd::AsRawFd;
    let fd = stream.as_raw_fd();
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: getsockopt writes at most `len` bytes into `cred`, a ucred.
        let rc = unsafe { libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_PEERCRED, (&raw mut cred).cast(), &mut len) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(PeerIds { uid: cred.uid, gid: cred.gid, pid: u32::try_from(cred.pid).ok().filter(|p| *p != 0) })
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: getpeereid writes the two ids of a connected socket.
        if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(PeerIds { uid, gid, pid: None })
    }
}

#[cfg(test)]
mod tests;
