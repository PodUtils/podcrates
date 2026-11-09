//! `TOOL.md` section 6.5: the chroot entry sequence.
//!
//! [`TODO/milestones.md`](../../../TODO/milestones.md) T-1104 and
//! [`TODO/enter.md`](../../../TODO/enter.md) T-0501 to T-0506.
//!
//! ⭐ **The order of this sequence is the design, and every step of it is an
//! entry.** Doing them in a different order is not a style choice; each ordering
//! constraint below was paid for by a defect somebody shipped.
//!
//! 1. the rootfs is resolved from the store and **refused if it is a symlink**
//!    ([`TODO/enter.md`](../../../TODO/enter.md) T-0504), then held as a
//!    directory descriptor so nothing can swap the path afterwards;
//! 2. the image lock is taken and handed to the payload
//!    ([`TODO/image.md`](../../../TODO/image.md) T-0204, T-0211), so a
//!    concurrent `rmi` cannot delete a rootfs a running container is executing
//!    out of;
//! 3. **every descriptor is opened before the root changes** (T-0501): a chroot
//!    cuts off every path outside the new root, and a child with no explicit
//!    stdio opens `/dev/null`, which an extracted rootfs does not have;
//! 4. the platform is checked against the host's, and a foreign one is either
//!    run through a registered `binfmt_misc` interpreter or **refused by name**
//!    (T-0506), never exec'd into a bare `Exec format error`;
//! 5. `fchdir`, `chroot(".")`, `chdir("/")`, and only **then** is the program
//!    resolved, in the process that changed the root (T-0502).
//!
//! ⛔ **The banner goes to stderr and the payload owns stdout.** T-1104's own
//! decision: a banner on stdout corrupts every pipeline the payload is in.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod abi;
pub mod binfmt;
pub mod device;
pub mod fuse;
pub mod ladder;
pub mod memfd;
pub mod plan;
pub mod stage;
pub mod userland;

use std::io::Write;

use podvm_probe::sys::{self, CBuf};

pub use plan::{Plan, Program};

/// ⭐ **THE RUNG THIS CRATE ENTERS WHERE NOTHING ELSE APPLIES**, which is
/// not always the rung the probe selects.
///
/// [`crate::run`] performs `TOOL.md` section 6.5's sequence and nothing else: it
/// `chroot`s. It does not `unshare`, it mounts nothing, and it creates no
/// network or pid namespace, on any machine.
///
/// ⛔ **The banner must say THIS and not what the machine would permit**, or it
/// breaks [`TODO/cli.md`](../../../TODO/cli.md) T-0804 rule 4: no output may
/// imply namespaces, cgroups or devices exist when they do not. Measured on
/// 2026-09-09: this host's probe selects `namespace`, and `podvm run` printed
/// `mode=namespace (namespaces: as configured; mounts: full)` while entering a
/// plain chroot. The machine's own selection is still reported, beside this,
/// because "what podvm did" and "what this machine would permit" are two facts
/// and a reader needs both.
///
/// ⚠ The namespace rung (T-1339) does not move this constant: it stays the
/// rung entered where no other rung applies, and [`entered_rung`] maps a
/// namespace selection onto `Rung::Namespace`. One mapping, so the banner,
/// the record and `--strict` cannot disagree.
pub const ENTERED_RUNG: podvm_probe::select::Rung = podvm_probe::select::Rung::Chroot;

/// The rung `podvm run` enters with for a probe selection.
///
/// `userland` where the probe selects `interpose`: chroot is denied
/// there, so a run enters without it where a family holds, and refuses
/// where none does. `namespace` where the probe selects it (T-1339):
/// the mount namespace is attempted and chroot is the fallback, named
/// on the banner. Every other selection enters the chroot sequence.
pub fn entered_rung(selection: podvm_probe::select::Rung) -> podvm_probe::select::Rung {
    use podvm_probe::select::Rung as R;
    match selection {
        R::Interpose => R::Userland,
        R::Namespace => R::Namespace,
        _ => ENTERED_RUNG,
    }
}

