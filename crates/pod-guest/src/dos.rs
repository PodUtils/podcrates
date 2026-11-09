//! The DOS flavour: FreeDOS on the machine profile of the host, with no KVM
//! and no licensed image.
//!
//! The mailbox, the token, the exit-code line, the QMP monitor, the per-run
//! directory and the overlay are the same for each guest. Two things
//! differ: what the emulator boots, and how the command gets in. The UEFI
//! flavour starts a scheduled task and powers itself off, so a run needs no
//! console. FreeDOS has no scheduler, so the crate types one line into its
//! console through QMP. This module has the machine line, the boot keys and
//! the typed line, and nothing else.
//!
//! The guest is DOS: FreeDOS 1.4 (FreeCom 0.86) runs DOS batch, which is
//! portable and free to give to others. A caller that needs the Windows
//! kernel uses the UEFI flavour with a disk image.

use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use pod_core::{Class, Failure};

use crate::qmp::{self, Monitor};
use crate::{
    Accel, Fat16, Outcome, Plan, Result, Settings, Timeout, agent, context, emulator, failure, io_at, outcome,
};

/// The guest memory of DOS in MiB. DOS needs little; a value of the caller
/// wins.
pub const DEFAULT_MEM_MIB: u64 = 512;
/// The guest CPUs of DOS. [`argv`] gives this count.
pub const CPUS: u32 = 2;

/// The limit of one DOS run in seconds. The boot keys alone take 42 s on
/// the measured host, the typing takes seconds, and `ver` answers at once;
/// the rest is for a slow host. A command that never answers (`pause`)
/// reaches the limit and gives no status.
pub const DEFAULT_TIMEOUT_SECS: u64 = 180;

/// The boot keys of the FreeDOS 1.4 LiteUSB installer, in milliseconds,
/// measured on an AMD Ryzen 7 7700 with qemu 10.2.3 under TCG: 18 s to the
/// language menu, 8 s to the proceed menu, 16 s to `C:\>`. A slower host
/// needs longer values (TCG costs 3 to 21 times the time of KVM), and a
/// base with no installer menus needs no keys.
pub const TO_MENU_MS: u64 = 18_000;
pub const TO_PROCEED_MS: u64 = 8_000;
pub const TO_PROMPT_MS: u64 = 16_000;

/// The drive letter of the mailbox. The machine line attaches two IDE
/// disks (the overlay first, the mailbox second), and FreeDOS gives them C:
/// and D:. With a wrong letter the result line never comes, and the run
/// gives no status: a failure, not a wrong result.
pub const MBOX_DRIVE: &str = "D:";
/// The batch file with the extension that DOS runs. FreeDOS `command.com`
/// does `call` on `.BAT` and does nothing on `.CMD`, so both files hold the
/// same bytes.
pub const BAT: &str = "WQCMD.BAT";

/// The argv of the emulator for a DOS guest, with no program: SeaBIOS on
/// `pc`, IDE disks, VGA text. This path needs no OVMF files. The
/// accelerator and the QMP socket are as in the UEFI flavour.
#[allow(clippy::too_many_arguments)]
pub fn argv(
    accel: Accel,
    overlay: &Path,
    mailbox: &Path,
    serial: &Path,
    monitor: &Path,
    mem_mib: u64,
    emu_args: &[String],
) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    let mut push = |s: &str| a.push(s.to_string());
    push("-M");
    push("pc");
    push("-accel");
    push(accel.word());
    push("-cpu");
    push(accel.cpu());
    push("-smp");
    push(&CPUS.to_string());
    push("-m");
    push(&mem_mib.to_string());
    push("-drive");
    push(&format!("file={},format=qcow2,if=ide", overlay.display()));
    push("-drive");
    push(&format!("file={},format=raw,if=ide,cache=writethrough", mailbox.display()));
    push("-display");
    push("none");
    push("-vga");
    push("std");
    push("-rtc");
    push("base=localtime");
    push("-no-reboot");
    push("-nic");
    push("none");
    push("-serial");
    push(&format!("file:{}", serial.display()));
    push("-qmp");
    push(&format!("unix:{},server=on,wait=off", monitor.display()));
    a.extend(emu_args.iter().cloned());
    a
}

/// Stage one DOS run: the overlay over the read-only base image, and the
/// mailbox with the marker of `s`, the batch and the token.
///
/// The mailbox is the same volume as in the UEFI flavour, so one reader
/// serves both: the host polls for the code.
pub fn stage(s: &Settings, qemu_img: &Path, base: &Path, plan: &Plan, batch: &str, token: &str) -> Result<()> {
    let mut m = Fat16::with_oem(&s.oem)?;
    m.put(agent::MARKER, &s.marker)?;
    let mut body = Vec::from(&b"@echo off\r\n"[..]);
    body.extend_from_slice(batch.as_bytes());
    body.extend_from_slice(b"\r\n");
    m.put(agent::CMD, &body)?;
    m.put(BAT, &body)?;
    m.put(agent::GO, token.as_bytes())?;
    std::fs::write(&plan.mailbox, m.image()).map_err(|e| io_at(&plan.mailbox, e))?;
    let _ = std::fs::remove_file(&plan.root);
    let _ = std::fs::remove_file(&plan.monitor);
    emulator::run_steps(&crate::overlay_argv(qemu_img, base, &plan.root))?;
    Ok(())
}

