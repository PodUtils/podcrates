//! The save and the restore of a saved state of a [`Plan`], over the
//! steps of [`super::line`].

use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

use pod_core::Class;

use super::line::{self, MONITOR_WAIT, Stopped, Transport};
use super::{Cold, FINGERPRINT, Fingerprint, MAILBOX, ROOT, STATE, VARS, Warm, log_stream};
use crate::plan::MailboxBus;
use crate::swap::{self, Swap};
use crate::{Fat16, Plan, Request, Result, Settings, agent, context, emulator, failure, io_at, overlay_argv, qmp};

/// The `-incoming` arguments of a restore from `dir`, after the machine
/// line of `plan` ([`crate::argv`]), with no program ([`line::incoming`]).
pub fn incoming_argv(plan: &Plan, dir: &Path) -> Result<Vec<String>> {
    line::incoming(&crate::argv(plan), Transport::File, &dir.join(STATE))
}

/// Check that the mailbox `m` is the one of an idle agent in keep-alive
/// mode: [`agent::READY`] and [`agent::KEEP`] on it, and no
/// [`agent::GO`]. A token on the mailbox is a command that waits or runs,
/// and a state with it runs that command again at each restore.
pub fn idle(m: &Fat16) -> Result<()> {
    let why = if !agent::ready(m) {
        "the agent did not write WQREADY.TXT"
    } else if m.get(agent::KEEP).is_none() {
        "no WQKEEP.TXT: the guest is not in keep-alive mode"
    } else if m.get(agent::GO).is_some() {
        "WQGO.TXT is on the mailbox: a command waits or runs"
    } else {
        return Ok(());
    };
    Err(failure(Class::Usage, format!("the guest is not an idle agent: {why}")))
}

/// Refuse a plan whose mailbox is not [`MailboxBus::Usb`]: the restored
/// guest holds the saved mailbox in its cache, and only a new medium
/// gives it the next command.
fn usb_mailbox(plan: &Plan) -> Result<()> {
    if plan.options.mailbox == MailboxBus::Usb {
        return Ok(());
    }
    Err(failure(
        Class::Usage,
        "a warm start needs a USB mailbox (MailboxBus::Usb): the restored guest reads the saved mailbox from its \
         cache, and only a new medium gives it the next command",
    ))
}

/// Freeze the guest of `plan`, which runs, into `dir`, and let it go on.
///
/// The guest must be an idle agent in keep-alive mode ([`idle`]) on a USB
/// mailbox ([`MailboxBus::Usb`], else [`Class::Usage`]): the caller boots
/// it with [`super::stage_idle`] and waits for
/// [`crate::ready::Sign::Marker`]. The save ([`line::freeze`]) stops the
/// guest, writes the migration stream, copies the overlay, the medium in
/// the mailbox device (after a swap it is the other file,
/// [`swap::mailbox_file`]) and the variable store, and then sends `cont`;
/// `fingerprint` comes last. The files go to `DIR.part` first and get the
/// name `dir` at the end, so a save that fails leaves no state and the
/// state before it.
///
/// An emulator that cannot migrate a device of the plan is
/// [`Class::Unavailable`] with its reasons. A save that does not end
/// within `wait` is cancelled ([`Class::Timeout`]). The overlay must name
/// its base by an absolute path: the copy is in another directory.
pub fn save(plan: &Plan, fingerprint: &Fingerprint, dir: &Path, wait: Duration) -> Result<()> {
    let mut part = dir.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let stream = part.join(STATE);
    Transport::File.save_uri(&stream)?;
    usb_mailbox(plan)?;
    let mut copies = Copies { plan, part: &part, medium: None };
    let f = line::Freeze { monitor: &plan.monitor, stream: &stream, transport: Transport::File, fallback: false, wait };
    let frozen = line::freeze(&f, &mut copies);
    let made = frozen.stream.and_then(|_| {
        let file = part.join(FINGERPRINT);
        std::fs::write(&file, fingerprint.text()).map_err(|e| io_at(&file, e))
    });
    if let Err(e) = made {
        let _ = std::fs::remove_dir_all(&part);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(&part, dir).map_err(|e| io_at(dir, e))?;
    frozen.goes_on.map_err(|e| context("the state is saved, and the guest did not go on", e))
}

/// The steps of [`save`] while the guest is stopped, into `part`.
struct Copies<'a> {
    plan: &'a Plan,
    part: &'a Path,
    /// The medium in the mailbox device, from [`Stopped::before`].
    medium: Option<PathBuf>,
}

