//! The warm start of a guest of the UEFI flavour: a booted guest in
//! [`agent::Mode::KeepAlive`] is frozen to a directory once, and each later run
//! restores it in place of a boot.
//!
//! * [`save`]: QMP `stop`, `migrate` to `file:DIR/state.bin`, a copy of the
//!   overlay, the mailbox and the variable store, the [`Fingerprint`] last;
//!   then `cont`, so that the guest goes on.
//! * [`restore`]: a thin overlay over the saved one, the saved mailbox as
//!   it is, the saved variable store, and the emulator with
//!   `-incoming file:DIR/state.bin`; then `cont`, and the next command on
//!   a new medium of the mailbox ([`crate::swap::Swap::next`]).
//! * [`start`]: a restore, or a cold boot ([`crate::stage`]) when the state
//!   is missing, has another fingerprint, or does not load ([`Cold`]).
//!
//! A wrong or stale state boots cold. It never gives the answer of another
//! guest: the fingerprint holds each input that the state depends on.
//!
//! The steps that know no [`Plan`] are in [`mod@line`]: a guest of another
//! machine line (a direct kernel boot) saves and loads with them, and
//! [`save`] and [`restore`] call them.
//!
//! The mailbox must be a USB stick ([`crate::plan::MailboxBus::Usb`]): the
//! restored RAM holds the cache of the saved mailbox, and Windows reads a
//! mounted volume again only after a change of its medium. The emulator
//! must be able to migrate each device of the plan: QEMU before 11 refuses
//! the NVMe root disk of [`crate::argv`], and [`save`] then fails with
//! [`Class::Unavailable`] and the reasons of the emulator.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use sha2::{Digest, Sha256};

use crate::swap::Swap;
use crate::{Fat16, Plan, Request, Result, Settings, agent, emulator, failure, io_at, overlay_argv};

pub mod line;
mod state;

pub use state::{idle, incoming_argv, restore, save};

/// The migration stream in a saved directory.
pub const STATE: &str = "state.bin";
/// The overlay at the time of the save, over the base of the plan.
pub const ROOT: &str = "root.qcow2";
/// The mailbox at the time of the save.
pub const MAILBOX: &str = "mailbox.img";
/// The variable store of the firmware at the time of the save.
pub const VARS: &str = "vars.fd";
/// The text of the [`Fingerprint`]; [`save`] writes it last, so that a
/// directory with it holds each other file.
pub const FINGERPRINT: &str = "fingerprint.txt";
/// The first line of a fingerprint: the layout of a saved directory.
pub const FORMAT: &str = "pod-guest warm 1";

/// Each input that a saved state depends on. Two runs with the same text
/// ([`Fingerprint::text`]) can share a state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// The first line of `EMULATOR --version`: the device models and the
    /// format of the stream.
    pub qemu: String,
    /// `tcg`, or under KVM `host`, the sha256 of the CPU of the host and
    /// the release of its kernel ([`line::cpu`]): `-cpu host` gives the
    /// guest the features of the host.
    pub cpu: String,
    /// The sha256 of the firmware code.
    pub firmware: String,
    /// The digest of the base image, as the caller pinned it.
    pub base: String,
    /// The sha256 of the agent of the image.
    pub agent: String,
    /// The machine line of the plan ([`machine_line`]).
    pub machine: Vec<String>,
}

impl Fingerprint {
    /// The inputs in the form of a machine line of any guest: `qemu`,
    /// `cpu`, `firmware`, `base`, `agent`, then one `machine` for each
    /// argument.
    pub fn inputs(&self) -> line::Inputs {
        line::Inputs::new()
            .with("qemu", self.qemu.as_str())
            .with("cpu", self.cpu.as_str())
            .with("firmware", self.firmware.as_str())
            .with("base", self.base.as_str())
            .with("agent", self.agent.as_str())
            .machine(&self.machine)
    }

    /// The text of the fingerprint ([`line::Inputs::text`]): [`FORMAT`],
    /// then one line for each input, each value as a JSON string.
    pub fn text(&self) -> String {
        self.inputs().text()
    }
}

