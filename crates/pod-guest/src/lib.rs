//! A disposable QEMU guest: one emulator child, a per-run overlay over a
//! read-only base image, and a FAT16 mailbox volume that the host and the
//! guest both see. There is no daemon and no libvirt.
//!
//! Four rules:
//!
//! 1. The accelerator comes from the machine profile of `pod-hostprobe`:
//!    `kvm` where the `full` profile holds, `tcg` where only `tcg` holds,
//!    and a refusal where neither holds ([`plan::accel_for`]). TCG is
//!    slower, and it is not a refusal.
//! 2. The base image is never written. A run writes to a qcow2 overlay
//!    with the base as its read-only backing file; a provision commits the
//!    overlay into a new standalone image ([`provision`]).
//! 3. The mailbox is the only channel. The guest has no network
//!    (`-nic none`). The host writes the command and a token before the
//!    boot, and reads the exit code, stdout and stderr after the guest
//!    stops. A result with another token is refused ([`outcome`]).
//! 4. A run that times out is stopped and reported, never `Ok`. The stop
//!    says if the emulator was reaped ([`Stop`]).
//!
//! Two flavours share the mailbox, the token, QMP, the per-run directory
//! and the stop: the UEFI flavour ([`plan::argv`]: q35, OVMF, NVMe, a
//! `cmd.exe` agent that a scheduled task starts, [`agent`]) and the DOS
//! flavour ([`dos`]: SeaBIOS, IDE, a line typed into the console).
//!
//! The crate reads no variable of the environment and prints nothing. The
//! caller gives the names and the places in [`Settings`]. At run time the
//! crate starts the emulator and `qemu-img` that the caller names.
//!
//! The crate is Linux only. On another target it is empty.
#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

pub mod agent;
pub mod dos;
pub mod emulator;
pub mod fat16;
pub mod fetch;
pub mod plan;
pub mod qmp;

mod provision;
mod scratch;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use pod_core::{Class, Failure, Stage};

pub use agent::Code;
pub use fat16::Fat16;
pub use fetch::{Ceiling, Refusal};
pub use plan::{Accel, Plan, accel_for, argv, commit_argv, overlay_argv};
pub use provision::provision;
pub use scratch::{RunGuard, cleanup, scratch_dir, scratch_of};

pub type Result<T> = std::result::Result<T, Failure>;

/// The names and the places that the caller gives. The caller fills it one
/// time and gives it to each call that needs it.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The directory that holds each per-run directory.
    pub scratch_root: PathBuf,
    /// The start of the name of a per-run directory: `PREFIX`, the pid,
    /// `-`, the tag. [`cleanup`] removes only a directory with this prefix.
    pub scratch_prefix: String,
    /// Keep each per-run directory after the run, so that a person can
    /// examine a failed run.
    pub keep: bool,
    /// The content of the marker file of the mailbox ([`agent::MARKER`]).
    pub marker: Vec<u8>,
    /// The OEM name in the boot sector of the mailbox volume: at most 8
    /// printable ASCII characters.
    pub oem: String,
}

impl Settings {
    /// The settings for per-run directories in `scratch_root`, with the
    /// prefix `pod-guest-`, no keep, the marker `pod-guest` and the OEM
    /// name `PODGUEST`.
    pub fn new(scratch_root: impl Into<PathBuf>) -> Settings {
        Settings {
            scratch_root: scratch_root.into(),
            scratch_prefix: "pod-guest-".to_string(),
            keep: false,
            marker: b"pod-guest".to_vec(),
            oem: "PODGUEST".to_string(),
        }
    }
}

/// An I/O failure at `path`: the text is `PATH: CAUSE`, and the cause stays
/// in the chain.
pub(crate) fn io_at(path: &Path, e: std::io::Error) -> Failure {
    Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("{}: {e}", path.display()), e)
}

/// A failure of the guest, with no I/O cause.
pub(crate) fn failure(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, class, text)
}

