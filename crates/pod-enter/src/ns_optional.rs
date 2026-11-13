//! The mounts of the namespace rung that do not stop it: a fresh proc on
//! the `/proc` of the image, and a bind of the host's `/dev/pts` on the
//! `/dev/pts` of the image. Each is made where the image has that
//! directory (not a link). A refused one writes one line to the payload's
//! standard error, and the payload runs as on the chroot rung.

use pod_hostprobe::sys::{self, CBuf};

use crate::entry::{decimal, ns_dir_target, ns_setup_step};
use crate::names::Names;

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
        out.push(Optional { source: lit("proc"), target, fstype: lit("proc"), flags: PROC_FLAGS, refused });
    }
    if let Some(target) = ns_pts_target(root).and_then(|t| CBuf::new(&t)) {
        let refused = ns_refused_line(names, "/dev/pts", NS_PTS_STEP).into_bytes();
        let flags = sys::MS_BIND | sys::MS_REC;
        out.push(Optional { source: lit("/dev/pts"), target, fstype: sys::cempty(), flags, refused });
    }
    out
}

/// Make each optional mount, in the child after the tmpfs on `/tmp`: no
/// allocation. A refused one writes its line and the next one goes on.
pub(crate) fn mount_each(list: &[Optional]) {
    for m in list {
        if let Err(e) = sys::mount(&m.source, &m.target, &m.fstype, m.flags) {
            let mut digits = [0u8; 10];
            let _ = sys::write(2, &m.refused);
            let _ = sys::write(2, decimal(e.0.unsigned_abs(), &mut digits));
            let _ = sys::write(2, b"\n");
        }
    }
}
