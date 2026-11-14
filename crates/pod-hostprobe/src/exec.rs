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
//! run is the kernel's refusal, whatever the bits say.
//!
//! **`EACCES` alone does not name the cause.** A `noexec` mount refuses
//! with it, and so does a policy on the path or on the file (an LSM, a
//! seccomp filter, a sandbox). The probe reads the flags of the mount with
//! `statfs(2)`: it names the filesystem only where the mount is `noexec`.
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
use crate::sys::{CBuf, ST_NOEXEC, statfs};

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
/// `EACCES` returns `Denied`: the bit checks cannot give it, only the run
/// can. It does not tell a `noexec` mount from a policy on the path. A run that starts is `Executable`,
/// whatever status it exits with, because the kernel already answered the
/// only question asked. Anything else (no file, no interpreter, no memory)
/// is an `Error` naming the path, never a verdict.
///
/// `ETXTBSY` is tried again, for at most [`BUSY_TRIES`] times: a fork of
/// another thread holds a copy of the descriptor that wrote the file until
/// that child execs or exits, and the kernel refuses the exec of a file
/// that is open for writing.
pub fn try_exec(path: &Path) -> Result<ExecVerdict> {
    let mut tries = 1;
    loop {
        match std::process::Command::new(path).status() {
            Ok(_) => return Ok(ExecVerdict::Executable),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                let reason = format!("{}: execve refused with EACCES ({e})", path.display());
                return Ok(ExecVerdict::Denied { reason });
            }
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < BUSY_TRIES => {
                tries += 1;
                std::thread::sleep(BUSY_PAUSE);
            }
            Err(e) => return Err(io(path, e)),
        }
    }
}

/// The bound of the tries of [`try_exec`] on `ETXTBSY`: 250 tries, 2 ms
/// apart, so at most about 0.5 s.
pub const BUSY_TRIES: u32 = 250;
const BUSY_PAUSE: std::time::Duration = std::time::Duration::from_millis(2);

/// Probe whether files created under `dir` execute.
///
/// Writes a tiny script, marks it executable, runs it, and removes it. The
/// removal runs on every path: a probe that litters the store it is checking
/// is a second defect filed under the first. The detail of a denial names
/// the cause that the flags of the mount establish (`denial_cause`), and
/// `settings.directory_setting` where the remedy is to move the directory.
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
            let cause = denial_cause(mount_noexec(dir), settings.directory_setting.as_deref());
            (false, format!("{}: a created file does not execute ({reason}); {cause}", dir.display()))
        }
        Err(e) => return Err(e),
    };
    Ok(ExecProbe { dir: dir.to_path_buf(), ok, detail })
}

/// The `noexec` flag of the mount that holds `dir`, from `statfs(2)`.
/// `None` where the call fails or the kernel did not fill `f_flags` (an
/// emulator): that is not the same answer as `false`.
fn mount_noexec(dir: &Path) -> Option<bool> {
    let path = CBuf::new(&dir.to_string_lossy())?;
    statfs(&path).ok()?.flag(ST_NOEXEC)
}

