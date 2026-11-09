//! The Windows guest driver: a disposable Validation OS machine driven
//! through a FAT mailbox. `TODO/milestones.md` T-1112.
//!
//! ⛔ **What this is for.** T-1112 asked for "a disposable guest that is not
//! Linux", and the entry was blocked because the machine tier's `full`
//! profile needs `/dev/kvm`, which the constrained machines podvm targets
//! do not have. The reference implementation refuses to run without
//! hardware acceleration, so its feature could not be ported as written.
//!
//! ⭐ **This is that feature with the refusal removed and the portability
//! taken seriously.** The design is the same shape as the reference, one
//! emulator child process, UEFI firmware, a per-run overlay so the base image
//! is never written, a FAT "mailbox" volume the host and guest both see, no
//! daemon and no libvirt, but:
//!
//! * it runs under `tcg` where `kvm` is missing, chosen from the machine
//!   tier's own profile rather than refused, so it works on the machines
//!   podvm was built for;
//! * the mailbox is built in process rather than by shelling out to
//!   `mkfs.fat` and `mtools`;
//! * autostart is a scheduled task rather than `cmd.exe`'s `AutoRun`, which
//!   is what the reference relies on and what does not fire on the
//!   Validation OS image;
//! * the command's exit code, stdout and stderr all come back, and a run
//!   that produces no result is an error rather than an empty success.
//!
//! ⚠ **The guest has no network, and the mailbox is the only channel.** A
//! command is a file the host writes before boot and a result is a file the
//! host reads after the guest powers itself off. That costs a boot per
//! command and buys a guest that cannot talk to the host except through a
//! volume the host owns.

// Edition 2024 makes `std::env::set_var` and `remove_var` unsafe. The tests
// change variables of the process, so a test build denies unsafe code in
// place of forbidding it, and only the test modules allow it.
#![cfg_attr(not(test), forbid(unsafe_code))]
#![cfg_attr(test, deny(unsafe_code))]

pub mod agent;
pub mod dos;
pub mod fat16;
pub mod fetch;
pub mod plan;
pub mod qmp;

use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

pub use agent::Code;
pub use fat16::Fat16;
pub use fetch::{Ceiling, FetchError};
pub use plan::{accel_for, argv, commit_argv, overlay_argv, Accel, Plan};

/// A command to run in the guest.
#[derive(Debug, Clone)]
pub struct Request {
    /// One `cmd.exe` command line, run verbatim by the guest. There is no
    /// argv on the Windows side: `&`, `|`, `>`, `%VAR%` and quoting inside
    /// this string are the guest shell's own.
    pub command: String,
    /// Echoed back with the result, so two runs in flight cannot be confused.
    pub token: String,
    /// How long the whole boot-and-run may take before the guest is killed.
    pub timeout: Duration,
}

/// What a guest run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
    pub token: String,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// What went wrong, in the vocabulary the caller can report on.
#[derive(Debug)]
pub enum Error {
    /// The emulator is not on this machine.
    NoEmulator(String),
    /// The machine tier establishes no profile, so there is nothing to run.
    NoAcceleration(String),
    /// An I/O step failed.
    Io(String),
    /// The guest never produced a result before the timeout. The second field
    /// is what happened to the emulator, never an assumption about it.
    Timeout(Duration, Stop),
    /// A result file was there but did not parse.
    BadResult(String),
}

/// What happened to an emulator this crate started and then had to stop.
///
/// ⛔ **Two states and no third, because a caller acts on the difference.** A
/// reader who is told only "it was stopped" cannot tell a reaped child from a
/// survivor, and those are opposite: the first needs nothing, the second needs
/// a survivor removed. [`Stop::Confirmed`] means the emulator was reaped by
/// this process, so the pid it held is gone and its own children were killed
/// as its process group. [`Stop::Attempted`] means the kill was issued and the
/// child was not observed to die inside the wait, so it may still be running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The emulator was reaped here. Its process group was killed first, so
    /// anything it forked is gone with it.
    Confirmed { pid: u32 },
    /// The stop was asked for through QMP and then issued as a signal, and
    /// the child was not observed to die within the wait. The message names
    /// the pid so a reader can go and look for it.
    Attempted { pid: u32 },
}

impl Stop {
    /// The emulator process this describes. Reported whether the stop was
    /// confirmed or only attempted: a reader looking for a survivor needs the
    /// pid in both cases.
    pub fn pid(self) -> u32 {
        match self {
            Stop::Confirmed { pid } | Stop::Attempted { pid } => pid,
        }
    }