impl Stopped for Copies<'_> {
    /// The guest is stopped, so the medium on disk is the one that it
    /// sees: it must be the mailbox of an idle agent.
    fn before(&mut self, m: &mut qmp::Monitor) -> Result<()> {
        let block = m.ask("{\"execute\":\"query-block\"}")?;
        let medium = swap::mailbox_file(&block).map(PathBuf::from).ok_or_else(|| {
            failure(Class::Protocol, format!("the emulator names no medium of the mailbox device: {}", block.trim()))
        })?;
        let bytes = std::fs::read(&medium).map_err(|e| io_at(&medium, e))?;
        let mailbox = Fat16::from_image(bytes)
            .ok_or_else(|| failure(Class::Protocol, format!("{}: not a mailbox", medium.display())))?;
        idle(&mailbox)?;
        let part = self.part;
        let _ = std::fs::remove_dir_all(part);
        // The stream holds the memory of the guest.
        std::fs::DirBuilder::new().mode(0o700).create(part).map_err(|e| io_at(part, e))?;
        self.medium = Some(medium);
        Ok(())
    }

    /// The emulator flushed its disks at the end of the migration, and the
    /// guest is stopped: the files are the ones of the stream.
    fn after(&mut self, _: &mut qmp::Monitor) -> Result<()> {
        let (plan, part) = (self.plan, self.part);
        let medium = self.medium.clone().ok_or_else(|| failure(Class::Protocol, "no medium of the mailbox device"))?;
        let root = part.join(ROOT);
        copy(&plan.root, &root)?;
        absolute_backing(&root)?;
        copy(&medium, &part.join(MAILBOX))?;
        copy(&plan.firmware_vars, &part.join(VARS))
    }
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    std::fs::copy(from, to).map(drop).map_err(|e| io_at(from, e))
}

/// Refuse a qcow2 overlay whose backing file has a relative name: the
/// emulator reads it from the directory of the overlay, and the copy is
/// in another one.
pub(super) fn absolute_backing(qcow2: &Path) -> Result<()> {
    let mut f = std::fs::File::open(qcow2).map_err(|e| io_at(qcow2, e))?;
    let mut head = [0u8; 20];
    f.read_exact(&mut head).map_err(|e| io_at(qcow2, e))?;
    if head[..4] != *b"QFI\xfb" {
        return Err(failure(Class::Protocol, format!("{}: not a qcow2 image", qcow2.display())));
    }
    let offset = u64::from_be_bytes(head[8..16].try_into().unwrap_or_default());
    let size = u32::from_be_bytes(head[16..20].try_into().unwrap_or_default());
    // The format limits the name to 1023 bytes.
    if offset == 0 || size == 0 || size > 1023 {
        return Ok(());
    }
    let mut name = vec![0u8; size as usize];
    f.seek(SeekFrom::Start(offset)).and_then(|_| f.read_exact(&mut name)).map_err(|e| io_at(qcow2, e))?;
    if name.first() == Some(&b'/') || name.windows(3).any(|w| w == b"://") || name.starts_with(b"json:") {
        return Ok(());
    }
    Err(failure(
        Class::Usage,
        format!(
            "the overlay names its base {:?} by a relative path; give the base by an absolute path",
            String::from_utf8_lossy(&name)
        ),
    ))
}

