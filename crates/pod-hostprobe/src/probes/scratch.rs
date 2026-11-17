//! The helpers of the probe bodies: the scratch files, the payload of the
//! exec probes, the detached tmpfs and the skip of a failed precondition.

use super::*;

/// A path that cannot exist. A syscall that resolves paths and is not filtered
/// must answer `ENOENT` for it.
pub(super) const BOGUS: &str = "/proc/self/nonexistent-probe-path";

pub(super) fn c(s: &str) -> CBuf {
    // Every literal below is NUL-free, so this cannot fail. Constructing it
    // through the checked path anyway keeps one constructor.
    CBuf::new(s).expect("probe path literals carry no interior NUL")
}

/// A scratch path of this probe: `base`, the pid and a random nonce. A
/// fixed path is taken by a file that a killed probe or another tool left,
/// and a pid repeats in each PID namespace (PodUtils/podvm#569): the probe
/// then skips its row.
pub(super) fn unique(base: &str) -> String {
    let mut bytes = [0u8; 8];
    let got = unsafe { sys::sys(sys::SYS_GETRANDOM, [bytes.as_mut_ptr() as u64, 8, 0, 0, 0, 0]) };
    let nonce = match got {
        Ok(8) => u64::from_ne_bytes(bytes),
        // No random bytes: the clock. The exclusive create still holds.
        _ => std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64),
    };
    format!("{base}.{}.{nonce:016x}", sys::getpid())
}

pub(super) fn exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Create a scratch file the probe owns.
///
/// `O_EXCL`. A probe that deletes whatever is in its way has destroyed data
/// to measure a permission, and the errno it then reports is about a file it
/// created rather than the one that was there.
pub(super) fn make_scratch(path: &str) -> Result<(), Outcome> {
    match create_exclusive(path, 0o600) {
        Ok(fd) => {
            let _ = sys::close(fd);
            Ok(())
        }
        Err(sys::EEXIST) => Err(already_there(path)),
        Err(e) => Err(Outcome::skip(Some(e), format!("could not create the scratch file {path}"))),
    }
}

pub(super) fn create_exclusive(path: &str, mode: u64) -> Result<i64, Errno> {
    sys::open(&c(path), sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL, mode)
}

pub(super) fn already_there(path: &str) -> Outcome {
    Outcome::skip(
        Some(sys::EEXIST),
        format!("{path} already exists and this probe does not overwrite; remove it and re-run"),
    )
}

pub(super) fn unscratch(path: &str) {
    let _ = sys::unlink(&c(path));
}

/// `/bin/sh` is the interpreter both exec probes need. Its absence is a
/// missing precondition, and the `ENOENT` it would otherwise produce reads
/// exactly like a denial.
pub(super) fn need_sh() -> Result<(), Outcome> {
    if exists("/bin/sh") {
        Ok(())
    } else {
        Err(Outcome::skip(
            Some(sys::ENOENT),
            "/bin/sh is absent, so there is no interpreter for the payload this \
             probe executes; the exec was not attempted",
        ))
    }
}

/// Run a path as a child process and report what happened to it.
///
/// Through [`sys::shed_after_fork`], because here a probe starts a process
/// with libstd and not with `sys::clone_fork`, and a probe makes one new
/// child for each probe. With no hook, each lock that another thread holds
/// is duplicated into this child at the `fork` and outlives the release of
/// its holder.
pub(super) fn run_payload(path: &str) -> Outcome {
    let mut cmd = std::process::Command::new(path);
    match sys::shed_after_fork(&mut cmd).status() {
        Ok(st) if st.success() => Outcome::ok(),
        Ok(st) => Outcome::skip(None, format!("the payload {path} ran and ended {st}; the exec itself succeeded")),
        Err(e) => match e.raw_os_error() {
            Some(n) => Outcome::denied(Errno(n)),
            None => Outcome::skip(None, format!("spawning {path} failed: {e}")),
        },
    }
}

/// Which of the three calls behind a detached mount answered.
///
/// Named, not compared as a string: creation, configuration and attachment
/// are separate verdicts, and a
/// caller cannot tell them apart from one errno.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Fsopen,
    Fsconfig,
    Fsmount,
}

impl Step {
    pub(super) fn label(self) -> &'static str {
        match self {
            Step::Fsopen => "fsopen(tmpfs)",
            Step::Fsconfig => "fsconfig(FSCONFIG_CMD_CREATE)",
            Step::Fsmount => "fsmount(tmpfs)",
        }
    }
}

/// A detached tmpfs mount fd, or the step that failed and its errno.
pub(super) fn detached_tmpfs() -> Result<i64, (Errno, Step)> {
    let fs = c("tmpfs");
    let fd = unsafe { sys::sys(sys::SYS_FSOPEN, [fs.ptr(), 0, 0, 0, 0, 0]) }.map_err(|e| (e, Step::Fsopen))?;
    let r = unsafe { sys::sys(sys::SYS_FSCONFIG, [fd as u64, sys::FSCONFIG_CMD_CREATE, 0, 0, 0, 0]) };
    if let Err(e) = r {
        let _ = sys::close(fd);
        return Err((e, Step::Fsconfig));
    }
    let mfd = unsafe { sys::sys(sys::SYS_FSMOUNT, [fd as u64, 0, 0, 0, 0, 0]) };
    let _ = sys::close(fd);
    mfd.map_err(|e| (e, Step::Fsmount))
}

pub(super) fn precondition(step: &str, e: Errno) -> Outcome {
    Outcome::skip(Some(e), format!("{step} failed with {} ({}), so this operation was never attempted", e.name(), e.0))
}
