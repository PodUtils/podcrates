use super::*;

/// The emulator's error stream: a file in the per-run directory, or `None`
/// where the plan has no directory that [`cleanup`] would remove.
pub(super) fn emulator_log(plan: &Plan) -> Option<PathBuf> {
    let dir = scratch_of(plan)?;
    let named = dir
        .file_name()
        .map(|n| n.to_string_lossy().starts_with(SCRATCH_PREFIX))
        .unwrap_or(false);
    named.then(|| dir.join("emulator.log"))
}

/// The last lines of the emulator's error stream, for a failure message.
/// Empty when the file is absent or empty.
pub(super) fn log_tail(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_default();
    // Only the end of the file, and each line cut short: the stream is the
    // emulator's, and its size is not ours to choose.
    let text = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(4096)..]);
    let lines: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take(200).collect())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let start = lines.len().saturating_sub(5);
    format!("; the emulator said: {}", lines[start..].join(" | "))
}

/// Provision a fresh image: boot it once and type the installer into its
/// console, because the agent has to exist before it can be autostarted.
///
/// ⚠ **This is the only step that needs the console, and it happens once per
/// base image.** Everything after it is the mailbox. The driver types a
/// drive-scanning loop rather than a drive letter: Windows assigns the letter
/// and the reference image has been seen to change it between boots.
///
/// ⛔ **The base is written to nothing.** The installer runs on
/// [`Plan::root`], which is a disposable overlay over `base` created by
/// [`overlay_argv`], and the overlay is never `base`: the vendor image is
/// named only as that overlay's `-b`, which `qemu-img create` opens
/// read-only. `TODO/gate.md` T-1641.
///
/// ⛔ **And the provisioned image is a separate standalone file.** The overlay
/// is committed by [`commit_argv`] into the second path named here, with no
/// `-b`, so what comes out of this carries no pointer back at `base` and the
/// vendor file can be moved or removed afterwards. The commit happens after
/// the guest has powered itself off, because a commit reads the overlay while
/// the emulator still holds it.
pub fn provision(
    qemu_img: &Path,
    base: &Path,
    plan: &Plan,
    out: &Path,
    wait: Duration,
) -> Result<Vec<u8>, Error> {
    let mut m = Fat16::new();
    m.put(agent::MARKER, b"podvm").expect("8.3 marker");
    m.put(agent::AGENT_NAME, agent::AGENT_CMD.as_bytes())
        .expect("8.3 agent name");
    m.put(agent::SETUP_NAME, agent::SETUP_CMD.as_bytes())
        .expect("8.3 setup name");
    std::fs::write(&plan.mailbox, m.image())
        .map_err(|e| Error::Io(format!("{}: {e}", plan.mailbox.display())))?;
    let _ = std::fs::remove_file(&plan.root);
    let _ = std::fs::remove_file(&plan.monitor);
    // ⚠ `qemu-img create` refuses an existing file, and `out` is the caller's
    // product rather than scratch, so it is only removed here when this boot
    // is going to overwrite it.
    if out != plan.root {
        let _ = std::fs::remove_file(out);
    }
    run_steps(&overlay_argv(qemu_img, base, &plan.root))?;
    let mut child = spawn_windows(plan, Stdio::null())?;
    // The shell needs the image to finish booting first; typing into a boot
    // screen types into nothing.
    std::thread::sleep(wait);
    let typed = type_into_console(plan, "for %d in (D E F G H I J K L M N O P Q R S T U V W X Y Z) do @if exist %d:\\WQMARK.TXT %d:\\WA.CMD");
    typed?;
    // ⛔ The installer ends in `shutdown`, so the guest powers itself off and
    // its FAT writes are flushed before it does. Waiting for the emulator to
    // exit is what makes the read below safe; killing it here is a read of a
    // volume the guest had not finished writing.
    let start = Instant::now();
    let mut exited = false;
    while start.elapsed() < wait {
        if child
            .try_wait()
            .map_err(|e| Error::Io(e.to_string()))?
            .is_some()
        {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    // ⚠ The signal is only for an emulator that outlived its wait. A child
    // that already exited has been reaped by the `try_wait` above, and a kill
    // aimed at its pid afterwards is aimed at a pid the kernel may already
    // have reused.
    let _stop = if exited {
        Stop::Confirmed { pid: child.id() }
    } else {
        stop_emulator(plan, &mut child)
    };
    // ⛔ The commit happens here, after the emulator is gone and before
    // anything is reported as provisioned. `run_steps` turns a non-zero exit
    // into `Error::Io` carrying the program and its own stderr, so a commit
    // that failed is named rather than leaving a half-written `out` behind
    // that a later `run` would boot and call a provisioned image.
    run_steps(&commit_argv(qemu_img, &plan.root, out))?;
    // The installer writes SETUP.TXT back to the mailbox; that file is the
    // evidence that provisioning ran, and it is read after the guest stops.
    let bytes = std::fs::read(&plan.mailbox).map_err(|e| Error::Io(e.to_string()))?;
    let back = Fat16::from_image(bytes)
        .ok_or_else(|| Error::Io(format!("{}: not a mailbox", plan.mailbox.display())))?;
    back.get(agent::SETUP)
        .ok_or_else(|| Error::BadResult("the installer wrote no SETUP.TXT".into()))
}

/// Type `text` into the guest console through QMP. A shifted character is
/// the modifier and the key pressed together, which is one `send-key`.
fn type_into_console(plan: &Plan, text: &str) -> Result<(), Error> {
    let mut m =
        qmp::Monitor::connect(&plan.monitor, Duration::from_secs(120)).map_err(Error::Io)?;
    for ch in text.chars() {
        let (name, shifted) = keys(ch).ok_or_else(|| Error::Io(format!("no key for {ch:?}")))?;
        if shifted {
            m.keys(&["shift", name]).map_err(Error::Io)?;
        } else {
            m.keys(&[name]).map_err(Error::Io)?;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    m.keys(&["ret"]).map_err(Error::Io)?;
    std::thread::sleep(Duration::from_millis(500));
    Ok(())
}