/// The fingerprint of `plan` over the base image with the pinned digest
/// `base_digest`, for a guest whose agent is `agent_cmd`
/// ([`agent::AGENT_CMD`]).
///
/// It starts `EMULATOR --version`, reads the firmware code, and under KVM
/// reads `/proc/cpuinfo` and the release of the kernel ([`line::cpu`]). An
/// empty `base_digest` is [`Class::Usage`]: with no digest, a new base
/// under the old name restores the old guest.
pub fn fingerprint(plan: &Plan, base_digest: &str, agent_cmd: &str) -> Result<Fingerprint> {
    if base_digest.trim().is_empty() {
        return Err(failure(Class::Usage, "the fingerprint of a saved state needs the digest of the base"));
    }
    let cpu = line::cpu(plan.accel)?;
    let code = std::fs::read(&plan.firmware_code).map_err(|e| io_at(&plan.firmware_code, e))?;
    Ok(Fingerprint {
        qemu: qemu_version(&plan.emulator)?,
        cpu,
        firmware: sha256(&code),
        base: base_digest.to_string(),
        agent: sha256(agent_cmd.as_bytes()),
        machine: machine_line(plan),
    })
}

/// The first line of `EMULATOR --version`, with the build of the package
/// (`QEMU emulator version 10.2.1 (Debian 1:10.2.1+ds-1)`).
pub fn qemu_version(emulator: &Path) -> Result<String> {
    let out = Command::new(emulator)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| io_at(emulator, e))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    if !out.status.success() || line.is_empty() {
        return Err(failure(Class::Unavailable, format!("{} --version gave no version", emulator.display())));
    }
    Ok(line.to_string())
}

/// The CPU of the host for a fingerprint: `host` and the sha256 of the
/// vendor, the family, the model, the stepping and the flags of the first
/// processor of `cpuinfo`. The microcode and the clock are not in it: they
/// change and do not change the features.
pub fn host_cpu(cpuinfo: &str) -> String {
    let keys = ["vendor_id", "cpu family", "model", "model name", "stepping", "flags"];
    let mut kept = String::new();
    for line in cpuinfo.lines() {
        if line.trim().is_empty() && !kept.is_empty() {
            break;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        if keys.contains(&key.trim()) {
            kept.push_str(&format!("{}={}\n", key.trim(), value.trim()));
        }
    }
    format!("host {}", sha256(kept.as_bytes()))
}

/// The machine line of `plan` ([`crate::argv`]) with each path of a run as
/// a fixed name: the overlay, the mailbox, the variable store, the serial
/// log, the sockets and the media change from run to run, and the state
/// does not depend on their names. A medium keeps its extension, which
/// gives its format. The arguments of the caller stay as they are.
pub fn machine_line(plan: &Plan) -> Vec<String> {
    let mut p = plan.clone();
    p.share = "SHARE".into();
    p.firmware_code = "CODE".into();
    p.firmware_vars = "VARS".into();
    p.root = "ROOT".into();
    p.mailbox = "MAILBOX".into();
    p.serial = "SERIAL".into();
    p.monitor = "QMP".into();
    let o = &mut p.options;
    if let Some(vnc) = o.vnc.as_mut() {
        *vnc = "VNC".into();
    }
    if let Some(socket) = o.serial_socket.as_mut() {
        *socket = "SERIAL_SOCKET".into();
    }
    for (i, c) in o.channels.iter_mut().enumerate() {
        c.socket = format!("CHANNEL{i}").into();
    }
    for (i, m) in o.media.iter_mut().enumerate() {
        let mut name = PathBuf::from(format!("MEDIUM{i}"));
        if let Some(ext) = m.path.extension() {
            name.set_extension(ext);
        }
        m.path = name;
    }
    crate::argv(&p)
}

/// The sha256 of `bytes` in lower-case hex.
fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A saved state for [`start`] and [`restore`].
#[derive(Debug, Clone)]
pub struct Warm {
    /// The directory that [`save`] wrote.
    pub dir: PathBuf,
    /// The fingerprint of this run; the state must have the same text.
    pub fingerprint: Fingerprint,
    /// The limit of the load of the state.
    pub wait: Duration,
}

/// Why a run did not start warm: the cause of the failure of [`restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cold {
    /// The directory holds no complete state.
    Missing,
    /// The state has another fingerprint: the first line that differs, of
    /// the state and of this run.
    Stale { saved: String, now: String },
    /// The state did not load: the emulator refused it, exited, did not
    /// end the load within the limit, or refused the medium of the command.
    Failed { why: String },
}

impl std::fmt::Display for Cold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Cold::Missing => write!(f, "no saved state"),
            Cold::Stale { saved, now } => write!(f, "the saved state is stale: it has {saved:?}, this run has {now:?}"),
            Cold::Failed { why } => write!(f, "the saved state did not load: {why}"),
        }
    }
}

