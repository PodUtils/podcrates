//! A devpts instance of the unit on the namespace rung in a user namespace
//! (`ns_user`, PodUtils/podvm#469).
//!
//! The host's devpts usually has `ptmxmode=000`, and the image of a user
//! namespace holds no `/dev/ptmx` node (the kernel refuses `mknod` there).
//! With only the bind of the host's `/dev/pts`, `openpty` fails. A user
//! namespace can mount a new devpts instance: the child mounts one on the
//! `/dev/pts` of the image, and binds its `ptmx` on the `/dev/ptmx` of the
//! image (`ns_dev_bind`). The options give no `gid=`: the group `tty` of
//! the host has no id in the user namespace.
//!
//! Where the kernel refuses the new instance, the child writes one line,
//! binds the host's `/dev/pts` as on a rung with no user namespace
//! (`ns_optional`), and binds its `ptmx` on `/dev/ptmx`. That `ptmx` opens
//! only where the host's `ptmxmode` lets the caller open it.

use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::entry::ns_setup_step;
use crate::names::Names;

/// The step byte of the mount of the new devpts instance. It names its
/// step but never goes to the readiness pipe, as [`crate::NS_PROC_STEP`].
pub const NS_DEVPTS_STEP: u8 = 46;
/// The step byte of the bind of the `ptmx` of that instance on
/// `/dev/ptmx`, as [`NS_DEVPTS_STEP`].
pub const NS_PTMX_BIND_STEP: u8 = 47;

/// The options of the new instance: a `ptmx` that each user opens, and
/// each new terminal readable by its owner and writable by its group.
pub const DEVPTS_OPTIONS: &str = "newinstance,ptmxmode=0666,mode=0620";

/// The flags of the new instance: no set-user-ID and no program. No
/// `MS_NODEV`: the terminals must work.
pub const DEVPTS_FLAGS: u64 = sys::MS_NOSUID | sys::MS_NOEXEC;

/// The mount of a new devpts instance, built before the fork.
pub(crate) struct NewPts {
    fstype: CBuf,
    data: CBuf,
    refused: Vec<u8>,
}

/// The start of the line where the kernel refuses the new instance. The
/// child adds the number of the errno and a newline.
pub fn devpts_refused(names: &Names) -> String {
    let step = ns_setup_step(NS_DEVPTS_STEP).unwrap_or("the mount of a new devpts instance");
    format!("{}: the namespace rung binds the host's /dev/pts and its ptmx: {step} failed with errno ", names.product)
}

/// The `ptmx` of the new instance on the `/dev/pts` target `pts`.
pub fn ptmx_of(pts: &str) -> String {
    format!("{}/ptmx", pts.trim_end_matches('/'))
}

impl NewPts {
    pub(crate) fn new(names: &Names) -> NewPts {
        let lit = |s: &str| CBuf::new(s).expect("a literal");
        NewPts { fstype: lit("devpts"), data: lit(DEVPTS_OPTIONS), refused: devpts_refused(names).into_bytes() }
    }

    /// In the child: the new instance on `target`. A refusal writes its
    /// line, and the caller binds the host's `/dev/pts`. No allocation.
    pub(crate) fn mount(&self, target: &CBuf) -> Result<(), Errno> {
        let r = sys::mount_data(&self.fstype, target, &self.fstype, DEVPTS_FLAGS, &self.data);
        if let Err(e) = r {
            crate::ns_optional::refused_line(&self.refused, e);
        }
        r.map(drop)
    }
}

#[cfg(test)]
mod tests;
