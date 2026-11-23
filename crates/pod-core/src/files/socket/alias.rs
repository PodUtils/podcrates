//! A short path for the stage of a bind whose path does not fit in
//! `sun_path`. The bind goes through the short path, and the kernel puts
//! the socket at the long one.
//!
//! On Linux the short path is `/proc/self/fd/N/s`, through a descriptor of
//! the stage: nothing is made in a shared directory, and no other process
//! can move the descriptor. When `/proc` is not there, or its `self` is
//! not this process, the short path is a symbolic link to the stage, made
//! under a random name in `/tmp` (else `/var/tmp`). The link goes only into
//! a directory where no other user can remove or rename it: one of this
//! user or of root, that other users cannot write or that is sticky. A
//! [`Short`] removes its link when it drops, so the link goes on each path
//! out.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The directories that can take the link, in this order.
pub(super) const LINK_DIRS: [&str; 2] = ["/tmp", "/var/tmp"];

/// The ways to a short path that a bind may take.
pub(super) struct Route<'a> {
    /// Take `/proc/self/fd/N` first (Linux and Android).
    pub proc_fd: bool,
    /// The directories that can take a link, in order.
    pub link_dirs: &'a [&'a Path],
}

/// The route of [`super::bind_staged`].
pub(super) fn auto() -> Route<'static> {
    static DIRS: std::sync::LazyLock<[&'static Path; 2]> =
        std::sync::LazyLock::new(|| [Path::new(LINK_DIRS[0]), Path::new(LINK_DIRS[1])]);
    Route { proc_fd: cfg!(any(target_os = "linux", target_os = "android")), link_dirs: &*DIRS }
}

/// A short path to the socket `s` in a stage. It keeps the descriptor or
/// the link alive until it drops.
pub(super) struct Short {
    /// The path to bind.
    pub bind: PathBuf,
    /// The link to remove at the drop.
    link: Option<PathBuf>,
    /// The descriptor that `/proc/self/fd/N` names.
    _fd: Option<std::fs::File>,
}

impl Drop for Short {
    fn drop(&mut self) {
        if let Some(link) = &self.link {
            let _ = std::fs::remove_file(link);
        }
    }
}

/// A short path to `stage/s`, by the first way of `route` that works.
pub(super) fn short(stage: &Path, route: &Route<'_>, most: usize) -> std::io::Result<Short> {
    if route.proc_fd
        && let Some(s) = through_proc(stage).filter(|s| s.bind.as_os_str().len() <= most)
    {
        return Ok(s);
    }
    let target = std::path::absolute(stage)?;
    let mut last = None;
    for dir in route.link_dirs.iter().filter(|d| safe_dir(d)) {
        for _ in 0..super::NAMES {
            let link = dir.join(super::random_name());
            let bind = link.join("s");
            if bind.as_os_str().len() > most {
                break;
            }
            // symlink(2) refuses a name that is there, and follows nothing.
            match std::os::unix::fs::symlink(&target, &link) {
                Ok(()) => return Ok(Short { bind, link: Some(link), _fd: None }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => {
                    last = Some(e);
                    break;
                }
            }
        }
    }
    let why = last.map(|e| format!(": {e}")).unwrap_or_default();
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "{} is longer than the {most} bytes of a socket address, and no short path to it was made \
             (/proc/self/fd, a link in {}){why}",
            stage.join("s").display(),
            route.link_dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(" or "),
        ),
    ))
}

/// `/proc/self/fd/N/s` for a descriptor of `stage`, when `/proc/self/fd/N`
/// is that directory.
fn through_proc(stage: &Path) -> Option<Short> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let fd = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(stage)
        .ok()?;
    let held = fd.metadata().ok()?;
    let via = PathBuf::from(format!("/proc/self/fd/{}", fd.as_raw_fd()));
    // A `/proc` of another process (a mount of another pid namespace)
    // names another directory: then this way is not taken.
    let seen = std::fs::metadata(&via).ok()?;
    (seen.dev() == held.dev() && seen.ino() == held.ino()).then(|| Short {
        bind: via.join("s"),
        link: None,
        _fd: Some(fd),
    })
}

/// True when no other user can remove or rename a link in `dir`: it is a
/// directory of this user or of root, and only its owner can write it or
/// it is sticky.
fn safe_dir(dir: &Path) -> bool {
    let Ok(m) = std::fs::metadata(dir) else { return false };
    let owner = m.uid() == super::super::euid() || m.uid() == 0;
    m.is_dir() && owner && (m.mode() & 0o022 == 0 || m.mode() & 0o1000 != 0)
}
