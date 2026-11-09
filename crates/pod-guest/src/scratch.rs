use super::*;

/// The per-run directory: where the mailbox, the QMP socket and the serial
/// log are, which is the directory [`scratch_dir`] made. `None` where the
/// plan's mailbox has no parent.
///
/// ⛔ **Not `Plan::root`'s parent, and that distinction is a defect that was
/// caught in review.** A cleanup keyed on `root` would remove whatever
/// directory the root disk lives in, and on the `setup` path that is a
/// caller's directory of Windows images beside the vendor's. The mailbox
/// never moves out of the per-run directory, which is why it is the field this
/// reads. `TODO/gate.md` T-1641 took `setup`'s `root` back into the per-run
/// directory as well; this is keyed on the mailbox because `run`'s root is
/// still per-run and the two must not depend on that staying true.
pub fn scratch_of(plan: &Plan) -> Option<PathBuf> {
    plan.mailbox.parent().map(|p| p.to_path_buf())
}

/// The prefix every per-run directory carries, used both to create one and
/// to be sure of one before removing it.
pub const SCRATCH_PREFIX: &str = "podvm-windows-";

/// Remove a per-run directory unless `PODVM_WINDOWS_KEEP` is set.
///
/// ⛔ **A run's leftovers are about a hundred megabytes**, measured: a 56 MB
/// overlay, a 17 MB mailbox and a 0.5 MB variable store, per run. Leaving
/// them behind is how the runs earlier in this entry's own record failed at
/// 300 s: the sandbox's 489 MB tmpfs filled, the host saw ENOSPC on the
/// writethrough mailbox, and the guest never answered. So the default is to
/// remove, and the override exists because a run that needs looking at is
/// exactly the run that failed.
///
/// ⚠ **Two guards before anything is removed, and both are refusals rather
/// than errors.** The directory must be the one [`scratch_of`] names *and*
/// its own name must carry [`SCRATCH_PREFIX`]. A recursive delete driven by
/// a path a caller can influence is the one operation here that can destroy
/// work that is not ours, so a plan that does not look like ours is left
/// exactly as it is. Best effort after that: a refusal to remove is not a
/// failure of the run, which has already returned its result.
pub fn cleanup(plan: &Plan) {
    if std::env::var_os("PODVM_WINDOWS_KEEP").is_some() {
        return;
    }
    let Some(dir) = scratch_of(plan) else {
        return;
    };
    let named = dir
        .file_name()
        .map(|n| n.to_string_lossy().starts_with(SCRATCH_PREFIX))
        .unwrap_or(false);
    if !named {
        return;
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A per-run directory that removes itself when it falls out of scope.
///
/// `cleanup` runs on every coded exit; this runs on the exits without one:
/// a panic past the staging point, where the explicit calls never execute.
/// SIGKILL aside, which no guard survives. Removing twice is safe
/// (`cleanup` ignores a missing directory through `let _`), so the coded
/// paths keep their explicit calls and this only backstops them.
pub struct RunGuard<'a> {
    plan: &'a Plan,
}

impl<'a> RunGuard<'a> {
    /// Arm the guard for `plan`. Name the binding so it lives to the end
    /// of the run: a guard dropped at once guards nothing.
    pub fn arm(plan: &'a Plan) -> Self {
        RunGuard { plan }
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        cleanup(self.plan);
    }
}

/// A per-run directory, mode 0700, holding the QMP socket, the mailbox, the
/// overlay and the serial log.
///
/// ⛔ **The mode is the point.** The QMP socket lets whoever connects type
/// into the guest console, so the directory that holds it is not readable
/// or traversable by another local user.
pub fn scratch_dir(tag: &str) -> Result<PathBuf, Error> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("{SCRATCH_PREFIX}{}-{tag}", std::process::id()));
    let mut b = std::fs::DirBuilder::new();
    b.mode(0o700);
    match b.create(&dir) {
        Ok(()) => Ok(dir),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(dir),
        Err(e) => Err(Error::Io(format!("{}: {e}", dir.display()))),
    }
}

/// The cached base image: `PODVM_WINDOWS_BASE` where it is set, so an
/// experiment isolates with its own copy, else under the home directory so
/// it survives between runs the way an image cache should.
pub fn base_cache() -> PathBuf {
    if let Some(p) = std::env::var_os("PODVM_WINDOWS_BASE")
        && !p.is_empty()
    {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into());
    Path::new(&home).join(".local/share/podvm/windows/base.vhdx")
}
