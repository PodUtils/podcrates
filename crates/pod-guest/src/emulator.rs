//! The emulator process: the tool steps (`qemu-img`), the spawn in a
//! process group of its own, the bounded stop, and the log of its error
//! stream.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use pod_core::Class;

use crate::{Plan, Result, Settings, Stop, failure, io_at, scratch};

/// Run one argv to its end and return its status.
///
/// A status that is not success is a failure that names the program and
/// its own stderr. A status of success comes back, so that a caller can
/// read it.
pub fn run_steps(argv: &[String]) -> Result<ExitStatus> {
    let (prog, rest) = argv.split_first().ok_or_else(|| failure(Class::Usage, "empty argv"))?;
    let out = Command::new(prog)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| io_at(Path::new(prog), e))?;
    if !out.status.success() {
        return Err(failure(
            Class::Unavailable,
            format!("{prog} {}: {}", rest.join(" "), String::from_utf8_lossy(&out.stderr).trim()),
        ));
    }
    Ok(out.status)
}

/// Start the emulator of `plan` with `args`, in a process group of its own.
///
/// `process_group(0)` makes a new group whose id is the pid of the child,
/// before the child runs. [`stop_emulator`] then kills the whole group,
/// with each helper that the emulator forked: a helper that holds the disk
/// image open blocks the next run.
///
/// The argv is the caller's: each flavour has its own machine line
/// ([`crate::plan::argv`], [`crate::dos::argv`]).
pub fn spawn_emulator(plan: &Plan, args: &[String], stderr: Stdio) -> Result<Child> {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new(&plan.emulator);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(stderr).process_group(0);
    cmd.spawn().map_err(|e| io_at(&plan.emulator, e))
}

/// Start the guest of the UEFI flavour, whose machine line is
/// [`crate::plan::argv`].
pub(crate) fn spawn_uefi(plan: &Plan, stderr: Stdio) -> Result<Child> {
    plan.options.check()?;
    spawn_emulator(plan, &crate::argv(plan), stderr)
}

/// The bound of the wait for a killed emulator. The kernel reaps a killed
/// process when it has torn it down, so the wait usually ends at once.
pub const STOP_WAIT: Duration = Duration::from_secs(5);
/// The time that QMP `quit` gets before the signal. The guest timed out
/// already, so this bounds only the polite stop.
pub const QUIT_WAIT: Duration = Duration::from_secs(2);

/// Stop the emulator that this crate started, and report what was seen.
///
/// Three steps in a fixed order: QMP `quit`, so that a guest that can
/// reach its monitor ends with no signal; `SIGKILL` to the process group
/// of [`spawn_emulator`]; a wait for the child. Only a reaped status is a
/// confirmation: [`Child::try_wait`] gives `Ok(Some(_))` only for a child
/// that this process reaped.
///
/// A failure of `quit` changes nothing: an emulator that stops or a guest
/// that powered off has no monitor to answer.
pub fn stop_emulator(plan: &Plan, child: &mut Child) -> Stop {
    stop_with(plan, child, QUIT_WAIT, STOP_WAIT)
}

/// [`stop_emulator`] with both bounds from the caller.
pub fn stop_with(plan: &Plan, child: &mut Child, quit_wait: Duration, reap_wait: Duration) -> Stop {
    let pid = child.id();
    crate::stop(plan);
    let start = Instant::now();
    while start.elapsed() < quit_wait {
        if child.try_wait().ok().flatten().is_some() {
            // The emulator obeyed `quit`. No signal: the kernel can give
            // the pid to another process now.
            return Stop::Confirmed { pid };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    kill_group(pid);
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Stop::Confirmed { pid },
            Ok(None) => {}
            // A `try_wait` that cannot answer is not a clean exit.
            Err(_) => return Stop::Attempted { pid },
        }
        if start.elapsed() >= reap_wait {
            return Stop::Attempted { pid };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `kill(-pgid, SIGKILL)`: the group of the emulator, whose id is its pid
/// ([`spawn_emulator`]). The caller is in another group and is never a
/// target.
///
/// The errno is not reported: `ESRCH` is a group that is gone, and each
/// other errno leaves the child alive, which the reap loop reports as
/// [`Stop::Attempted`].
fn kill_group(pid: u32) {
    let _ = pod_hostprobe::sys::kill(-i64::from(pid), SIGKILL);
}

/// `SIGKILL`, 9 on each architecture. Not `SIGTERM`: the guest had its
/// chance with QMP `quit`.
const SIGKILL: i64 = 9;

/// The file of the error stream of the emulator, in the per-run directory;
/// `None` when the plan has no directory that [`crate::cleanup`] removes.
pub(crate) fn emulator_log(s: &Settings, plan: &Plan) -> Option<PathBuf> {
    let dir = crate::scratch_of(plan)?;
    scratch::is_scratch(s, &dir).then(|| dir.join("emulator.log"))
}

/// The last lines of the error stream of the emulator, for the text of a
/// failure. Empty when the file is absent or empty.
pub(crate) fn log_tail(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_default();
    // Only the end of the file, and each line cut short: the emulator
    // chooses the size of the stream.
    let text = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(4096)..]);
    let lines: Vec<String> =
        text.lines().filter(|l| !l.trim().is_empty()).map(|l| l.chars().take(200).collect()).collect();
    if lines.is_empty() {
        return String::new();
    }
    let start = lines.len().saturating_sub(5);
    format!("; the emulator said: {}", lines[start..].join(" | "))
}
