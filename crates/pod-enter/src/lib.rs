//! The entry of a payload into an extracted root file system, with no
//! daemon and no root.
//!
//! The chroot sequence has a fixed order. Each step prevents a known
//! defect, so do not change the order:
//!
//! 1. The caller opens the root file system with [`RootDir::open`]. A
//!    symbolic link is refused. The entry then holds the directory as a
//!    descriptor, so a change of the path cannot move the payload.
//! 2. The caller takes its image lock before the entry and gives it to
//!    the payload by inheritance, so a concurrent removal of the image
//!    cannot delete a root file system that a payload runs from.
//! 3. Each descriptor of the payload opens before the root changes
//!    ([`Fds`]): after a chroot no path outside the new root resolves,
//!    and an extracted root file system has no `/dev/null`.
//! 4. The caller checks the platform. A foreign architecture runs through
//!    a `binfmt_misc` interpreter, or the caller refuses it by name
//!    ([`binfmt`]). It never becomes a bare `Exec format error`.
//! 5. `fchdir`, `chroot(".")`, `chdir("/")`. Only then does the child
//!    resolve the program, in the process that changed the root.
//!
//! After the root change come the limits, `no_new_privs` and, where the
//! plan has one, the confinement ([`Plan::confine`], `pod-confine`): its
//! paths open before the fork (guest paths on each rung with a chroot),
//! and the child applies it as the last step before the exec.
//!
//! The banner goes to the error writer. The payload owns the standard
//! output, so a banner cannot corrupt a pipeline of the payload.
//!
//! The other entries use the same fork: the rungs of the launch ladder
//! ([`ladder`], [`spawn_ladder`]), the mount namespace rung
//! ([`spawn_namespace`]) and the entry with no chroot ([`spawn_userland`],
//! [`userland`]).
//!
//! The crate reads no variable of the environment. The caller gives the
//! names of the variables and of the files in [`Names`]. A failure of the
//! entry is [`Error::Runtime`] with a `pod_core::Failure`; a payload that
//! did not start is [`Error::CannotInvoke`] or [`Error::NotFound`], with
//! the codes of the shell in [`status`]. The code of a failure of the
//! entry belongs to the caller (docker uses 125).
//!
//! The crate is Linux only. On another target it is empty.
#![cfg(target_os = "linux")]
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod abi;
pub mod binfmt;
pub mod device;
pub mod fuse;
pub mod ladder;
pub mod limits;
pub mod memfd;
pub mod plan;
pub mod stage;
pub mod status;
pub mod userland;

mod child;
mod confine;
mod entry;
mod launch;
mod names;
mod ns_optional;

use std::io::Write;

use pod_core::{Class, Failure, Stage};
use pod_hostprobe::select::Rung;
use pod_hostprobe::sys::{self, CBuf};

pub use child::Bounded;
pub use entry::*;
pub use limits::{Limit, Resource};
pub use names::Names;
pub use ns_optional::{NS_PROC_STEP, NS_PTS_STEP, ns_proc_refused, ns_proc_target, ns_pts_target, ns_refused_line};
pub use plan::{Plan, Program};
pub use status::exit_status;

/// The rung that the chroot sequence enters. It is not always the rung
/// that the probe selects.
///
/// [`run`] does the chroot sequence and nothing more: no `unshare`, no
/// mount, no network or PID namespace. A banner must name this rung, not
/// the rung that the machine permits: no output may imply a namespace,
/// a cgroup or a device that does not exist. The selection of the machine
/// is a second fact, and a caller can report both.
///
/// The namespace rung does not change this constant: [`entered_rung`]
/// maps a namespace selection to `Rung::Namespace`, in one place, so the
/// banner, the record and a strict check cannot disagree.
pub const ENTERED_RUNG: Rung = Rung::Chroot;

/// The rung that a run enters for a selection of the probe.
///
/// `userland` where the probe selects `interpose`: chroot is denied
/// there, so a run enters with no chroot where a family holds, and
/// refuses where none holds. `namespace` where the probe selects it: the
/// entry tries the mount namespace, and chroot is the fallback that the
/// banner names. Each other selection enters the chroot sequence.
pub fn entered_rung(selection: Rung) -> Rung {
    match selection {
        Rung::Interpose => Rung::Userland,
        Rung::Namespace => Rung::Namespace,
        _ => ENTERED_RUNG,
    }
}

/// The word of the rung that a run enters for a selection of the probe.
pub fn entered_word(selection: Rung) -> &'static str {
    entered_rung(selection).word()
}

/// Why an entry did not start the payload.
#[derive(Debug)]
pub enum Error {
    /// The entry could not do its work. The caller gives the exit code
    /// (docker uses 125).
    Runtime(Failure),
    /// The program of the payload exists and the kernel refused to run
    /// it. The shell and docker exit 126 ([`status::CANNOT_INVOKE`]).
    CannotInvoke(String),
    /// The program of the payload was not found. The shell and docker
    /// exit 127 ([`status::NOT_FOUND`]).
    NotFound(String),
    /// The setup of the namespace rung failed before the chroot sequence.
    /// [`spawn_namespace`] catches it, enters chroot, and names the
    /// fallback on the banner, so a caller does not see it there.
    NsSetup {
        /// The step that failed: unshare, the private remount, the mount.
        step: &'static str,
        errno: sys::Errno,
    },
}

impl Error {
    /// A failure of the entry at `Stage::Host`.
    pub fn runtime(class: Class, text: impl Into<String>) -> Error {
        Error::Runtime(Failure::new(Stage::Host, class, text))
    }