/// The cause and the remedy of an `EACCES` from the run, from the `noexec`
/// flag of the mount (`None`: not known).
///
/// Only a `noexec` mount is fixed by a move to another filesystem. Where
/// the mount is not `noexec`, a policy on the path or on the file refuses
/// the run, and a move off the filesystem can fix nothing.
fn denial_cause(noexec: Option<bool>, setting: Option<&str>) -> String {
    let move_off = match setting {
        Some(name) => format!("move the directory off this filesystem, for example with {name}"),
        None => "move the directory off this filesystem".to_string(),
    };
    match noexec {
        Some(true) => format!("the mount of the directory is noexec; {move_off}"),
        Some(false) => "the mount of the directory is not noexec, so a policy on the path or on the file \
             refuses the run (an LSM such as SELinux, AppArmor or Landlock, a seccomp filter, or a \
             sandbox); check the policy of this path and the audit log of the kernel"
            .to_string(),
        None => format!(
            "the flags of the mount could not be read, so the cause is not known: a noexec mount, or a \
             policy on the path or on the file (an LSM, a seccomp filter, a sandbox); if the mount is \
             noexec, {move_off}; else check the policy of this path"
        ),
    }
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
        pod_testkit::need_tool!("/bin/sh");
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
        pod_testkit::need_tool!("/bin/sh");
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("mirror");
        let path = d.join("exec.sh");
        std::fs::write(&path, PROBE_SCRIPT).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        assert_eq!(try_exec(&path).unwrap(), ExecVerdict::Executable);
    }

    /// A writer of the file that closes a moment later (as the copy of the
    /// descriptor in the fork of another thread does) does not make the run
    /// an error: the run waits for it, within a bound.
    #[test]
    fn a_file_busy_for_a_moment_executes_after_the_writer_closes() {
        pod_testkit::need_tool!("/bin/sh");
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("busy");
        let path = d.join("busy.sh");
        std::fs::write(&path, PROBE_SCRIPT).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let writer = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        let (held, wait) = std::sync::mpsc::channel();
        let closer = std::thread::spawn(move || {
            held.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            drop(writer);
        });
        wait.recv().unwrap();
        assert_eq!(try_exec(&path).unwrap(), ExecVerdict::Executable);
        closer.join().unwrap();
    }

    /// The flag of the mount decides the remedy: only a `noexec` mount gets
    /// the move off the filesystem, and a mount that is not `noexec` names
    /// a policy on the path and never the move (PodUtils/podvm#335).
    #[test]
    fn the_flag_of_the_mount_decides_the_remedy() {
        let noexec = denial_cause(Some(true), Some("$STORE"));
        assert!(noexec.contains("is noexec"), "{noexec}");
        assert!(noexec.contains("move the directory off this filesystem, for example with $STORE"), "{noexec}");
        let policy = denial_cause(Some(false), Some("$STORE"));
        assert!(policy.contains("is not noexec") && policy.contains("a policy on the path"), "{policy}");
        assert!(policy.contains("check the policy of this path"), "{policy}");
        assert!(!policy.contains("move") && !policy.contains("$STORE"), "no move where the mount executes: {policy}");
        let unknown = denial_cause(None, None);
        assert!(unknown.contains("not known") && unknown.contains("if the mount is noexec"), "{unknown}");
        assert!(unknown.contains("move the directory off this filesystem;"), "{unknown}");
    }

    /// End to end on a real `noexec` mount: the probe reads the flag of the
    /// mount it refused on, and names the filesystem. It needs root to
    /// mount a tmpfs.
    #[test]
    fn a_noexec_mount_is_named_by_the_probe() {
        pod_testkit::need_root!();
        pod_testkit::need_tool!("/bin/sh");
        let d = scratch("noexec-mount");
        let at = CBuf::new(&d.to_string_lossy()).unwrap();
        let (src, fs, data) = (CBuf::new("tmpfs").unwrap(), CBuf::new("tmpfs").unwrap(), CBuf::new("").unwrap());
        if let Err(e) = crate::sys::mount_data(&src, &at, &fs, crate::sys::MS_NOEXEC, &data) {
            pod_testkit::host::skip_line(&format!("a tmpfs does not mount here ({})", e.name()));
            return;
        }
        let probe = probe_dir_exec(&Settings::default(), &d);
        let _ = crate::sys::umount_detach(&at);
        let probe = probe.unwrap();
        assert!(!probe.ok, "a file on a noexec tmpfs ran: {}", probe.detail);
        assert!(probe.detail.contains("is noexec; move the directory off this filesystem"), "{}", probe.detail);
    }

    /// `mount_noexec` reads the mount that holds the directory: its answer
    /// agrees with the line of that mount in `/proc/self/mounts`. A `statfs`
    /// of the wrong path or a wrong bit fails here.
    #[test]
    fn the_noexec_flag_agrees_with_the_mount_table() {
        let d = scratch("noexec-flag");
        let Ok(mounts) = std::fs::read_to_string("/proc/self/mounts") else {
            return;
        };
        // The last line of a mount point that holds the directory is the
        // mount on top, which `statfs(2)` answers for.
        let mut want = None;
        for line in mounts.lines() {
            let f: Vec<&str> = line.split(' ').collect();
            let (Some(point), Some(opts)) = (f.get(1), f.get(3)) else {
                continue;
            };
            if point.contains('\\') || !d.starts_with(point) {
                continue;
            }
            want = Some(opts.split(',').any(|o| o == "noexec"));
        }
        let (Some(want), Some(got)) = (want, mount_noexec(&d)) else {
            // No line, or an emulator that fills no `f_flags`.
            return;
        };
        assert_eq!(got, want, "statfs of {} against /proc/self/mounts", d.display());
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
        pod_testkit::need_tool!("/bin/sh");
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
