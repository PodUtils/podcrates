//! The unix socket carrier: a stream over `AF_UNIX`, for a host that
//! refuses `bind(AF_INET)`. The address is a path of 1 to 107 bytes.
//!
//! * A listener's socket has the mode 0600: it is bound in a new directory
//!   of mode 0700 beside the path, and moved to the path with no replace,
//!   so it never exists at the path with the mode of the umask.
//! * A regular file at the path is never removed (a usage error); a socket
//!   that answers belongs to another listener (in use); a stale socket is
//!   removed.
//! * Each end checks the uid of its peer (`SO_PEERCRED`, or `getpeereid`)
//!   against its own, unless [`Unix::allow_other_uid`] is set.

use std::io;
use std::os::unix::fs::{DirBuilderExt as _, FileTypeExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Listener, Pending};

/// The name of the carrier.
pub const NAME: &str = "unix";

/// The longest path: the 108 bytes of `sun_path` with its NUL.
pub const MAX_PATH: usize = 107;

/// Two connected ends of a unix socket pair, for an embedder in one
/// process.
pub fn pair() -> io::Result<(UnixStream, UnixStream)> {
    UnixStream::pair()
}

/// The unix socket carrier.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unix {
    /// Accept a peer of another uid.
    pub allow_other_uid: bool,
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

fn check_path(address: &str) -> Result<&Path, CarrierError> {
    if address.is_empty() || address.len() > MAX_PATH || address.contains('\0') {
        return Err(err(
            CarrierErrorKind::Usage,
            format!("a path of {} bytes: give 1 to {MAX_PATH} bytes with no NUL", address.len()),
        ));
    }
    Ok(Path::new(address))
}

/// The effective uid of this process.
fn own_uid() -> u32 {
    // SAFETY: geteuid has no argument and cannot fail.
    unsafe { libc::geteuid() }
}

/// Refuse a peer of another uid, unless `allow` is set.
fn check_peer(stream: &UnixStream, allow: bool) -> Result<(), CarrierError> {
    if allow {
        return Ok(());
    }
    let cred = stream.peer_cred().map_err(|e| CarrierError::io(NAME, "the credentials of the peer", e))?;
    let own = own_uid();
    if cred.uid() != own {
        return Err(err(
            CarrierErrorKind::Refused,
            format!("the peer runs as uid {}, not as this user (uid {own})", cred.uid()),
        ));
    }
    Ok(())
}

impl Carrier for Unix {
    fn name(&self) -> &'static str {
        NAME
    }

    fn facts(&self) -> Facts {
        Facts::STREAM
    }

    /// A socket pair of `AF_UNIX` opens: the host allows the family.
    fn available(&self) -> Result<(), CarrierError> {
        std::os::unix::net::UnixStream::pair()
            .map(drop)
            .map_err(|e| CarrierError::new(NAME, CarrierErrorKind::Unavailable, format!("AF_UNIX stream sockets: {e}")))
    }

    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let path = check_path(address)?;
            let stream = UnixStream::connect(path)
                .await
                .map_err(|e| CarrierError::io(NAME, format!("connect to {}", path.display()), e))?;
            check_peer(&stream, self.allow_other_uid)?;
            Ok(Box::new(stream) as BoxIo)
        })
    }

    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            let path = check_path(address)?;
            claim(path)?;
            let listener =
                bind_private(path).map_err(|e| CarrierError::io(NAME, format!("bind {}", path.display()), e))?;
            Ok(Box::new(UnixCarrierListener {
                path: path.to_path_buf(),
                listener,
                allow_other_uid: self.allow_other_uid,
            }) as Box<dyn Listener>)
        })
    }
}

/// A listener of the unix carrier. Its drop removes its socket file.
struct UnixCarrierListener {
    path: PathBuf,
    listener: UnixListener,
    allow_other_uid: bool,
}

impl Listener for UnixCarrierListener {
    fn address(&self) -> String {
        self.path.display().to_string()
    }

    fn accept(&mut self) -> Pending<'_, BoxIo> {
        Box::pin(async move {
            let (stream, _) = self.listener.accept().await.map_err(|e| CarrierError::io(NAME, "accept", e))?;
            check_peer(&stream, self.allow_other_uid)?;
            Ok(Box::new(stream) as BoxIo)
        })
    }
}

impl Drop for UnixCarrierListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Make `path` free for a new listener. A socket that answers belongs to
/// another listener: in use. A file that is not a socket is never removed.
/// A stale socket is removed.
fn claim(path: &Path) -> Result<(), CarrierError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => {
            if std::os::unix::net::UnixStream::connect(path).is_ok() {
                return Err(err(CarrierErrorKind::InUse, format!("{} is served by another listener", path.display())));
            }
            std::fs::remove_file(path)
                .map_err(|e| CarrierError::io(NAME, format!("remove the stale socket {}", path.display()), e))
        }
        Ok(_) => Err(err(CarrierErrorKind::Usage, format!("{} exists and is not a socket", path.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CarrierError::io(NAME, format!("read {}", path.display()), e)),
    }
}

/// Bind a listener at `path` with the mode 0600. The socket is bound in a
/// new directory of mode 0700 beside the path, then moved to the path with
/// no replace. When the staged name does not fit in `sun_path`, the socket
/// is bound at the path and its mode set at once.
fn bind_private(path: &Path) -> io::Result<UnixListener> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let dir = parent.join(format!(".pod-link-{:016x}", nonce()));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    let staged = dir.join("s");
    let bound = if staged.as_os_str().len() > MAX_PATH {
        None
    } else {
        Some(UnixListener::bind(&staged).and_then(|listener| {
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600))?;
            move_no_replace(&staged, path)?;
            Ok(listener)
        }))
    };
    let _ = std::fs::remove_file(&staged);
    let _ = std::fs::remove_dir(&dir);
    match bound {
        Some(bound) => bound,
        None => {
            let listener = UnixListener::bind(path)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            Ok(listener)
        }
    }
}

/// A value for a name that no other bind of this process or another
/// process takes at the same time.
fn nonce() -> u64 {
    use std::hash::{BuildHasher as _, RandomState};
    RandomState::new().hash_one((std::process::id(), std::time::SystemTime::now()))
}

/// Move `from` to `to` when `to` does not exist. An existing `to` is
/// `AddrInUse`.
fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let in_use = |e: io::Error| match e.kind() {
        io::ErrorKind::AlreadyExists => io::Error::new(io::ErrorKind::AddrInUse, format!("{} exists", to.display())),
        _ => e,
    };
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::ffi::OsStrExt as _;
        let a = std::ffi::CString::new(from.as_os_str().as_bytes())?;
        let b = std::ffi::CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: two NUL-terminated paths, relative to the working
        // directory; renameat2 reads them and keeps no pointer.
        let rc = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if rc == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        // A file system or a kernel with no RENAME_NOREPLACE: a link and an
        // unlink do the same.
        if !matches!(e.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL)) {
            return Err(in_use(e));
        }
    }
    std::fs::hard_link(from, to).map_err(in_use)?;
    std::fs::remove_file(from)
}
