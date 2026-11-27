//! The overlay volumes of a plan on the namespace rung
//! ([`crate::VolumeBind::overlay`], PodUtils/podvm#689): `-v
//! HOST:GUEST:overlay`, an overlayfs mount on GUEST whose read-only lower
//! layer is the directory HOST, with the upper and the work directory of
//! the caller.
//!
//! Before the fork (`ns_volume::prepare`): HOST is a directory, the volume
//! is not `read_only`, the upper and the work directory are absolute paths
//! of directories, none of the three is at or under another (with each
//! link resolved), and the options fit in one page with `userxattr`. A
//! refusal names the volume.
//!
//! In the child, at the place of the bind in the order of the volumes
//! (`ns_volume::bind_one`): the mount point is made as for a bind; HOST
//! opens as a directory (under `host_base` with `pod-beneath` where it is
//! set); `fchdir` to that descriptor; then `mount("overlay",
//! /proc/self/fd/N, "overlay", 0, "lowerdir=.,upperdir=U,workdir=W")`. The
//! lower layer is the descriptor, not a path. Where a user namespace owns
//! the mount namespace (the user namespace of the rung, or one of the
//! caller), the options have `userxattr`, with no second try without it: a
//! mount with no `userxattr` there reads no whiteout. The working directory
//! stays on HOST: the pivot (`ns_root`) changes it. The mount comes before
//! the nested user namespace of the payload (`ns_lock`), which locks it as
//! each other mount of the rung.
//!
//! A refused step stops the payload and names the volume (`ns_volume::failure`):
//! no fallback to a bind, as a bind would write HOST.

use pod_core::Class;
use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::ns_overlay::{OVERLAY_OPTIONS_MOST, USERXATTR, overlay_options};
use crate::{OverlayLayers, VolumeBind};

/// The step byte of the open of the lower layer, or of the change of the
/// working directory to it.
pub const NS_LAYER_LOWER_STEP: u8 = 61;
/// The step byte of the overlayfs mount of a volume.
pub const NS_LAYER_MOUNT_STEP: u8 = 62;
/// The step byte of an overlayfs mount that the kernel refused because its
/// layers overlap (`ELOOP`) or another mount uses a layer (`EBUSY`).
pub const NS_LAYER_OVERLAP_STEP: u8 = 63;

/// The words of a step of an overlay volume, or `None` where the step is
/// not one.
pub(crate) fn step_name(step: u8) -> Option<&'static str> {
    match step {
        NS_LAYER_LOWER_STEP => Some("opening the lower layer of an overlay volume as a directory"),
        NS_LAYER_MOUNT_STEP => Some("mounting overlayfs on the guest path of an overlay volume"),
        NS_LAYER_OVERLAP_STEP => {
            Some("mounting overlayfs on an overlay volume whose layers overlap or are in use by another mount")
        }
        _ => None,
    }
}

/// The overlayfs mount of one volume, built before the fork.
pub(crate) struct Layer {
    fstype: CBuf,
    options: CBuf,
    /// [`Layer::options`] with [`USERXATTR`].
    options_userxattr: CBuf,
}

impl Layer {
    /// The options of the mount: with `userxattr` where a user namespace
    /// owns the mount namespace.
    pub(crate) fn options(&self, userns: bool) -> &CBuf {
        if userns { &self.options_userxattr } else { &self.options }
    }
}