    /// The exit code of a payload that did not start: 126 or 127.
    /// `None` for a failure of the entry, whose code the caller gives.
    pub fn payload_code(&self) -> Option<i32> {
        match self {
            Error::CannotInvoke(_) => Some(status::CANNOT_INVOKE),
            Error::NotFound(_) => Some(status::NOT_FOUND),
            Error::Runtime(_) | Error::NsSetup { .. } => None,
        }
    }

    /// The exit code: [`Error::payload_code`], or `runtime` for a failure
    /// of the entry.
    pub fn code_or(&self, runtime: i32) -> i32 {
        self.payload_code().unwrap_or(runtime)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Runtime(failure) => write!(f, "{failure}"),
            Error::CannotInvoke(s) | Error::NotFound(s) => write!(f, "{s}"),
            Error::NsSetup { step, errno } => {
                write!(f, "namespace setup failed at {step}: {} ({})", errno.name(), errno.0)
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Runtime(failure) => std::error::Error::source(failure),
            _ => None,
        }
    }
}

impl From<Failure> for Error {
    fn from(f: Failure) -> Error {
        Error::Runtime(f)
    }
}

impl From<Error> for Failure {
    /// One failure for a caller that does not need the code of the shell:
    /// a refused program is `Class::Refused`, a missing one
    /// `Class::NotFound`, and a refused namespace `Class::Unsupported`.
    fn from(e: Error) -> Failure {
        match e {
            Error::Runtime(f) => f,
            Error::CannotInvoke(s) => Failure::new(Stage::Host, Class::Refused, s),
            Error::NotFound(s) => Failure::new(Stage::Host, Class::NotFound, s),
            e @ Error::NsSetup { .. } => Failure::new(Stage::Host, Class::Unsupported, e.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A failure of the entry for a refused system call: the text, then the
/// name and the number of the errno. The class follows the errno.
pub(crate) fn errno_failure(text: &str, e: sys::Errno) -> Error {
    Error::runtime(errno_class(e), format!("{text}: {} ({})", e.name(), e.0))
}

/// The class of a failure for an errno of the kernel.
pub(crate) fn errno_class(e: sys::Errno) -> Class {
    match e.0 {
        2 | 20 => Class::NotFound,
        1 | 13 | 30 => Class::Refused,
        38 | 95 => Class::Unsupported,
        11 | 4 => Class::TempFail,
        _ => Class::Unavailable,
    }
}

/// A directory descriptor on the root file system, opened before anything
/// changes.
///
/// `chroot` follows a symbolic link, so a root path that is a link puts
/// the payload in a directory that the caller did not choose. The path is
/// read with `AT_SYMLINK_NOFOLLOW`, refused when it is a link, and then
/// held open: `fchdir` onto a descriptor does not follow a later change
/// of the path.
#[derive(Debug)]
pub struct RootDir {
    fd: i64,
    pub path: String,
}

impl RootDir {
    pub fn open(path: &str) -> Result<RootDir> {
        let c = CBuf::new(path).ok_or_else(|| Error::runtime(Class::Usage, format!("{path:?} contains a NUL byte")))?;
        let st =
            sys::fstatat(sys::AT_FDCWD as i64, &c, sys::AT_SYMLINK_NOFOLLOW).map_err(|e| errno_failure(path, e))?;
        if st.st_mode & sys::S_IFMT == sys::S_IFLNK {
            let target = sys::readlink(&c).unwrap_or_else(|_| "(unreadable)".into());
            return Err(Error::runtime(
                Class::Policy,
                format!(
                    "{path} is a symlink to {target}. The entry refuses it and does not \
                     resolve it: chroot follows a symlink, so the payload would run in a \
                     directory that the caller did not choose"
                ),
            ));
        }
        if st.st_mode & sys::S_IFMT != sys::S_IFDIR {
            return Err(Error::runtime(Class::Usage, format!("{path} is not a directory")));
        }
        let fd = sys::open(&c, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC | sys::O_NOFOLLOW, 0)
            .map_err(|e| errno_failure(&format!("opening {path}"), e))?;
        Ok(RootDir { fd, path: path.to_string() })
    }

    pub fn fd(&self) -> i64 {
        self.fd
    }
}

impl Drop for RootDir {
    fn drop(&mut self) {
        let _ = sys::close(self.fd);
    }
}

/// The descriptors of the payload, opened before the fork.
///
/// A descriptor opened before the root changes works after the change,
/// and it is the only thing that crosses into the new root: there is no
/// attach path and no bind mount. The default is empty on purpose: the
/// payload inherits the standard streams, so the standard output of the
/// caller is the standard output of the payload. A `--device` or a
/// terminal pair adds its descriptors here.
#[derive(Default)]
pub struct Fds {
    /// `(child_fd, host_fd)` pairs, each duplicated into place in the
    /// child.
    pub pass: Vec<(i64, i64)>,
}

/// Run a payload inside `root`, and return its exit status.
///
/// The exit status is the status of the payload. A caller reads the exit
/// code first, so the entry never changes it.
pub fn run(root: &RootDir, plan: &Plan, err: &mut dyn Write) -> Result<i32> {
    let child = spawn(root, plan, err)?;
    child.wait_forwarding(err)
}

/// A payload that reached its `execve`, and its PID.
///
/// This value exists only after the exec succeeded: the parent reads the
/// error pipe to its end, and a sleep does not decide it. A failure before
/// the exec arrives on the pipe as an errno and becomes an [`Error`], so a
/// caller never has to decide whether a process that it cannot see yet
/// starts or is already dead.
#[derive(Debug)]
pub struct Child {
    pub pid: i64,
}

#[cfg(test)]
mod tests;