/// The same failure with `before: ` in front of its text. The stage, the
/// class and the cause stay.
pub(crate) fn context(before: &str, f: Failure) -> Failure {
    Failure::caused(f.stage, f.class, format!("{before}: {f}"), f)
}

/// A command to run in the guest.
#[derive(Debug, Clone)]
pub struct Request {
    /// One `cmd.exe` command line. The guest runs it as it is: `&`, `|`,
    /// `>`, `%VAR%` and the quotes are the operators of the guest shell.
    pub command: String,
    /// The token of this run. The guest writes it back with the result, so
    /// that the host does not read the result of another run.
    pub token: String,
    /// The limit of the boot and the run. At the limit the emulator stops.
    pub timeout: Duration,
}

/// The result of a guest run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
    pub token: String,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// What happened to an emulator that this crate started and then stopped.
///
/// There are two states, because a caller acts on the difference:
/// [`Stop::Confirmed`] needs no action; [`Stop::Attempted`] can leave a
/// process that the caller must remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// This process reaped the emulator. Its process group was killed
    /// first, so each process that it forked is gone too.
    Confirmed { pid: u32 },
    /// QMP `quit` and then `SIGKILL` to the process group were sent, and
    /// the child did not die in the wait. It can still run.
    Attempted { pid: u32 },
}

impl Stop {
    /// The pid of the emulator, in both states: a caller that looks for a
    /// survivor needs it.
    pub fn pid(self) -> u32 {
        match self {
            Stop::Confirmed { pid } | Stop::Attempted { pid } => pid,
        }
    }

    /// True when the emulator was reaped.
    pub fn confirmed(self) -> bool {
        matches!(self, Stop::Confirmed { .. })
    }
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Confirmed { pid } => {
                write!(f, "the emulator {pid} was killed and reaped, so its process group is gone")
            }
            Stop::Attempted { pid } => write!(
                f,
                "the stop of emulator {pid} was attempted and not confirmed: QMP quit and SIGKILL to its \
                 process group were sent, and it was not reaped in the wait, so it may still be running"
            ),
        }
    }
}

/// The cause of a [`Class::Timeout`] failure of a run: the limit, and what
/// happened to the emulator. [`timeout_of`] finds it in a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout {
    pub after: Duration,
    pub stop: Stop,
}

impl std::fmt::Display for Timeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the guest did not power off within {}s; {}", self.after.as_secs(), self.stop)
    }
}

impl std::error::Error for Timeout {}

impl Timeout {
    /// The failure of a run that reached its limit.
    pub fn failure(self) -> Failure {
        Failure::caused(Stage::Host, Class::Timeout, self.to_string(), self)
    }
}

/// The [`Timeout`] in the chain of `f`, when `f` is the failure of a run
/// that reached its limit.
pub fn timeout_of(f: &Failure) -> Option<&Timeout> {
    let mut next = std::error::Error::source(f);
    while let Some(e) = next {
        if let Some(t) = e.downcast_ref::<Timeout>() {
            return Some(t);
        }
        next = e.source();
    }
    None
}

/// The mailbox of a run: the marker, the agent, the installer, the command
/// and the token.
///
/// The token tells the result of one run from the result of another. A
/// caller that uses a mailbox directory again must read [`Outcome::token`].
pub fn mailbox(s: &Settings, agent_cmd: &str, request: &Request) -> Result<Fat16> {
    let mut m = Fat16::with_oem(&s.oem)?;
    m.put(agent::MARKER, &s.marker)?;
    m.put(agent::AGENT_NAME, agent_cmd.as_bytes())?;
    m.put(agent::SETUP_NAME, agent::SETUP_CMD.as_bytes())?;
    m.put(agent::CMD, agent::command_file(&request.command).as_bytes())?;
    m.put(agent::GO, request.token.as_bytes())?;
    Ok(m)
}