    /// Whether the emulator was observed to die. A timeout message that does
    /// not carry this is asserting a stop nobody checked.
    pub fn confirmed(self) -> bool {
        matches!(self, Stop::Confirmed { .. })
    }
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Confirmed { pid } => {
                write!(
                    f,
                    "the emulator {pid} was killed and reaped, so its process group is gone"
                )
            }
            Stop::Attempted { pid } => write!(
                f,
                "the stop of emulator {pid} was attempted and not confirmed: \
                 QMP quit and SIGKILL to its process group were issued, and it \
                 was still not reaped within the wait, so it may still be running"
            ),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoEmulator(e) => write!(f, "{e}"),
            Error::NoAcceleration(e) => write!(f, "{e}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::Timeout(d, stop) => write!(
                f,
                "the guest did not power off within {}s; {stop}",
                d.as_secs()
            ),
            Error::BadResult(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

/// The mailbox a run is handed, and the results it is read back from.
///
/// ⚠ **The marker and the token are written here, and the token is the only
/// thing that tells one run's answer from another's.** A host that reuses a
/// mailbox directory must read [`Outcome::token`].
pub fn mailbox(agent_cmd: &str, request: &Request) -> Fat16 {
    let mut m = Fat16::new();
    // These three cannot fail: every name is 8.3, which the tests pin.
    m.put(agent::MARKER, b"podvm").expect("8.3 marker");
    m.put(agent::AGENT_NAME, agent_cmd.as_bytes())
        .expect("8.3 agent name");
    m.put(agent::SETUP_NAME, agent::SETUP_CMD.as_bytes())
        .expect("8.3 setup name");
    m.put(agent::CMD, agent::command_file(&request.command).as_bytes())
        .expect("8.3 command name");
    m.put(agent::GO, request.token.as_bytes())
        .expect("8.3 token name");
    m
}

/// Read a finished run's mailbox. `Ok(None)` is "the guest did not finish":
/// a missing `WQCODE.TXT` is not a zero.
///
/// ⛔ **The token is checked, not assumed.** A result whose token does not
/// match the request is refused rather than reported: that is a stale volume
/// or a second writer, and either one would otherwise be read as this run's
/// answer.
pub fn outcome(m: &Fat16, token: &str) -> Result<Option<Outcome>, Error> {
    let Some(code) = m.get(agent::CODE) else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&code).to_string();
    let parsed = agent::parse_code(&text)
        .ok_or_else(|| Error::BadResult(format!("{text:?} is not an exit code and a token")))?;
    if parsed.token != token {
        return Err(Error::BadResult(format!(
            "the mailbox holds a result for token {:?}, not {token:?}",
            parsed.token
        )));
    }
    Ok(Some(Outcome {
        code: parsed.code,
        token: parsed.token,
        stdout: m.get(agent::OUT).unwrap_or_default(),
        stderr: m.get(agent::ERR).unwrap_or_default(),
    }))
}

/// Stage one run: the mailbox on disk and the disposable overlay over the
/// read-only base image. Returns the in-memory mailbox, though the result is
/// read back from the file the guest wrote rather than from it.
///
/// ⚠ The per-run variable store is the caller's, because the caller also
/// copies it: see `Plan::firmware_vars`, which must already name a per-run
/// copy by the time this runs.
pub fn stage(
    qemu_img: &Path,
    base: &Path,
    plan: &Plan,
    agent_cmd: &str,
    request: &Request,
) -> Result<Fat16, Error> {
    let m = mailbox(agent_cmd, request);
    std::fs::write(&plan.mailbox, m.image())
        .map_err(|e| Error::Io(format!("{}: {e}", plan.mailbox.display())))?;
    // ⛔ `qemu-img create` refuses an existing file, and a scratch directory
    // that survived a killed run would otherwise fail here naming the overlay
    // rather than running.
    let _ = std::fs::remove_file(&plan.root);
    // ⛔ And the emulator refuses to **bind** a socket path that already
    // exists, so a stale one from a killed run stops the boot at startup
    // with an error about the monitor rather than about the guest.
    let _ = std::fs::remove_file(&plan.monitor);
    run_steps(&overlay_argv(qemu_img, base, &plan.root))?;
    Ok(m)
}

mod emulator;
use emulator::*;

/// Boot the guest, wait for it to power itself off, and read the result.
///
/// ⛔ **A run that times out is killed and reported, never `Ok`.** The agent
/// powers the machine off only after it has written the exit code, so a
/// timeout means the command is still running or the guest never reached the
/// mailbox, and both are failures.
pub fn run(plan: &Plan, request: &Request) -> Result<Outcome, Error> {
    // ⛔ **Never a pipe that nobody reads.** The emulator's streams were
    // piped and not drained until exit: a guest that makes the emulator
    // write more than one pipe buffer blocks it, and the run then reads as
    // a guest that did not power off. The error stream goes to a file in
    // the per-run directory, which `cleanup` removes with the run.
    let log = emulator_log(plan);
    let stderr = match &log {
        Some(path) => std::fs::File::create(path)
            .map(Stdio::from)
            .map_err(|e| Error::Io(format!("{}: {e}", path.display())))?,
        None => Stdio::null(),
    };
    let mut child = spawn_windows(plan, stderr)?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| Error::Io(e.to_string()))? {
            Some(s) => break Some(s),
            None if start.elapsed() >= request.timeout => break None,
            None => std::thread::sleep(Duration::from_millis(500)),
        }
    };
    if status.is_none() {
        // ⚠ The emulator that hit this is the one the run started. The agent
        // powers the machine off only after it has written the exit code, so
        // this is a run that is still going, and what it leaves behind is a
        // machine rather than a file.
        let stop = stop_emulator(plan, &mut child);
        return Err(Error::Timeout(request.timeout, stop));
    }
    // The emulator exited with a status the host does not act on, so it is
    // judged and dropped here rather than carried past the point that would
    // report it: the mailbox below is what this function answers from, and a
    // missing result is the error either way. ⛔ Do not turn it into `Ok`.
    let _ = status.expect("checked above");
    // Re-read the volume from disk: the guest is what wrote the result, and
    // the in-memory copy is the one we handed it, not the one it filled in.
    let bytes = std::fs::read(&plan.mailbox).map_err(|e| Error::Io(e.to_string()))?;
    let back = Fat16::from_image(bytes)
        .ok_or_else(|| Error::Io(format!("{}: not a mailbox", plan.mailbox.display())))?;
    outcome(&back, &request.token)?.ok_or_else(|| {
        let tail = log.as_deref().map(log_tail).unwrap_or_default();
        Error::BadResult(format!("the guest powered off with no result{tail}"))
    })
}