/// Start the guest of `plan` from the state of `warm`, give it the command
/// of `request`, and return the emulator that runs and the media of the
/// mailbox, with the command on the medium in the guest.
///
/// The emulator loads the state with the saved mailbox as it is. The
/// restored guest holds that mailbox in its cache, so the command comes
/// after the load on a new medium ([`Swap::restored`], [`Swap::next`] with
/// `agent_cmd`), as each next command of a guest that runs.
///
/// A failure with the cause [`Cold`] says why the state was not used: no
/// state, another fingerprint, or a load or a swap that failed (the
/// emulator is then stopped, and the variable store of the caller is put
/// back). Each other failure (a bad option of the plan, a mailbox that is
/// not [`MailboxBus::Usb`], an I/O error before the state is read) has no
/// such cause.
pub fn restore(
    s: &Settings,
    qemu_img: &Path,
    plan: &Plan,
    agent_cmd: &str,
    request: &Request,
    warm: &Warm,
) -> Result<(Child, Swap)> {
    plan.options.check()?;
    usb_mailbox(plan)?;
    let dir = &warm.dir;
    let saved = match std::fs::read_to_string(dir.join(FINGERPRINT)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Cold::Missing.failure()),
        Err(e) => return Err(io_at(&dir.join(FINGERPRINT), e)),
    };
    if [STATE, ROOT, MAILBOX, VARS].iter().any(|f| !dir.join(f).is_file()) {
        return Err(Cold::Missing.failure());
    }
    let now = warm.fingerprint.text();
    if saved != now {
        let (saved, now) = line::first_difference(&saved, &now);
        return Err(Cold::Stale { saved, now }.failure());
    }
    let fresh_vars = std::fs::read(&plan.firmware_vars).map_err(|e| io_at(&plan.firmware_vars, e))?;
    let loaded = load_from(s, qemu_img, plan, agent_cmd, request, warm);
    if loaded.is_err() {
        // A cold boot after this one takes the store of the caller.
        let _ = std::fs::write(&plan.firmware_vars, &fresh_vars);
    }
    loaded.map_err(|e| Cold::Failed { why: e.to_string() }.failure())
}

/// The files of the run from the state, the emulator, the load, and the
/// command on a new medium.
fn load_from(
    s: &Settings,
    qemu_img: &Path,
    plan: &Plan,
    agent_cmd: &str,
    request: &Request,
    warm: &Warm,
) -> Result<(Child, Swap)> {
    let dir = &warm.dir;
    let args = incoming_argv(plan, dir)?;
    let file = dir.join(MAILBOX);
    let bytes = std::fs::read(&file).map_err(|e| io_at(&file, e))?;
    // The saved mailbox stays as the restored RAM knows it: a command on it
    // is hidden by the cache of the guest.
    let m = Fat16::from_image(bytes)
        .ok_or_else(|| failure(Class::Protocol, format!("{}: not a mailbox", file.display())))?;
    idle(&m)?;
    let _ = std::fs::remove_file(&plan.root);
    let _ = std::fs::remove_file(&plan.monitor);
    // A thin overlay: the saved one stays as it is, and the next restore
    // reads it again.
    emulator::run_steps(&overlay_argv(qemu_img, &dir.join(ROOT), &plan.root))?;
    std::fs::write(&plan.mailbox, m.image()).map_err(|e| io_at(&plan.mailbox, e))?;
    copy(&dir.join(VARS), &plan.firmware_vars)?;
    let log = emulator::emulator_log(s, plan);
    let mut running = emulator::Running::new(plan, emulator::spawn_emulator(plan, &args, log_stream(s, plan)?)?);
    let given = load(plan, running.child(), warm.wait).and_then(|()| {
        let mut swap = Swap::restored(plan)?;
        let mut monitor = qmp::Monitor::connect(&plan.monitor, MONITOR_WAIT)?;
        swap.next(&mut monitor, s, agent_cmd, request).map(|()| swap)
    });
    match given {
        Ok(swap) => Ok((running.into_child(), swap)),
        Err(e) => {
            // The stop comes before the log is read. An emulator that
            // exited is reaped and gets no signal.
            drop(running);
            let tail = log.as_deref().map(emulator::log_tail).unwrap_or_default();
            Err(failure(e.class, format!("{e}{tail}")))
        }
    }
}

/// Wait for the end of the load of the emulator `child` within `wait`,
/// then let the guest go on ([`line::running`] on the QMP socket of
/// `plan`). An emulator that exits, or another status, is a failed load.
pub(super) fn load(plan: &Plan, child: &mut Child, wait: Duration) -> Result<()> {
    line::running(&plan.monitor, Some(child), wait)
}