/// Read the mailbox of a finished run. `Ok(None)`: the guest did not
/// finish. A missing [`agent::CODE`] is not a zero.
///
/// A result with another token is refused, not reported: it comes from an
/// old volume or from a second writer.
pub fn outcome(m: &Fat16, token: &str) -> Result<Option<Outcome>> {
    let Some(code) = m.get(agent::CODE) else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&code).to_string();
    let parsed = agent::parse_code(&text)
        .ok_or_else(|| failure(Class::Protocol, format!("{text:?} is not an exit code and a token")))?;
    if parsed.token != token {
        return Err(failure(
            Class::Protocol,
            format!("the mailbox holds a result for token {:?}, not {token:?}", parsed.token),
        ));
    }
    Ok(Some(Outcome {
        code: parsed.code,
        token: parsed.token,
        stdout: m.get(agent::OUT).unwrap_or_default(),
        stderr: m.get(agent::ERR).unwrap_or_default(),
    }))
}

/// Stage one run of the UEFI flavour: the mailbox on disk and the overlay
/// over the read-only base image. Returns the mailbox in memory; [`run`]
/// reads the result from the file that the guest wrote.
///
/// The caller makes the per-run copy of the variable store:
/// [`Plan::firmware_vars`] names it before this call.
pub fn stage(
    s: &Settings,
    qemu_img: &Path,
    base: &Path,
    plan: &Plan,
    agent_cmd: &str,
    request: &Request,
) -> Result<Fat16> {
    let m = mailbox(s, agent_cmd, request)?;
    std::fs::write(&plan.mailbox, m.image()).map_err(|e| io_at(&plan.mailbox, e))?;
    // `qemu-img create` refuses a file that exists, and a per-run directory
    // of a killed run can hold the overlay.
    let _ = std::fs::remove_file(&plan.root);
    // The emulator refuses to bind a socket path that exists.
    let _ = std::fs::remove_file(&plan.monitor);
    emulator::run_steps(&overlay_argv(qemu_img, base, &plan.root))?;
    Ok(m)
}

/// Boot the guest of the UEFI flavour, wait until it powers off, and read
/// the result.
///
/// The agent powers the machine off only after it wrote the exit code. A
/// timeout is a command that still runs or a guest that did not reach the
/// mailbox: both are failures ([`Timeout`]).
pub fn run(s: &Settings, plan: &Plan, request: &Request) -> Result<Outcome> {
    // No pipe that nobody reads: an emulator that writes more than one pipe
    // buffer blocks. The error stream goes to a file in the per-run
    // directory, which `cleanup` removes.
    let log = emulator::emulator_log(s, plan);
    let stderr = match &log {
        Some(path) => std::fs::File::create(path).map(Stdio::from).map_err(|e| io_at(path, e))?,
        None => Stdio::null(),
    };
    let mut child = emulator::spawn_uefi(plan, stderr)?;
    let start = Instant::now();
    let exited = loop {
        match child.try_wait().map_err(|e| io_at(&plan.emulator, e))? {
            Some(_) => break true,
            None if start.elapsed() >= request.timeout => break false,
            None => std::thread::sleep(Duration::from_millis(500)),
        }
    };
    if !exited {
        let stop = emulator::stop_emulator(plan, &mut child);
        return Err(Timeout { after: request.timeout, stop }.failure());
    }
    // The exit status of the emulator is not the answer: the mailbox is.
    // Read the volume again from disk, as the guest wrote the result there.
    let bytes = std::fs::read(&plan.mailbox).map_err(|e| io_at(&plan.mailbox, e))?;
    let back = Fat16::from_image(bytes)
        .ok_or_else(|| failure(Class::Protocol, format!("{}: not a mailbox", plan.mailbox.display())))?;
    outcome(&back, &request.token)?.ok_or_else(|| {
        let tail = log.as_deref().map(emulator::log_tail).unwrap_or_default();
        failure(Class::Protocol, format!("the guest powered off with no result{tail}"))
    })
}

/// Ask a running guest to stop, through QMP. A guest that powered off has
/// no monitor: that is the success case, not a failure.
pub fn stop(plan: &Plan) {
    if let Ok(mut m) = qmp::Monitor::connect(&plan.monitor, Duration::from_secs(2)) {
        m.quit();
    }
}

#[cfg(test)]
mod tests;
