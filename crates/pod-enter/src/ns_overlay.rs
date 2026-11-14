//! The overlay root of the namespace rung: the caller's tree is the
//! read-only lower layer of an overlay, and each write of the payload goes
//! to an upper layer (a private tmpfs of the run, or a directory of the
//! caller). The lower directory never changes.
//!
//! In the child: `fchdir` onto the held lower descriptor before the
//! unshare (which moves the working directory into the new namespace),
//! then, after the private remount of `/`, the tmpfs of the layers on the
//! root path (where the
//! upper layer is a tmpfs), the upper and work directories in it, then
//! overlayfs on the root path with `lowerdir=.`. The lower layer is the
//! descriptor, not a path. Then the rung goes on as for a tree: the tmpfs
//! on `/tmp`, the optional mounts, the chroot by path. The landing check
//! asks for an overlay that is not the lower directory.
//!
//! A refusal at any step is [`Error::Overlay`]: there is no fallback to
//! chroot, because chroot would run the payload on the lower directory.

use std::io::Write;

use pod_core::Class;
use pod_hostprobe::select::Rung;
use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::entry::{NsEntry, NsMount, ns_setup_step, ns_tmp_target, spawn_namespace};
use crate::launch::spawn_with;
use crate::ns_optional::optional_mounts;
use crate::{Child, Error, Plan, Result, RootDir};

/// The upper layer of an overlay root.
#[derive(Clone, Copy, Debug)]
pub enum Upper<'a> {
    /// A private tmpfs of the mount namespace of the run, at most `size`
    /// bytes (0: the default of the kernel, half of the memory). Each
    /// write goes away with the run.
    Tmpfs { size: u64 },
    /// A directory of the caller, and an empty work directory on the same
    /// file system; both absolute. Each write stays in `upper`.
    Dir { upper: &'a str, work: &'a str },
}

/// The root of the namespace rung.
#[derive(Clone, Copy, Debug)]
pub enum Root<'a> {
    /// A tree of the run: the payload writes into it.
    Tree(&'a RootDir),
    /// A shared tree as the read-only lower layer of an overlay.
    Overlay { lower: &'a RootDir, upper: Upper<'a> },
}

impl Root<'_> {
    /// The held directory: the tree, or the lower layer.
    pub fn dir(&self) -> &RootDir {
        match self {
            Root::Tree(d) | Root::Overlay { lower: d, .. } => d,
        }
    }
}

/// The step bytes of the overlay root.
pub const NS_OVERLAY_TMPFS_STEP: u8 = 18;
pub const NS_OVERLAY_DIRS_STEP: u8 = 19;
pub const NS_OVERLAY_STEP: u8 = 20;

/// The most bytes of the options of a mount: `mount(2)` copies one page.
pub const OVERLAY_OPTIONS_MOST: usize = 4095;

/// `path` as one value of an overlay option: overlayfs splits the options
/// at `,` and the layers at `:`, and takes `\` as the escape.
pub fn overlay_escape(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, '\\' | ',' | ':') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The options of the overlay mount. The lower layer is `.`: the working
/// directory of the child, which is the held lower descriptor.
pub fn overlay_options(upper: &str, work: &str) -> String {
    format!("lowerdir=.,upperdir={},workdir={}", overlay_escape(upper), overlay_escape(work))
}

/// The tmpfs of the layers and its two directories, built before the fork.
pub(crate) struct Staging {
    tmpfs: CBuf,
    data: CBuf,
    upper: CBuf,
    work: CBuf,
    /// The mode and the owner of the lower root: the root of the overlay
    /// takes them from the upper directory.
    mode: u64,
    uid: u32,
    gid: u32,
}

/// What the child mounts for the overlay root, built before the fork: the
/// child cannot allocate.
pub(crate) struct OverlayMount {
    staging: Option<Staging>,
    fstype: CBuf,
    options: CBuf,
}

impl OverlayMount {
    pub(crate) fn new(lower: &RootDir, upper: Upper<'_>) -> Result<OverlayMount> {
        let usage = |what: String| Error::runtime(Class::Usage, format!("the overlay root: {what}"));
        let cbuf = |s: &str| CBuf::new(s).ok_or_else(|| usage(format!("{s:?} contains a NUL byte")));
        let (staging, upper_path, work_path) = match upper {
            Upper::Tmpfs { size } => {
                let st = sys::fstatat(lower.fd(), &sys::cempty(), sys::AT_EMPTY_PATH)
                    .map_err(|e| crate::errno_failure(&format!("the overlay root: fstat of {}", lower.path), e))?;
                let base = lower.path.trim_end_matches('/');
                let (upper, work) = (format!("{base}/upper"), format!("{base}/work"));
                let data = if size == 0 { "mode=0700".to_string() } else { format!("size={size},mode=0700") };
                let staging = Staging {
                    tmpfs: cbuf("tmpfs")?,
                    data: cbuf(&data)?,
                    upper: cbuf(&upper)?,
                    work: cbuf(&work)?,
                    mode: u64::from(st.st_mode & 0o7777),
                    uid: st.st_uid,
                    gid: st.st_gid,
                };
                (Some(staging), upper, work)
            }
            Upper::Dir { upper, work } => {
                if !upper.starts_with('/') || !work.starts_with('/') {
                    return Err(usage(format!("the upper {upper:?} and the work {work:?} must be absolute paths")));
                }
                (None, upper.to_string(), work.to_string())
            }
        };
        let options = overlay_options(&upper_path, &work_path);
        if options.len() > OVERLAY_OPTIONS_MOST {
            return Err(usage(format!(
                "its options are {} bytes, and mount(2) takes at most {OVERLAY_OPTIONS_MOST}",
                options.len()
            )));
        }
        Ok(OverlayMount { staging, fstype: cbuf("overlay")?, options: cbuf(&options)? })
    }
}

