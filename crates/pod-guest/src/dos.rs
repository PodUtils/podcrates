//! The DOS guest: FreeDOS under the machine tier's own profile, no KVM and
//! no licensed image. `TODO/milestones.md` T-1112.
//!
//! ⛔ **Why DOS is a flavor and not a second driver.** The mailbox, the
//! token, the exit-code line, the QMP monitor, the per-run directory and
//! the overlay discipline are the same mechanism whatever the guest is.
//! What differs is only what the emulator boots and how the command gets
//! in: the other flavor autostarts a scheduled task and powers itself off,
//! so a run needs no console at all; FreeDOS has no scheduler, so the
//! driver types one wrapper line into its console through QMP. This file
//! owns the three things that differ: the machine line, the boot keys
//! and the typed line, and nothing else.
//!
//! ⚠ **The guest is DOS, and it says so on every run.** FreeDOS 1.4
//! (FreeCom 0.86) runs DOS batch, which is portable and redistributable,
//! not Windows 11. A caller who needs the Windows kernel names a disk
//! image and takes the other flavor.

use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use super::plan::Plan;
use super::qmp::Monitor;
use super::{agent, outcome, Accel, Error, Outcome};

/// The DOS base image: `PODVM_DOS_BASE` where it is set, so an experiment
/// isolates with its own copy, else under the home directory beside the
/// other flavor's cache so both persist between runs the same way.
///
/// ⛔ **Not `PODVM_WINDOWS_BASE`.** That name is the other flavor's cache,
/// and one variable naming two different disk images is the variant-keyed
/// cache defect `docs/conventions/forbidden-patterns.md` refuses: a DOS
/// run would boot a Windows disk and report its output as DOS's.
pub fn base_cache() -> PathBuf {
    if let Some(p) = std::env::var_os("PODVM_DOS_BASE")
        && !p.is_empty()
    {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into());
    Path::new(&home).join(".local/share/podvm/windows/freedos-base.img")
}

/// Guest memory for DOS in MiB. DOS needs little; a configured value wins.
pub const DEFAULT_MEM_MIB: u64 = 512;
/// Guest CPUs for DOS.
pub const CPUS: u32 = 2;

/// How long one DOS run may take. The boot walk alone is 42 s on the
/// calibrated host, typing takes seconds, and `ver` answers at once, so
/// this is margin for a slow host rather than luck. A command that never
/// answers (`pause`) reaches it and is reported with no status.
pub const DEFAULT_TIMEOUT_SECS: u64 = 180;

/// The boot walk, in milliseconds. Calibrated on AMD Ryzen 7 7700 with
/// qemu 10.2.3 under TCG: 18 s to the language menu, 8 s to the proceed
/// menu, 16 s to `C:\>`. A slower host needs longer bounds (T-1308 prices
/// TCG at 3x to 21x), and a pre-installed base with no installer menus
/// removes the keys entirely. Named as the limit it is.
pub const TO_MENU_MS: u64 = 18_000;
pub const TO_PROCEED_MS: u64 = 8_000;
pub const TO_PROMPT_MS: u64 = 16_000;

/// The mailbox drive letter. The machine line below attaches exactly two
/// IDE disks (the overlay first, the mailbox second), and FreeDOS numbers
/// them C: then D:. Where the letter is wrong the completion line never
/// arrives and the run reports no status, which is a failure rather than a
/// wrong result.
pub const MBOX_DRIVE: &str = "D:";
/// The batch under the extension DOS runs. FreeDOS `command.com` answers
/// `call` on `.BAT` and stays silent on `.CMD`, so both files carry the
/// same bytes and the guest calls this one.
pub const BAT: &str = "WQCMD.BAT";

/// The emulator's argv for a DOS guest: SeaBIOS on `pc`, IDE disks, VGA
/// text. No OVMF files are needed on this path, so a host without them
/// still runs. The accelerator and the QMP socket match the other flavor.
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
    push(&format!(
        "file={},format=raw,if=ide,cache=writethrough",
        mailbox.display()
    ));
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

/// Stage one DOS run: the disposable overlay over the read-only base image
/// and the mailbox carrying the marker, the batch and the token.
///
/// The batch goes in first and the token arms it in the same volume the
/// other flavor uses, so one reader serves both: the guest polls for the
/// token and the host polls for the code.
pub fn stage(
    qemu_img: &Path,
    base: &Path,
    plan: &Plan,
    batch: &str,
    token: &str,
) -> Result<(), Error> {
    let mut m = super::Fat16::new();
    m.put(super::agent::MARKER, b"podvm-dos")
        .expect("8.3 marker");
    let mut body = Vec::from(&b"@echo off\r\n"[..]);
    body.extend_from_slice(batch.as_bytes());
    body.extend_from_slice(b"\r\n");
    m.put(super::agent::CMD, &body).expect("8.3 command");
    m.put(BAT, &body).expect("8.3 batch");
    m.put(super::agent::GO, token.as_bytes())
        .expect("8.3 token");
    std::fs::write(&plan.mailbox, m.image())
        .map_err(|e| Error::Io(format!("{}: {e}", plan.mailbox.display())))?;
    let _ = std::fs::remove_file(&plan.root);
    let _ = std::fs::remove_file(&plan.monitor);
    super::run_steps(&super::overlay_argv(qemu_img, base, &plan.root))?;
    Ok(())
}

