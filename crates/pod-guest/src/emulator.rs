use super::*;

/// Run one argv to completion and answer its status.
///
/// ⚠ **The status comes back with `Ok`, and is not read for success.** Two
/// callers need different things from the same exit: `stage` and the
/// provisioning commit need only that it did not fail, and the stop path
/// needs to know that a signal killed it. Reporting success and reporting the
/// status are different questions, so the status is the answer and each caller
/// judges it.
pub(crate) fn run_steps(argv: &[String]) -> Result<ExitStatus, Error> {
    let (prog, rest) = argv
        .split_first()
        .ok_or_else(|| Error::Io("empty argv".into()))?;
    let out = Command::new(prog)
        .args(rest)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| Error::Io(format!("{prog}: {e}")))?;
    if !out.status.success() {
        return Err(Error::Io(format!(
            "{prog} {}: {}",
            rest.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out.status)
}

/// Spawn the emulator with `args`, in a process group of its own.
///
/// ⛔ **The group is the point, and it is set before the child exists.**
/// `process_group(0)` puts the child in a new group whose id is its own pid,
/// so [`stop_emulator`] can signal the whole group and take anything the
/// emulator forked with it. A kill of the direct child alone leaves a helper
/// running, and a helper that holds the disk image open is what leaves a
/// machine in a state the next run cannot enter. Neither `process_group` nor
/// the signal below is `unsafe`, so [`forbid(unsafe_code)`] stands.
///
/// ⚠ **The argv is the caller's, not [`plan::argv`]'s.** The two flavors have
/// different machine lines: the Windows guest is q35 with two NVMe devices and
/// the DOS guest is `pc` with IDE disks, and one spawn serving both would
/// change whichever line it did not own. `plan` supplies the binary and the
/// stop needs it; the command line stays with the flavor that builds it.
pub(crate) fn spawn_emulator(plan: &Plan, args: &[String], stderr: Stdio) -> Result<Child, Error> {
    let mut cmd = Command::new(&plan.emulator);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .process_group(0);
    cmd.spawn()
        .map_err(|e| Error::NoEmulator(format!("{}: {e}", plan.emulator.display())))
}

/// Spawn the Windows guest, whose machine line is [`plan::argv`]'s.
pub(crate) fn spawn_windows(plan: &Plan, stderr: Stdio) -> Result<Child, Error> {
    spawn_emulator(plan, &argv(plan), stderr)
}

/// How long [`stop_emulator`] waits for a killed emulator to be reaped. A
/// killed process is reaped as soon as the kernel has torn it down, and the
/// group is dead with it, so this is a bound and not a wait that has to
/// elapse.
const STOP_WAIT: Duration = Duration::from_secs(5);
/// How long the QMP `quit` is given to work before the signal is sent. ⛔ The
/// guest is the wrong thing to wait for: it is what timed out. This bounds
/// how long the emulator keeps running on the polite path, nothing more.
const QUIT_WAIT: Duration = Duration::from_secs(2);

/// Stop the emulator this crate started, and report what was observed.
///
/// ⛔ **Three steps in a fixed order, and each one answers with evidence.**
/// QMP `quit` first, so a guest that can still reach its monitor ends without
/// a signal. Then `SIGKILL` to the whole process group from
/// [`spawn_emulator`], because `SIGKILL` is what a stuck emulator needs and a
/// helper it forked is in that group too. Then the child is waited for, and
/// the reaped status *is* the confirmation: [`Child::try_wait`] answers
/// `Ok(Some(status))` only for a process this process reaped, and a process
/// that was never reaped is still running however many signals it took.
///
/// ⚠ **The QMP step is best effort and its failure changes nothing.** The
/// monitor is gone once the emulator is stopping, and a guest that has already
/// powered itself off has no socket to answer on. Both are the success case,
/// so a refusal from `quit` is not reported as a failure of the stop.
pub(crate) fn stop_emulator(plan: &Plan, child: &mut Child) -> Stop {
    stop_with(plan, child, QUIT_WAIT, STOP_WAIT)
}

/// [`stop_emulator`] with both bounds supplied, so the wait for a stop can be
/// tested without a signal that has to be delivered.
pub(super) fn stop_with(
    plan: &Plan,
    child: &mut Child,
    quit_wait: Duration,
    reap_wait: Duration,
) -> Stop {
    let pid = child.id();
    stop(plan);
    let start = Instant::now();
    while start.elapsed() < quit_wait {
        if child.try_wait().ok().flatten().is_some() {
            // ⛔ A guest that obeyed `quit` needs no signal at all, and a kill
            // here would be aimed at a pid the kernel may already have reused.
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
            // ⛔ A `try_wait` that cannot answer is not a clean exit, so it is
            // not read as one: the worst report is the honest one.
            Err(_) => return Stop::Attempted { pid },
        }
        if start.elapsed() >= reap_wait {
            return Stop::Attempted { pid };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `kill(-pgid, SIGKILL)`, the group form of the signal.
///
/// ⛔ **The group, and the emulator's pid as the negative target.** The group
/// id is the child's pid because [`spawn_emulator`] asked for `process_group(0)`
/// before the child existed, so `-pid` is this emulator's group and nothing
/// else: the driver is in its own group and is never a target here.
///
/// ⚠ **The errno is not reported, and the reap is what reports.** `ESRCH`
/// means the group is already gone, which is the outcome this wanted; any
/// other errno leaves the child alive, and the reap loop below is what turns
/// that into [`Stop::Attempted`]. A second error path here would say the same
/// thing twice, and the one that cannot be wrong is the one that observed it.
fn kill_group(pid: u32) {
    let _ = podvm_probe::sys::kill(-(pid as i64), SIGKILL as i64);
}

/// `SIGKILL`, 9 in `asm-generic/signal.h` and in every architecture's
/// `general.rs` the probe reads its numbers from. ⛔ **Not `SIGTERM`.** A
/// guest that has to be killed has already had its chance: the QMP `quit`
/// above is the polite stop, and a term signal between it and the kill would
/// add a wait that has been measured not to finish.
const SIGKILL: u8 = 9;
