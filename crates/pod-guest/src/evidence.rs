//! The evidence of a run that timed out: what the mailbox tells of the
//! agent, and, in the directory of [`Settings::evidence`], a screen of the
//! guest, its serial log, the error stream of the emulator and the
//! mailbox.
//!
//! The screen comes before the stop: a stopped emulator has no screen. The
//! files come after the stop: the guest writes the mailbox and the serial
//! log until then. A step that fails is named in the text, and the other
//! steps go on: the failure of the run is the timeout, not its evidence.
//!
//! The mailbox shows what reached the disk. Windows writes a FAT volume
//! through its cache, so the last step of the agent can be missing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{Fat16, Plan, Settings, agent, qmp};

/// The name of the screen in the directory of a run.
pub const SCREEN: &str = "screen.ppm";
/// The name of the copy of the serial log.
pub const SERIAL: &str = "serial.log";
/// The name of the copy of the error stream of the emulator.
pub const EMULATOR: &str = "emulator.log";
/// The name of the copy of the mailbox.
pub const MAILBOX: &str = "mailbox.img";

/// How long the screen waits for the QMP socket. The emulator runs, so the
/// socket is there; a guest that hangs QEMU must not hang the run too.
const SCREEN_WAIT: Duration = Duration::from_secs(5);
/// The count of lines of [`agent::LOG`] in the text of a failure.
const LOG_LINES: usize = 8;

/// The directory of the evidence of the run of `token` in `root`:
/// `root/timeout-TOKEN`, with each character of the token that is not a
/// letter, a digit, `-` or `_` as `_`.
pub fn dir(root: &Path, token: &str) -> PathBuf {
    let safe: String = token
        .chars()
        .take(64)
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    root.join(format!("timeout-{}", if safe.is_empty() { "run" } else { &safe }))
}

/// Take the screen of the guest into the evidence directory of `token`,
/// while the emulator runs. `None` when [`Settings::evidence`] is not set;
/// else the file, or the reason that there is none.
pub(crate) fn screen(s: &Settings, plan: &Plan, token: &str) -> Option<Result<PathBuf, String>> {
    let root = s.evidence.as_deref()?;
    let dir = dir(root, token);
    let shot = || -> crate::Result<PathBuf> {
        std::fs::create_dir_all(&dir).map_err(|e| crate::io_at(&dir, e))?;
        let file = dir.join(SCREEN);
        // The connection closes before the stop: QEMU serves one QMP
        // client at a time, and the stop sends `quit` on a new one.
        qmp::Monitor::connect(&plan.monitor, SCREEN_WAIT)?.screendump(&file)?;
        Ok(file)
    };
    Some(shot().map_err(|e| e.chain()))
}

/// The text that a run that timed out adds to its failure, after the stop:
/// what the mailbox tells ([`seen`]), and, with [`Settings::evidence`], the
/// directory of the evidence with each file in it and each step that
/// failed. `log` is the error stream of the emulator, when the run kept
/// one.
pub(crate) fn after_stop(
    s: &Settings,
    plan: &Plan,
    token: &str,
    log: Option<&Path>,
    shot: Option<Result<PathBuf, String>>,
) -> String {
    let mailbox = std::fs::read(&plan.mailbox).ok().and_then(Fat16::from_image);
    let mut text = match &mailbox {
        Some(m) => format!("; {}", seen(m, token)),
        None => format!("; {} does not read as a mailbox", plan.mailbox.display()),
    };
    let Some(root) = s.evidence.as_deref() else {
        return text;
    };
    let dir = dir(root, token);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        text.push_str(&format!("; no evidence: {}: {e}", dir.display()));
        return text;
    }
    let (mut kept, mut lost) = (Vec::new(), Vec::new());
    match shot {
        Some(Ok(_)) => kept.push(SCREEN.to_string()),
        Some(Err(why)) => lost.push(format!("no screen: {why}")),
        None => lost.push("no screen".to_string()),
    }
    let copies = [(Some(plan.serial.as_path()), SERIAL), (log, EMULATOR), (Some(plan.mailbox.as_path()), MAILBOX)];
    for (from, name) in copies {
        let Some(from) = from else { continue };
        match std::fs::copy(from, dir.join(name)) {
            Ok(_) => kept.push(name.to_string()),
            Err(e) => lost.push(format!("no {name}: {}: {e}", from.display())),
        }
    }
    text.push_str(&format!("; the evidence is in {}: {}", dir.display(), kept.join(", ")));
    for l in lost {
        text.push_str(&format!("; {l}"));
    }
    text
}

/// What the mailbox `m` tells of the agent of the run of `token`: it did
/// not find the mailbox, it found it and wrote no result, or it wrote the
/// result and the guest did not power off; then the last lines of its log
/// ([`agent::LOG`]).
pub fn seen(m: &Fat16, token: &str) -> String {
    let code = m.get(agent::CODE).and_then(|c| agent::parse_code(&String::from_utf8_lossy(&c)));
    let mut text = match code {
        Some(c) if c.token == token => {
            format!("the agent wrote the result (code {}) and the guest did not power off", c.code)
        }
        Some(c) => format!("the mailbox holds a result for another token ({:?})", c.token),
        None if agent::ready(m) => format!("the agent found the mailbox and wrote no result for {token:?}"),
        None => format!("the agent did not write {}: it did not run, or it did not find the mailbox", agent::READY),
    };
    let log = m.get(agent::LOG).unwrap_or_default();
    let lines: Vec<String> = String::from_utf8_lossy(&log)
        .lines()
        .map(|l| l.chars().filter(|c| !c.is_control()).take(120).collect::<String>())
        .filter(|l| !l.trim().is_empty())
        .collect();
    if !lines.is_empty() {
        let start = lines.len().saturating_sub(LOG_LINES);
        text.push_str(&format!("; the log of the agent: {}", lines[start..].join(" | ")));
    }
    text
}

#[cfg(test)]
mod tests;