impl std::error::Error for Cold {}

impl Cold {
    /// [`Class::NotFound`] for no state, [`Class::Config`] for another
    /// fingerprint, [`Class::Unavailable`] for a load that failed.
    pub fn failure(self) -> Failure {
        let class = match self {
            Cold::Missing => Class::NotFound,
            Cold::Stale { .. } => Class::Config,
            Cold::Failed { .. } => Class::Unavailable,
        };
        Failure::caused(Stage::Host, class, self.to_string(), self)
    }
}

/// The [`Cold`] in the chain of `f`.
pub fn cold(f: &Failure) -> Option<&Cold> {
    crate::cause_of::<Cold>(f)
}

/// How [`start`] started the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// From the saved state: the command is on a new medium, swapped in
    /// after the load.
    Warm,
    /// With a boot, for this reason: the command is on the mailbox before
    /// the boot, as [`crate::stage`] puts it.
    Cold(Cold),
}

/// A guest that [`start`] started.
#[derive(Debug)]
pub struct Started {
    /// The emulator that runs.
    pub child: Child,
    /// Warm, or cold and why.
    pub how: Start,
    /// The media of the mailbox, with the command of the request on the
    /// medium in the guest ([`Swap::current`]): the file to poll, and the
    /// way to each next command.
    pub swap: Swap,
}

/// Start the guest of `plan` for `request`: warm from `warm` when it
/// restores, else with a boot over `base`. Each path ends with a running
/// emulator and the command of `request` on the medium in the guest
/// ([`Started::swap`]); the caller polls it ([`Swap::poll`]), gives the
/// next command ([`Swap::next`]), or waits for the power-off of a
/// [`agent::Mode::Once`] request, and stops the emulator.
///
/// A plan whose mailbox is not [`crate::plan::MailboxBus::Usb`] is
/// [`Class::Usage`]. A failure of the cold boot is a failure of the start;
/// a failure of the restore is only the reason of a cold boot. The error
/// stream of each emulator goes to `emulator.log` in the per-run
/// directory.
pub fn start(
    s: &Settings,
    qemu_img: &Path,
    base: &Path,
    plan: &Plan,
    agent_cmd: &str,
    request: &Request,
    warm: &Warm,
) -> Result<Started> {
    let why = match restore(s, qemu_img, plan, agent_cmd, request, warm) {
        Ok((child, swap)) => return Ok(Started { child, how: Start::Warm, swap }),
        Err(e) => match cold(&e) {
            Some(why) => why.clone(),
            None => return Err(e),
        },
    };
    crate::stage(s, qemu_img, base, plan, agent_cmd, request)?;
    let swap = Swap::new(plan, &request.token)?;
    let child = emulator::spawn_uefi(plan, log_stream(s, plan)?)?;
    Ok(Started { child, how: Start::Cold(why), swap })
}

/// Stage a guest to save: the overlay over `base`, and a mailbox with the
/// agent and [`agent::KEEP`] but no token. The agent writes
/// [`agent::READY`] and then waits for a token, which is the state that
/// [`save`] keeps. The caller makes the copy of the variable store first,
/// as for [`crate::stage`].
pub fn stage_idle(s: &Settings, qemu_img: &Path, base: &Path, plan: &Plan, agent_cmd: &str) -> Result<Fat16> {
    let mut m = Fat16::with_oem(&s.oem)?;
    m.put(agent::MARKER, &s.marker)?;
    m.put(agent::AGENT_NAME, agent_cmd.as_bytes())?;
    m.put(agent::SETUP_NAME, agent::SETUP_CMD.as_bytes())?;
    m.put(agent::KEEP, b"")?;
    std::fs::write(&plan.mailbox, m.image()).map_err(|e| io_at(&plan.mailbox, e))?;
    let _ = std::fs::remove_file(&plan.root);
    let _ = std::fs::remove_file(&plan.monitor);
    emulator::run_steps(&overlay_argv(qemu_img, base, &plan.root))?;
    Ok(m)
}

/// The error stream of an emulator: `emulator.log` in the per-run
/// directory, or nothing when the plan has none.
fn log_stream(s: &Settings, plan: &Plan) -> Result<Stdio> {
    match emulator::emulator_log(s, plan) {
        Some(path) => std::fs::File::create(&path).map(Stdio::from).map_err(|e| io_at(&path, e)),
        None => Ok(Stdio::null()),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod live;