/// Check the overlay of the volume `v`, whose host path is a directory
/// where `dir`, and build its mount. `Err` is why, for a refusal that names
/// the volume.
pub(crate) fn prepare(v: &VolumeBind, layers: &OverlayLayers, dir: bool) -> Result<Layer, String> {
    if v.read_only {
        return Err("is read-only and an overlay: the overlay is the writable layer over a read-only host path; \
                    give one of the two"
            .to_string());
    }
    if !dir {
        return Err("has a host path that is not a directory: the lower layer of an overlay is a directory".to_string());
    }
    for (what, path) in [("an upper", &layers.upper), ("a work", &layers.work)] {
        if !path.starts_with('/') {
            return Err(format!("has {what} directory {path:?} that is not an absolute path"));
        }
        let c = CBuf::new(path).ok_or_else(|| "has a NUL byte".to_string())?;
        match sys::stat(&c) {
            Ok(st) if st.st_mode & sys::S_IFMT == sys::S_IFDIR => {}
            Ok(_) => return Err(format!("has {what} directory {path:?} that is not a directory")),
            Err(e) => return Err(format!("has {what} directory {path:?} that cannot be read ({})", e.name())),
        }
    }
    if let Some(why) = overlap(&v.host, layers) {
        return Err(why);
    }
    let options = overlay_options(&layers.upper, &layers.work);
    if options.len() + USERXATTR.len() > OVERLAY_OPTIONS_MOST {
        return Err(format!(
            "has overlay options of {} bytes, and mount(2) takes at most {OVERLAY_OPTIONS_MOST}",
            options.len() + USERXATTR.len()
        ));
    }
    let nul = || "has a NUL byte".to_string();
    Ok(Layer {
        fstype: CBuf::new("overlay").ok_or_else(nul)?,
        options_userxattr: CBuf::new(&format!("{options}{USERXATTR}")).ok_or_else(nul)?,
        options: CBuf::new(&options).ok_or_else(nul)?,
    })
}

/// Why the host path, the upper and the work directory hold each other,
/// or `None`. The kernel refuses a host path in a layer (`ELOOP`), but
/// takes an upper or a work directory in the host path with a warning
/// (with no `index`), and the writes of the payload then reach the host
/// path. The paths are compared with each link resolved.
fn overlap(host: &str, layers: &OverlayLayers) -> Option<String> {
    let real = |p: &str| std::fs::canonicalize(p).ok();
    let named = [
        ("the host path", real(host)),
        ("the upper directory", real(&layers.upper)),
        ("the work directory", real(&layers.work)),
    ];
    for (a, pa) in &named {
        for (b, pb) in &named {
            if let (Some(x), Some(y)) = (pa, pb)
                && a != b
                && x.starts_with(y)
            {
                return Some(format!("has layers that overlap: {a} is at or under {b}"));
            }
        }
    }
    None
}

/// The step of a refused overlayfs mount.
pub fn mount_step(e: Errno) -> u8 {
    if e == sys::ELOOP || e == sys::EBUSY { NS_LAYER_OVERLAP_STEP } else { NS_LAYER_MOUNT_STEP }
}

/// In the child: the overlay on the mount point `point`, with the lower
/// layer `lower` (a descriptor of a directory). No allocation.
pub(crate) fn mount(layer: &Layer, lower: i64, point: i64, userns: bool) -> Result<(), (u8, Errno)> {
    // `lowerdir=.`: the working directory is the lower descriptor.
    sys::fchdir(lower).map_err(|e| (NS_LAYER_LOWER_STEP, e))?;
    let data = layer.options(userns).ptr();
    crate::ns_dev_bind::mount_data_on_fd(&layer.fstype, point, &layer.fstype, 0, data)
        .map(drop)
        .map_err(|e| (mount_step(e), e))
}

/// The class and the words of a refused step of an overlay volume, or
/// `None` where the step is not one.
pub(crate) fn failure(label: &str, step: u8, e: Errno) -> Option<(Class, String)> {
    let words = step_name(step)?;
    let errno = format!("{} ({})", e.name(), e.0);
    Some(match step {
        NS_LAYER_OVERLAP_STEP => (
            Class::Usage,
            format!(
                "the volume {label} is refused: {words} ({errno}); the host path, the upper and the work directory \
                 must not hold each other, and no other mount may use the upper or the work directory"
            ),
        ),
        NS_LAYER_MOUNT_STEP if e == sys::EINVAL || e == sys::EPERM => (
            crate::errno_class(e),
            format!(
                "the volume {label} could not be mounted: {words} failed with {errno}; in a user namespace the \
                 kernel needs Linux 5.11 for overlayfs, and the file system of the upper directory must take \
                 user.* attributes"
            ),
        ),
        _ => (crate::errno_class(e), format!("the volume {label} could not be mounted: {words} failed with {errno}")),
    })
}

#[cfg(test)]
mod tests;