/// The entered word for a probe selection.
///
/// `userland` where the probe selects `interpose`: chroot is denied
/// there, so a run enters without it where a family holds, and refuses
/// where none does. Every other selection enters its own rung's word.
/// TODO/enter.md T-1317.
pub fn entered_word(selection: podvm_probe::select::Rung) -> &'static str {
    entered_rung(selection).word()
}

// ⛔ **docker's codes, from the one file that holds them.**
// [`TODO/cli.md`](../../../TODO/cli.md) T-0802. This crate declared its own 125,
// 126 and 127 and `podvm-image` declared 125 again; a correction to one of them
// did not reach the others.
pub use podvm_probe::exit::{EXIT_CANNOT_INVOKE, EXIT_NOT_FOUND, EXIT_RUNTIME_ERROR};

#[derive(Debug)]
pub enum Error {
    /// Something podvm could not do. Exits 125.
    Runtime(String),
    /// The payload's command could not be invoked. Exits 126.
    CannotInvoke(String),
    /// The payload's command was not found. Exits 127.
    NotFound(String),
    /// The namespace setup failed before the chroot sequence. Never
    /// escapes: [`spawn_namespace`] catches it, enters chroot instead,
    /// and names the fallback on the banner. Exits 125 where it does.
    NsSetup {
        /// The readiness-pipe step that failed: unshare, privatise, mount.
        step: &'static str,
        errno: sys::Errno,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Runtime(s) | Error::CannotInvoke(s) | Error::NotFound(s) => write!(f, "{s}"),
            Error::NsSetup { step, errno } => write!(
                f,
                "namespace setup failed at {step}: {} ({})",
                errno.name(),
                errno.0
            ),
        }
    }
}

