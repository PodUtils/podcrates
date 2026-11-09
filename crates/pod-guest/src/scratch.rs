//! The per-run directory: the mailbox, the QMP socket, the overlay, the
//! serial log and the log of the emulator.

use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use crate::{Plan, Result, Settings, io_at};

/// The per-run directory of `plan`: the parent of [`Plan::mailbox`]. `None`
/// when the mailbox has no parent.
///
/// The key is the mailbox, not [`Plan::root`]. On the provision path a
/// caller can put the root disk beside its own images; a cleanup keyed on
/// the root would remove that directory. The mailbox is always in the
/// per-run directory.
pub fn scratch_of(plan: &Plan) -> Option<PathBuf> {
    plan.mailbox.parent().map(Path::to_path_buf)
}

/// True when the name of `dir` starts with [`Settings::scratch_prefix`].
pub(crate) fn is_scratch(s: &Settings, dir: &Path) -> bool {
    dir.file_name().map(|n| n.to_string_lossy().starts_with(&s.scratch_prefix)).unwrap_or(false)
}

/// Remove the per-run directory of `plan`, unless [`Settings::keep`].
///
/// A run leaves about 100 MB: an overlay of 56 MB, a mailbox of 17 MB and
/// a variable store of 0.5 MB. A full tmpfs gives `ENOSPC` on the mailbox,
/// and the guest then never answers. So the default is to remove.
///
/// Two guards come before the removal, and each is a refusal, not an
/// error: the directory is the one that [`scratch_of`] names, and its name
/// starts with [`Settings::scratch_prefix`]. A plan that does not look like
/// one of this crate stays as it is. A removal that fails is not a failure
/// of the run, which returned its result.
pub fn cleanup(s: &Settings, plan: &Plan) {
    if s.keep {
        return;
    }
    let Some(dir) = scratch_of(plan) else {
        return;
    };
    if !is_scratch(s, &dir) {
        return;
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A per-run directory that removes itself when it goes out of scope.
///
/// [`cleanup`] runs on each coded exit; this guard runs on the exits with
/// no code: a panic after the stage. No guard survives `SIGKILL`. A second
/// removal is safe, so the coded paths keep their own calls.
pub struct RunGuard<'a> {
    settings: Settings,
    plan: &'a Plan,
}

impl<'a> RunGuard<'a> {
    /// Arm the guard for `plan`. Give the guard a name, so that it lives to
    /// the end of the run: a guard that is dropped at once guards nothing.
    pub fn arm(s: &Settings, plan: &'a Plan) -> Self {
        RunGuard { settings: s.clone(), plan }
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        cleanup(&self.settings, self.plan);
    }
}

/// Make a per-run directory, mode 0700, in [`Settings::scratch_root`]: the
/// QMP socket, the mailbox, the overlay and the serial log go in it.
///
/// The mode is necessary: a process that connects to the QMP socket can
/// type into the console of the guest, so no other local user can read or
/// enter the directory.
pub fn scratch_dir(s: &Settings, tag: &str) -> Result<PathBuf> {
    let dir = s.scratch_root.join(format!("{}{}-{tag}", s.scratch_prefix, std::process::id()));
    let mut b = std::fs::DirBuilder::new();
    b.mode(0o700);
    match b.create(&dir) {
        Ok(()) => Ok(dir),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(dir),
        Err(e) => Err(io_at(&dir, e)),
    }
}
