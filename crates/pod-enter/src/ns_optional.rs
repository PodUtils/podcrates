//! The mounts of the namespace rung that do not stop it: a fresh proc on
//! the `/proc` of the image, and a bind of the host's `/dev/pts` on the
//! `/dev/pts` of the image (in a user namespace, first a new devpts
//! instance: `ns_pts`). Each is made where the image has that
//! directory (not a link). A refused one writes one line to the payload's
//! standard error, and the payload runs as on the chroot rung.
//!
//! In a user namespace of the rung (`ns_user`) the kernel refuses the
//! fresh proc with `EPERM`: the PID namespace belongs to the user
//! namespace of the host. There the child binds the host's `/proc`
//! (`ns_proc_bind`), where that proc is the one of the PID namespace of the
//! entry.

use pod_hostprobe::sys::{self, CBuf};

use crate::entry::{decimal, ns_dir_target, ns_setup_step};
use crate::names::Names;
use crate::ns_proc_bind::Fallback;
use crate::ns_pts::NewPts;

/// The step byte of the proc mount. It names its step but never goes to
/// the readiness pipe: a refused proc mount does not stop the rung.
pub const NS_PROC_STEP: u8 = 15;

/// The step byte of the bind of `/dev/pts`, as [`NS_PROC_STEP`].
pub const NS_PTS_STEP: u8 = 17;

/// The proc of the namespace rung: no set-user-ID, no device and no
/// program runs from it.
const PROC_FLAGS: u64 = sys::MS_NOSUID | sys::MS_NODEV | sys::MS_NOEXEC;

/// One mount that does not stop the rung, built before the fork: the child
/// cannot allocate, so the line of a refusal is made here too.
pub(crate) struct Optional {
    source: CBuf,
    target: CBuf,
    fstype: CBuf,
    flags: u64,
    refused: Vec<u8>,
    /// What the child does at `EPERM` in a user namespace.
    fallback: Fallback,
    /// The new devpts instance that replaces the bind in a user namespace
    /// (`ns_pts`).
    devpts: Option<NewPts>,
}

/// The `/proc` of the image as a mount target, with the rule of
/// [`crate::ns_tmp_target`]. `None` skips the proc mount.
pub fn ns_proc_target(root: &str) -> Option<String> {
    ns_dir_target(root, "proc")
}

/// The `/dev/pts` of the image as a mount target, with the same rule.
/// `None` skips the bind: `ttyname` of a pty then fails in the payload.
pub fn ns_pts_target(root: &str) -> Option<String> {
    ns_dir_target(root, "dev/pts")
}

/// The start of the line of a refused optional mount: what the payload
/// lacks and the name of the step. The child adds the number of the
/// errno and a newline.
pub fn ns_refused_line(names: &Names, lacks: &str, step: u8) -> String {
    let step = ns_setup_step(step).unwrap_or("an optional mount");
    format!("{}: the namespace rung runs with no {lacks}: {step} failed with errno ", names.product)
}

/// The start of the line of a refused proc mount.
pub fn ns_proc_refused(names: &Names) -> String {
    ns_refused_line(names, "/proc", NS_PROC_STEP)
}

/// The optional mounts for the image at `root`, in their order.
pub(crate) fn optional_mounts(root: &str, names: &Names) -> Vec<Optional> {
    let lit = |s: &str| CBuf::new(s).expect("a literal");
    let mut out = Vec::new();
    // A target with a NUL byte cannot come from `format!` over a path
    // that `RootDir` holds: such a path has none.
    if let Some(target) = ns_proc_target(root).and_then(|t| CBuf::new(&t)) {
        let refused = ns_proc_refused(names).into_bytes();
        let fallback = crate::ns_proc_bind::fallback(names);
        let (source, fstype) = (lit("proc"), lit("proc"));
        out.push(Optional { source, target, fstype, flags: PROC_FLAGS, refused, fallback, devpts: None });
    }
    if let Some(target) = ns_pts_target(root).and_then(|t| CBuf::new(&t)) {
        let refused = ns_refused_line(names, "/dev/pts", NS_PTS_STEP).into_bytes();
        let flags = sys::MS_BIND | sys::MS_REC;
        let (fallback, devpts) = (Fallback::None, Some(NewPts::new(names)));
        out.push(Optional { source: lit("/dev/pts"), target, fstype: sys::cempty(), flags, refused, fallback, devpts });
    }
    out
}

/// Make each optional mount, in the child after the tmpfs on `/tmp`: no
/// allocation. A refused one writes its line and the next one goes on.
/// Where a user namespace owns the mount namespace (the one of the rung, or
/// one of the caller), `/dev/pts` is first a new devpts instance
/// (`ns_pts`), `EPERM` calls for the fallback of the mount, and the host's
/// devices come after (`ns_dev_bind`).
pub(crate) fn mount_each(ns: &crate::entry::NsMount) {
    let user = ns.userns();
    for m in &ns.optional {
        if let Some(n) = m.devpts.as_ref().filter(|_| user)
            && n.mount(&m.target).is_ok()
        {
            continue;
        }
        let Err(e) = sys::mount(&m.source, &m.target, &m.fstype, m.flags) else { continue };
        if user && e == sys::EPERM && !matches!(m.fallback, Fallback::None) {
            crate::ns_proc_bind::bind(&m.fallback, &m.target);
            continue;
        }
        refused_line(&m.refused, e);
    }
    if let Some(devices) = ns.devices.as_ref().filter(|_| user) {
        crate::ns_dev_bind::bind_each(devices);
    }
}

/// The start of a line, the number of the errno and a newline, to the
/// standard error of the payload. No allocation.
pub(crate) fn refused_line(start: &[u8], e: sys::Errno) {
    let mut digits = [0u8; 10];
    let _ = sys::write(2, start);
    let _ = sys::write(2, decimal(e.0.unsigned_abs(), &mut digits));
    let _ = sys::write(2, b"\n");
}