/// Mount the overlay on `target`, the root path, in the child: no
/// allocation. The working directory is the lower descriptor, set before
/// the unshare. `Err` gives the step byte and the errno.
pub(crate) fn mount(m: &OverlayMount, target: &CBuf) -> std::result::Result<(), (u8, Errno)> {
    if let Some(s) = &m.staging {
        sys::mount_data(&s.tmpfs, target, &s.tmpfs, 0, &s.data).map_err(|e| (NS_OVERLAY_TMPFS_STEP, e))?;
        for dir in [&s.upper, &s.work] {
            sys::mkdir(dir, 0o700).map_err(|e| (NS_OVERLAY_DIRS_STEP, e))?;
        }
        // The mode of `mkdir` passes the umask; the mode of the root must not.
        sys::fchmodat(sys::AT_FDCWD as i64, &s.upper, s.mode, 0).map_err(|e| (NS_OVERLAY_DIRS_STEP, e))?;
        // An owner with no map in the namespace is EINVAL: the root then
        // keeps the owner of the namespace, as a tree that the run fills.
        let _ = sys::fchownat(sys::AT_FDCWD as i64, &s.upper, s.uid, s.gid, 0);
    }
    sys::mount_data(&m.fstype, target, &m.fstype, 0, &m.options).map_err(|e| (NS_OVERLAY_STEP, e))?;
    Ok(())
}

/// Did the chroot land on an overlay that is not the lower directory? An
/// overlay on the lower path has its own device, so the device of the
/// lower descriptor names a landing on the lower directory itself.
pub(crate) fn landed(lower: &sys::Stat, slash: &CBuf) -> bool {
    let overlay = sys::statfs(slash).is_ok_and(|f| f.f_type == sys::OVERLAYFS_SUPER_MAGIC);
    let other = sys::fstatat(sys::AT_FDCWD as i64, slash, 0).is_ok_and(|s| s.st_dev != lower.st_dev);
    overlay && other
}

/// The name of a step of the overlay root.
pub(crate) fn overlay_step(step: &'static str) -> &'static str {
    if Some(step) == ns_setup_step(10) { "verifying the chroot landed on the overlay" } else { step }
}

/// Enter `root` in a mount namespace and exec: [`spawn_namespace`] for a
/// tree (with its fallback to chroot), the overlay root for an overlay.
///
/// The overlay root needs the `/tmp` of the image as a directory, as the
/// tree does. Each refusal of the overlay root is [`Error::Overlay`],
/// before the payload starts: the caller can fill a tree and enter it.
pub fn spawn_namespace_root(root: Root<'_>, plan: &Plan, err: &mut dyn Write) -> Result<Child> {
    let (lower, upper) = match root {
        Root::Tree(dir) => return spawn_namespace(dir, plan, err),
        Root::Overlay { lower, upper } => (lower, upper),
    };
    let Some(tmp_target) = ns_tmp_target(&lower.path).and_then(|t| CBuf::new(&t)) else {
        return Err(Error::Overlay { step: "the /tmp of the image as a mount point", errno: sys::ENOTDIR });
    };
    let overlay = Some(OverlayMount::new(lower, upper)?);
    let mount = NsMount { tmp_target, optional: optional_mounts(&lower.path, &plan.names), overlay };
    match spawn_with(lower, plan, Rung::Namespace.word(), None, None, None, Some(NsEntry::Own(&mount)), err) {
        Err(Error::NsSetup { step, errno }) => Err(Error::Overlay { step: overlay_step(step), errno }),
        other => other,
    }
}

/// Run a payload in a mount namespace on `root`, and return its exit
/// status. As [`crate::run`], the exit status is the status of the
/// payload.
pub fn run_namespace_root(root: Root<'_>, plan: &Plan, err: &mut dyn Write) -> Result<i32> {
    let child = spawn_namespace_root(root, plan, err)?;
    child.wait_forwarding(err)
}

#[cfg(test)]
mod tests;