/// Type one line into the console of the guest and press return. A
/// character with no key refuses the whole line before the first key: a
/// part of a line is another command.
fn type_line(m: &mut Monitor, line: &str) -> Result<()> {
    let mut keys = Vec::with_capacity(line.len());
    for ch in line.chars() {
        keys.push(qmp::key(ch).ok_or_else(|| failure(Class::Usage, format!("no key for {ch:?} in {line:?}")))?);
    }
    for (name, shifted) in keys {
        if shifted {
            m.keys(&["shift", name])?;
        } else {
            m.keys(&[name])?;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    m.keys(&["ret"])?;
    std::thread::sleep(Duration::from_millis(1200));
    Ok(())
}

/// The boot keys of the LiteUSB installer: return for English, then down
/// and return for No (back to DOS). The waits are [`TO_MENU_MS`],
/// [`TO_PROCEED_MS`] and [`TO_PROMPT_MS`].
fn boot_keys(m: &mut Monitor) -> Result<()> {
    std::thread::sleep(Duration::from_millis(TO_MENU_MS));
    m.keys(&["ret"])?;
    std::thread::sleep(Duration::from_millis(TO_PROCEED_MS));
    m.keys(&["down"])?;
    std::thread::sleep(Duration::from_millis(500));
    m.keys(&["ret"])?;
    std::thread::sleep(Duration::from_millis(TO_PROMPT_MS));
    Ok(())
}

/// Run one DOS command: the boot, the boot keys, two typed lines, a poll
/// for the result with the token, the stop. Returns the outcome of the
/// guest; at `timeout` a [`Timeout`] failure.
///
/// The first line runs the batch through `command /c`, not with `call` on
/// the mailbox file: `call` with a redirection outside it wrote 0 bytes
/// where the same typed line wrote 68. The mailbox files stay, so that the
/// guest can `type` them.
pub fn run(plan: &Plan, batch: &str, token: &str, timeout: Duration) -> Result<Outcome> {
    // The DOS machine line goes to the shared spawn, which puts the
    // emulator in a process group of its own.
    let args =
        argv(plan.accel, &plan.root, &plan.mailbox, &plan.serial, &plan.monitor, plan.memory_mib, &plan.emu_args);
    let mut child: Child = emulator::spawn_emulator(plan, &args, Stdio::null())?;
    let failed = |child: &mut Child, before: &str, e: Failure| -> Failure {
        let _ = emulator::stop_emulator(plan, child);
        context(before, e)
    };
    let mut mon = match Monitor::connect(&plan.monitor, Duration::from_secs(10)) {
        Ok(m) => m,
        Err(e) => return Err(failed(&mut child, "qmp connect", e)),
    };
    if let Err(e) = boot_keys(&mut mon) {
        return Err(failed(&mut child, "boot keys", e));
    }
    let wrap1 = format!("command /c {batch} > {MBOX_DRIVE}\\WQOUT.TXT");
    let wrap2 = format!("echo %errorlevel% {token} > {MBOX_DRIVE}\\WQCODE.TXT");
    if let Err(e) = type_line(&mut mon, &wrap1).and_then(|()| type_line(&mut mon, &wrap2)) {
        return Err(failed(&mut child, "type command", e));
    }
    drop(mon);
    // Poll the mailbox file for the result line with the token. A wrong
    // token is ignored until the limit: a forgery costs time, never a wrong
    // exit code.
    let deadline = Instant::now() + timeout;
    let mut code: Option<Outcome> = None;
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        if let Ok(bytes) = std::fs::read(&plan.mailbox)
            && let Some(m) = Fat16::from_image(bytes)
            && let Ok(Some(o)) = outcome(&m, token)
        {
            code = Some(o);
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let stop = emulator::stop_emulator(plan, &mut child);
    match code {
        Some(o) => Ok(o),
        None => Err(Timeout { after: timeout, stop }.failure()),
    }
}

/// The batch line for a host argv. `cmd /c ver` runs `ver` in DOS, so the
/// prefix drops; an empty command is `ver`. The line runs in a subshell of
/// the guest, so an `exit` at its end does not end the shell that reports.
pub fn batch_from_argv(argv: &[String]) -> String {
    agent::command_from_argv(argv)
}

#[cfg(test)]
mod tests;
