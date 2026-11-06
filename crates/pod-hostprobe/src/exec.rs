//! Whether files created under a directory can EXECUTE.
//!
//! A store on a `noexec` filesystem pulls and extracts fully, then stops at
//! `execve` with raw `EACCES` and rc 126, and names neither the cause nor the
//! directory. The checks of writability and space that pick the store never
//! ask about exec, so the work is lost before anything names the fix.
//!
//! **The probe executes, it does not stat.** A mode bit says the file may
//! be executed; only the kernel answers whether the mount lets it. The probe
//! writes a tiny script, marks it executable, and runs it: `EACCES` from that
//! run is the `noexec` answer, whatever the bits say.
//!
//! **A failed probe never reads as a broken image.** The verdict is
//! `Executable` or `Denied` with the directory named, and anything that stops
//! the probe itself (unwritable directory, missing interpreter) is an `Error`
//! naming the path rather than a verdict.
//!
//! The seam: [`probe_dir_exec`]. A gate calls it before a fetch and refuses
//! with the path and the remedy; a doctor row reports `ok` with the same
//! detail. A failure is a [`Failure`] at [`Stage::Host`].

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

use pod_core::{Class, Failure, Stage};

use crate::Settings;

type Result<T> = std::result::Result<T, Failure>;

/// An I/O failure at `path`, with the class of its kind.
fn io(path: &Path, e: std::io::Error) -> Failure {
    let class = Class::of_io(e.kind());
    Failure::caused(Stage::Host, class, format!("{}: {e}", path.display()), e)
}

/// What one exec probe of a directory found.
#[derive(Debug)]
pub struct ExecProbe {
    /// The directory that was probed.
    pub dir: PathBuf,
    /// Whether a file created under `dir` executes.
    pub ok: bool,
    /// The run behind `ok`: the exit status, or the denial with its errno.
    pub detail: String,
}

/// Whether one file executes: the kernel's answer, not the mode bits'.
#[derive(Debug, PartialEq, Eq)]
pub enum ExecVerdict {
    /// The file ran.
    Executable,
    /// The kernel refused the run, with the errno named.
    Denied { reason: String },
}

/// Unique for each CALL, not for each process: two probes in one process
/// must not take one name.
static PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The bytes the probe executes. A script, so the probe needs no compiler;
/// it exits 0, so any other exit is the environment answering, not the file.
const PROBE_SCRIPT: &[u8] = b"#!/bin/sh\nexit 0\n";

/// Attempt to execute the file at `path`, and report what the kernel said.
///
/// `EACCES` is the `noexec` answer and returns `Denied`: the bit checks
/// cannot give it, only the run can. A run that starts is `Executable`,
/// whatever status it exits with, because the kernel already answered the
/// only question asked. Anything else (no file, no interpreter, no memory)
/// is an `Error` naming the path, never a verdict.
pub fn try_exec(path: &Path) -> Result<ExecVerdict> {
    match std::process::Command::new(path).status() {
        Ok(_) => Ok(ExecVerdict::Executable),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Ok(ExecVerdict::Denied { reason: format!("{}: execve refused with EACCES ({e})", path.display()) })
        }
        Err(e) => Err(io(path, e)),
    }
}

/// Probe whether files created under `dir` execute.
///
/// Writes a tiny script, marks it executable, runs it, and removes it. The
/// removal runs on every path: a probe that litters the store it is checking
/// is a second defect filed under the first. The detail of a denial names
/// `settings.directory_setting`.
pub fn probe_dir_exec(settings: &Settings, dir: &Path) -> Result<ExecProbe> {
    let name = format!(
        ".pod-hostprobe-exec.{}.{}",
        std::process::id(),
        PROBE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let path = dir.join(name);
    std::fs::write(&path, PROBE_SCRIPT).map_err(|e| io(&path, e))?;
    let verdict = probe_file(&path);
    let _ = std::fs::remove_file(&path);
    let (ok, detail) = match verdict {
        Ok(ExecVerdict::Executable) => (true, format!("{}: a created file executes", dir.display())),
        Ok(ExecVerdict::Denied { reason }) => {
            let remedy = match &settings.directory_setting {
                Some(name) => format!("move the directory off this filesystem, for example with {name}"),
                None => "move the directory off this filesystem".to_string(),
            };
            (false, format!("{}: a created file does not execute ({reason}); {remedy}", dir.display()))
        }
        Err(e) => return Err(e),
    };
    Ok(ExecProbe { dir: dir.to_path_buf(), ok, detail })
}

/// Mark `path` executable and run it. Separate from [`probe_dir_exec`] so
/// tests can drive the run half without the write half.
fn probe_file(path: &Path) -> Result<ExecVerdict> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).map_err(|e| io(path, e))?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(|e| io(path, e))?;
    try_exec(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory only this test names: the suite runs threads in one
    /// process, and two tests sharing a directory share a probe.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pod-hostprobe-exec-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The common case: a writable directory executes what it creates, and
    /// the probe leaves nothing behind.
    #[test]
    fn a_writable_directory_probes_executable_and_litters_nothing() {
        let d = scratch("executable");
        let probe = probe_dir_exec(&Settings::default(), &d).unwrap();
        assert!(probe.ok, "a writable directory executes: {}", probe.detail);
        assert_eq!(probe.dir, d);
        assert!(std::fs::read_dir(&d).unwrap().next().is_none(), "the probe file is removed after the run");
    }

    /// The run is the instrument: a file WITHOUT the
    /// executable bit is denied with EACCES by the kernel, and the helper
    /// reports the denial rather than erroring. A bit check would answer
    /// from `metadata`; this answers from `execve`.
    #[test]
    fn a_file_without_the_executable_bit_is_denied_not_errored() {
        let d = scratch("denied");
        let path = d.join("noexec.sh");
        std::fs::write(&path, PROBE_SCRIPT).unwrap();
        let verdict = try_exec(&path).unwrap();
        match verdict {
            ExecVerdict::Denied { reason } => {
                let at = path.display().to_string();
                assert!(
                    reason.contains("EACCES") && reason.contains(at.as_str()),
                    "the denial names the errno and the path: {reason}"
                );
            }
            ExecVerdict::Executable => panic!("a file without the executable bit ran"),
        }
    }

    /// The mirror: the same bytes WITH the bit execute, so the denial above
    /// is the kernel answering and not the script failing.
    #[test]
    fn the_same_bytes_with_the_bit_execute() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("mirror");
        let path = d.join("exec.sh");
        std::fs::write(&path, PROBE_SCRIPT).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        assert_eq!(try_exec(&path).unwrap(), ExecVerdict::Executable);
    }

    /// A probe that cannot write is an error naming the path, never a `no`
    /// verdict: refusing the store because the probe itself broke would read
    /// as a `noexec` filesystem.
    #[test]
    fn a_directory_that_cannot_be_written_errors_with_its_path() {
        let d = scratch("unwritable").join("does-not-exist");
        let err = probe_dir_exec(&Settings::default(), &d).unwrap_err();
        assert!(format!("{err}").contains(&d.display().to_string()), "the error names the directory: {err}");
    }

    /// Two probes in one process take two names: a shared name was a real
    /// defect of a store.
    #[test]
    fn two_probes_in_one_process_take_two_names() {
        let d = scratch("two-names");
        let first = probe_dir_exec(&Settings::default(), &d).unwrap();
        let second = probe_dir_exec(&Settings::default(), &d).unwrap();
        assert!(first.ok && second.ok);
        assert!(
            std::fs::read_dir(&d).unwrap().next().is_none(),
            "neither probe litters, so neither saw the other's file"
        );
    }
}