impl Error {
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Runtime(_) | Error::NsSetup { .. } => EXIT_RUNTIME_ERROR,
            Error::CannotInvoke(_) => EXIT_CANNOT_INVOKE,
            Error::NotFound(_) => EXIT_NOT_FOUND,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A directory descriptor on the rootfs, opened before anything changes.
///
/// ⛔ T-0504. `chroot` **follows a symlink**, so a rootfs path that is one puts
/// the payload somewhere podvm did not choose and every containment claim
/// afterwards is about a different directory. The path is `lstat`ed, refused if
/// it is a symlink, and then held open: `fchdir` onto a descriptor cannot be
/// redirected by anything that happens to the path in between.
#[derive(Debug)]
pub struct RootDir {
    fd: i64,
    pub path: String,
}

impl RootDir {
    pub fn open(path: &str) -> Result<RootDir> {
        let c = CBuf::new(path)
            .ok_or_else(|| Error::Runtime(format!("{path:?} contains a NUL byte")))?;

        // ⛔ `AT_SYMLINK_NOFOLLOW`, so this asks about the path itself and not
        // about whatever it points at.
        let st = sys::fstatat(sys::AT_FDCWD as i64, &c, sys::AT_SYMLINK_NOFOLLOW)
            .map_err(|e| Error::Runtime(format!("{path}: {} ({})", e.name(), e.0)))?;
        if st.st_mode & sys::S_IFMT == sys::S_IFLNK {
            let target = sys::readlink(&c).unwrap_or_else(|_| "(unreadable)".into());
            return Err(Error::Runtime(format!(
                "{path} is a symlink to {target}. podvm refuses it rather than \
                 resolving it: chroot follows a symlink, so entering it would \
                 put the payload somewhere podvm did not choose, and the \
                 ownership sidecar and the image lock belong to the store's own \
                 directory (TODO/enter.md T-0504)"
            )));
        }
        if st.st_mode & sys::S_IFMT != sys::S_IFDIR {
            return Err(Error::Runtime(format!("{path} is not a directory")));
        }

        let fd = sys::open(
            &c,
            sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC | sys::O_NOFOLLOW,
            0,
        )
        .map_err(|e| Error::Runtime(format!("opening {path}: {} ({})", e.name(), e.0)))?;
        Ok(RootDir {
            fd,
            path: path.to_string(),
        })
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

/// What the payload will be given, resolved before the fork.
///
/// ⛔ T-0501. Everything the child touches after the `chroot` exists before it.
/// A descriptor opened before the root changes keeps working after it, and it is
/// the only thing that crosses the boundary on this runtime: there is no attach
/// path and nothing can be bind-mounted.
/// ⚠ The default is EMPTY, and that is the design rather than a stub: stdio is
/// inherited rather than re-opened, so the caller's stdout IS the payload's,
/// which is what makes `podvm run ... | consumer` work at all. What goes in
/// here is what a `--device` or a `-t` pty pair will add, opened before the
/// root changes.
#[derive(Default)]
pub struct Fds {
    /// `(child_fd, host_fd)` pairs, dup'd into place in the child.
    pub pass: Vec<(i64, i64)>,
}

/// Run a payload inside `root`, and return its exit status.
///
/// ⛔ **The exit status is the payload's**, not podvm's. T-0802: an automated
/// caller reads the exit code first, and a runtime that improves a payload's
/// code lies in the field read first.
pub fn run(root: &RootDir, plan: &Plan, err: &mut dyn Write) -> Result<i32> {
    let child = spawn(root, plan, err)?;
    child.wait_forwarding(err)
}

/// A payload that has reached its `execve`, and its pid.
///
/// ⛔ [`TODO/supervise.md`](../../../TODO/supervise.md) T-0602: this value only
/// exists once the exec has SUCCEEDED, established by reading the error pipe to
/// EOF rather than by sleeping and looking. A failure before the exec arrives on
/// that pipe as an errno and becomes an [`Error`] here, so a caller never has to
/// decide whether a process it cannot see yet is starting or already dead.
#[derive(Debug)]
pub struct Child {
    pub pid: i64,
}
mod child;
pub use child::*;

mod entry;
pub use entry::*;

mod launch;
use launch::*;

/// Was the file there and refused, rather than absent?
///
/// ⛔ **The errno decides, not a guess.** `execve` answers `ENOENT` for a path
/// that is not there -- and also for a dynamic loader the binary names and that
/// is not there, which is the trap -- while `EACCES`, `ENOEXEC`, `EISDIR`,
/// `EPERM` and `ETXTBSY` all mean the file was found and could not be run.
/// docker's codes split exactly there: 127 for the first, 126 for the rest.
///
/// ⚠ `ELOOP` and `ENAMETOOLONG` are `NotFound`'s side deliberately: both are
/// answers about resolving the path rather than about the file at the end of it.
fn invocable_but_refused(e: sys::Errno) -> bool {
    matches!(e.0, 1 | 8 | 13 | 21 | 26)
}

/// docker's translation of a wait status into an exit code.
///
/// ⛔ A signalled payload exits `128 + signal`, which is the shell's convention
/// and docker's. Reporting 0 or 1 for a killed process is the lie T-0802 is
/// about: a caller that branches on the code cannot tell a clean exit from a
/// SIGKILL.
pub fn exit_status(status: i32) -> i32 {
    if status & 0x7f == 0 {
        (status >> 8) & 0xff
    } else {
        128 + (status & 0x7f)
    }
}

/// The word the forwarding wait prints for a signal: the name for the two
/// it forwards, `signal <n>` for anything the reap reports that it did not
/// send (a terminal's SIGINT arrives both ways, so the reap can name a
/// signal the forward never saw).
fn signal_name(signo: i64) -> String {
    match signo {
        n if n == sys::SIGINT as i64 => "SIGINT".to_string(),
        n if n == sys::SIGTERM as i64 => "SIGTERM".to_string(),
        n => format!("signal {n}"),
    }
}

#[cfg(test)]
mod tests;