mod provision;
pub use provision::*;

/// Ask a running guest to stop, through QMP. Best effort: a guest that has
/// already powered itself off has no monitor left to answer, and that is the
/// success case, not a failure.
pub fn stop(plan: &Plan) {
    if let Ok(mut m) = qmp::Monitor::connect(&plan.monitor, Duration::from_secs(2)) {
        m.quit();
    }
}

/// ⭐ Docker's exit range is 0-124, and 125 is `podvm` itself failing. A
/// guest status of 125 or above would therefore read as a podvm failure,
/// so it is capped rather than passed through: `None` is "this status
/// cannot be returned as-is", and the caller names it.
pub fn cap_status(code: i32) -> Option<i32> {
    (0..=124).contains(&code).then_some(code)
}

mod scratch;
pub use scratch::*;

/// A character to a `sendkey` name, `(name, shifted)`, for a US layout.
fn keys(c: char) -> Option<(&'static str, bool)> {
    Some(match c {
        'a'..='z' => (LETTER[(c as u8 - b'a') as usize], false),
        'A'..='Z' => (LETTER[(c as u8 - b'A') as usize], true),
        '0'..='9' => (DIGIT[(c as u8 - b'0') as usize], false),
        ' ' => ("spc", false),
        '!' => ("1", true),
        '"' => ("apostrophe", true),
        '#' => ("3", true),
        '$' => ("4", true),
        '%' => ("5", true),
        '&' => ("7", true),
        '\'' => ("apostrophe", false),
        '(' => ("9", true),
        ')' => ("0", true),
        '*' => ("8", true),
        '+' => ("equal", true),
        ',' => ("comma", false),
        '-' => ("minus", false),
        '.' => ("dot", false),
        '/' => ("slash", false),
        ':' => ("semicolon", true),
        ';' => ("semicolon", false),
        '<' => ("comma", true),
        '=' => ("equal", false),
        '>' => ("dot", true),
        '?' => ("slash", true),
        '@' => ("2", true),
        '[' => ("bracket_left", false),
        '\\' => ("backslash", false),
        ']' => ("bracket_right", false),
        '^' => ("6", true),
        '_' => ("minus", true),
        '`' => ("grave_accent", false),
        '{' => ("bracket_left", true),
        '|' => ("backslash", true),
        '}' => ("bracket_right", true),
        '~' => ("grave_accent", true),
        _ => return None,
    })
}

const LETTER: [&str; 26] = [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
];
const DIGIT: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

#[cfg(test)]
#[allow(unsafe_code)]
mod tests;