/// Type one line into the guest console and press return. Every character
/// maps through the crate's key table; an unmapped character refuses the
/// whole line rather than typing a prefix of it, because a prefix is a
/// different command.
fn type_line(m: &mut Monitor, line: &str) -> Result<(), String> {
    for ch in line.chars() {
        match super::keys(ch) {
            Some((name, false)) => m.keys(&[name])?,
            Some((name, true)) => m.keys(&["shift", name])?,
            None => return Err(format!("no key for {ch:?} in {line:?}")),
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    m.keys(&["ret"])?;
    std::thread::sleep(Duration::from_millis(1200));
    Ok(())
}

/// Walk the LiteUSB installer to the DOS prompt: ret for English, down
/// plus ret for No (return to DOS). The sleeps are [`TO_MENU_MS`] and
/// friends, calibrated, not guessed.
fn boot_keys(m: &mut Monitor) -> Result<(), String> {
    std::thread::sleep(Duration::from_millis(TO_MENU_MS));
    m.keys(&["ret"])?;
    std::thread::sleep(Duration::from_millis(TO_PROCEED_MS));
    m.keys(&["down"])?;
    std::thread::sleep(Duration::from_millis(500));
    m.keys(&["ret"])?;
    std::thread::sleep(Duration::from_millis(TO_PROMPT_MS));
    Ok(())
}

/// Run one DOS command: boot, one typed wrapper line, poll for the token,
/// quit. Returns the guest's outcome.
///
/// The wrapper runs the batch inline through `command /c` rather than
/// `call`ing the mailbox file: `call` with an outer redirect wrote 0
/// bytes where the same line typed inline wrote 68, measured on the lane.
/// The mailbox files stay as provenance: the guest can `type` them.
pub fn run(plan: &Plan, batch: &str, token: &str, timeout: Duration) -> Result<Outcome, Error> {
    // ⛔ **The DOS machine line is its own**, SeaBIOS on `pc` with IDE disks,
    // and it is passed to the shared spawn rather than dropped: the two
    // flavors have different disks and the same kill discipline.
    let args = argv(
        plan.accel,
        &plan.root,
        &plan.mailbox,
        &plan.serial,
        &plan.monitor,
        plan.memory_mib,
        &plan.emu_args,
    );
    // ⛔ The shared spawn is what puts the emulator in a process group of its
    // own, so a stop that killed the direct child left a helper running, and a
    // helper that holds the disk image open is what the next run meets.
    let mut child: Child = crate::spawn_emulator(plan, &args, Stdio::null())?;
    let failed = |child: &mut Child, e: String| -> Error {
        let _ = crate::stop_emulator(plan, child);
        Error::Io(e)
    };
    let mut mon = match Monitor::connect(&plan.monitor, Duration::from_secs(10)) {
        Ok(m) => m,
        Err(e) => return Err(failed(&mut child, format!("qmp connect: {e}"))),
    };
    if let Err(e) = boot_keys(&mut mon) {
        return Err(failed(&mut child, format!("boot keys: {e}")));
    }
    let wrap1 = format!("command /c {batch} > {MBOX_DRIVE}\\WQOUT.TXT");
    let wrap2 = format!("echo %errorlevel% {token} > {MBOX_DRIVE}\\WQCODE.TXT");
    if let Err(e) = type_line(&mut mon, &wrap1).and_then(|()| type_line(&mut mon, &wrap2)) {
        return Err(failed(&mut child, format!("type command: {e}")));
    }
    drop(mon);
    // Poll the mailbox file for the completion line with the exact token.
    // A wrong token is ignored until the deadline: a forgery costs time,
    // never a wrong exit code.
    let deadline = Instant::now() + timeout;
    let mut code: Option<Outcome> = None;
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        if let Ok(bytes) = std::fs::read(&plan.mailbox)
            && let Some(m) = super::Fat16::from_image(bytes)
        {
            match outcome(&m, token) {
                Ok(Some(o)) => {
                    code = Some(o);
                    break;
                }
                Ok(None) => {}
                Err(_) => {}
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let stop = crate::stop_emulator(plan, &mut child);
    match code {
        Some(o) => Ok(o),
        None => Err(Error::Timeout(timeout, stop)),
    }
}

/// The batch line for a host argv. `cmd /c ver` runs `ver` in DOS, so the
/// prefix drops; an empty command is `ver`, the acceptance command. The
/// line runs in a subshell on the guest, so a trailing `exit` cannot kill
/// the reporting shell.
pub fn batch_from_argv(argv: &[String]) -> String {
    agent::command_from_argv(argv)
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;

    #[test]
    fn the_dos_machine_is_seabios_ide_and_text() {
        let a = argv(
            Accel::Tcg,
            Path::new("root.qcow2"),
            Path::new("mbox.img"),
            Path::new("serial.log"),
            Path::new("mon.sock"),
            512,
            &[],
        );
        let has = |s: &str| a.iter().any(|x| x == s);
        assert_eq!(
            a.first().map(String::as_str),
            Some("-M"),
            "argv carries flags only, never the program: the caller names the emulator"
        );
        assert!(has("pc") && has("tcg") && has("max"));
        assert!(a
            .iter()
            .any(|x| x.contains("if=ide") && x.contains("root.qcow2")));
        assert!(a
            .iter()
            .any(|x| x.contains("if=ide") && x.contains("cache=writethrough")));
        assert!(
            !a.iter().any(|x| x.contains("pflash")),
            "no OVMF on the DOS path"
        );
        assert!(!a.iter().any(|x| x == "nvme"), "no NVMe on the DOS path");
        let nic = a.iter().position(|x| x == "-nic").expect("names -nic");
        assert_eq!(a[nic + 1], "none");
        assert!(has("-no-reboot"));
    }

    #[test]
    fn every_wrapper_character_has_a_key() {
        for ch in format!("command /c ver > {MBOX_DRIVE}\\WQOUT.TXT").chars() {
            assert!(crate::keys(ch).is_some(), "no key for {ch:?}");
        }
        for ch in "echo %errorlevel% 0123456789abcdef > D:\\WQCODE.TXT".chars() {
            assert!(crate::keys(ch).is_some(), "no key for {ch:?}");
        }
    }

    #[test]
    fn the_dos_cache_is_not_the_windows_cache() {
        // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
        unsafe { std::env::set_var("PODVM_DOS_BASE", "/tmp/dos-base.img") };
        // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
        unsafe { std::env::set_var("PODVM_WINDOWS_BASE", "/tmp/win-base.vhdx") };
        assert_eq!(base_cache(), PathBuf::from("/tmp/dos-base.img"));
        assert_eq!(crate::base_cache(), PathBuf::from("/tmp/win-base.vhdx"));
        assert_ne!(base_cache(), crate::base_cache());
        // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
        unsafe { std::env::remove_var("PODVM_DOS_BASE") };
        // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
        unsafe { std::env::remove_var("PODVM_WINDOWS_BASE") };
        let p = base_cache();
        assert!(p.to_string_lossy().ends_with("freedos-base.img"), "{p:?}");
    }

    #[test]
    fn a_cmd_slash_c_prefix_drops_and_an_empty_command_is_ver() {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(batch_from_argv(&v(&["cmd", "/c", "ver"])), "ver");
        assert_eq!(
            batch_from_argv(&v(&["cmd", "/c", "dir", "/zzz"])),
            "dir /zzz"
        );
        assert_eq!(batch_from_argv(&v(&[])), "ver");
    }

    #[test]
    fn the_dos_mailbox_carries_both_batch_spellings_and_the_token() {
        let dir = std::env::temp_dir().join(format!("pbx-dosmb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mbox = dir.join("mbox.img");
        let root = dir.join("root.qcow2");
        let plan = Plan {
            emulator: "qemu-system-x86_64".into(),
            accel: Accel::Tcg,
            share: "/usr/share/qemu".into(),
            firmware_code: "/code.fd".into(),
            firmware_vars: dir.join("vars.fd"),
            root: root.clone(),
            mailbox: mbox.clone(),
            serial: dir.join("serial.log"),
            monitor: dir.join("qmp.sock"),
            memory_mib: 512,
            cpus: 2,
            emu_args: Vec::new(),
        };
        // overlay creation needs qemu-img; where none is here the stage
        // refuses naming it rather than writing half a run
        match stage(
            Path::new("qemu-img"),
            Path::new("base.img"),
            &plan,
            "ver",
            "tok",
        ) {
            Ok(()) => {
                let bytes = std::fs::read(&mbox).unwrap();
                let back = crate::Fat16::from_image(bytes).expect("a mailbox");
                assert!(back.get(super::agent::CMD).is_some());
                assert!(back.get(BAT).is_some());
                assert_eq!(back.get(super::agent::GO).unwrap(), b"tok");
                assert!(root.is_file(), "the overlay was created");
            }
            Err(e) => {
                assert!(format!("{e}").contains("qemu-img"), "{e}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
