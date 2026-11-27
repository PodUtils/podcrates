//! The tmpfs mounts of a plan on the namespace rung ([`crate::Plan::tmpfs`],
//! PodUtils/podvm#667): a directory in memory with a byte limit at a guest
//! path (`--tmpfs PATH:size=N`), as root and through a user namespace.
//!
//! Before the fork, the entry checks each mount: the guest path is an
//! absolute normal path that is not `/`, no other tmpfs gives it, and it
//! is not at or under the guest path of a volume (a volume binds after the
//! tmpfs mounts and would hide it, and a mount point under a volume would
//! be made in the host path). The limit is a [`std::num::NonZeroU64`]: a
//! tmpfs with `size=0` has no limit. A mode over `0o7777` is refused. A
//! refusal names the mount, before the banner and the fork.
//!
//! In the child, after the mounts of the rung and before the volumes: the
//! walk of the volumes makes the mount point under the root path, with no
//! symbolic link (`ns_volume::walk`), then `mount("tmpfs",
//! /proc/self/fd/N, "tmpfs", MS_NOSUID | MS_NODEV, "size=N[,mode=M]")`,
//! with the mode in octal where the plan gives one. A new walk
//! crosses into the mount, and `fstatfs` reads it back: a file system that
//! is not a tmpfs, or a size that is not the limit in whole blocks, stops
//! the payload at [`NS_TMPFS_LIMIT_STEP`]. The kernel then did not enforce
//! the limit, and the payload never runs with no limit.
//!
//! A refused step stops the payload with its step byte and the index of the
//! mount, and the parent names the mount. No fallback to chroot: chroot has
//! no mount namespace. A join of a run (`ns_join`) mounts nothing: the mount
//! namespace of the run holds each mount with its limit.

use pod_core::Class;
use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::ns_volume::{NS_VOLUME_LINK_STEP, guest_parts, volume_label};
use crate::{Error, Plan, Result, TmpfsMount, errno_class};

/// The step byte of the mount point of a tmpfs (a component that does not
/// open, or that the child cannot make).
pub const NS_TMPFS_POINT_STEP: u8 = 49;
/// The step byte of a guest path of a tmpfs through a symbolic link of the
/// image.
pub const NS_TMPFS_LINK_STEP: u8 = 50;
/// The step byte of the mount of a tmpfs with its limit.
pub const NS_TMPFS_MOUNT_STEP: u8 = 51;
/// The step byte of the check of the limit after the mount: the errno of
/// `fstatfs`, or 0 where the mount has no such limit.
pub const NS_TMPFS_LIMIT_STEP: u8 = 52;

/// The most tmpfs mounts of a plan: the child names a refused one by its
/// index in one byte.
pub const MOST: usize = 255;

/// The magic number of a tmpfs (`TMPFS_MAGIC`, `include/uapi/linux/magic.h`).
const TMPFS_MAGIC: u64 = 0x0102_1994;

/// A tmpfs as `GUEST:size=N`, or `GUEST:mode=M,size=N` with a mode, the
/// words of podvm's `--tmpfs`, for a refusal.
pub fn tmpfs_label(t: &TmpfsMount) -> String {
    match t.mode {
        None => format!("{}:size={}", t.guest, t.bytes),
        Some(m) => format!("{}:mode={m:o},size={}", t.guest, t.bytes),
    }
}

/// The highest mode of a tmpfs root: the permission bits, set-user-ID,
/// set-group-ID and the sticky bit.
pub const MODE_MOST: u32 = 0o7777;

/// The options of the mount of a tmpfs: `size=N`, and `mode=M` in octal
/// where the plan gives a mode.
pub fn tmpfs_options(bytes: u64, mode: Option<u32>) -> String {
    match mode {
        None => format!("size={bytes}"),
        Some(m) => format!("size={bytes},mode={m:o}"),
    }
}

/// The words of a step of a tmpfs, or `None` where the step is not one.
pub(crate) fn step_name(step: u8) -> Option<&'static str> {
    match step {
        NS_TMPFS_POINT_STEP => Some("opening the mount point of a tmpfs in the image"),
        NS_TMPFS_LINK_STEP => Some("the guest path of a tmpfs crosses a symbolic link of the image"),
        NS_TMPFS_MOUNT_STEP => Some("mounting a tmpfs with a byte limit"),
        NS_TMPFS_LIMIT_STEP => Some("reading back the byte limit of a tmpfs"),
        _ => None,
    }
}

/// Does the mount that `fstatfs` read hold the limit `bytes`: a tmpfs of
/// `bytes` rounded up to whole blocks? A tmpfs with no limit has no blocks,
/// and a tmpfs with the default limit has half of the memory.
pub fn tmpfs_enforced(fs: &sys::Statfs, bytes: u64) -> bool {
    let Ok(block) = u64::try_from(fs.f_bsize) else { return false };
    fs.f_type == TMPFS_MAGIC && block != 0 && fs.f_blocks == bytes.div_ceil(block)
}

/// One tmpfs, built before the fork.
struct Prepared {
    /// The components of the guest path, in order; at least one.
    parts: Vec<CBuf>,
    /// `size=N`, the options of the mount.
    data: CBuf,
    bytes: u64,
}

/// The tmpfs mounts of a plan for the child, built before the fork.
#[derive(Default)]
pub(crate) struct Tmpfs {
    list: Vec<Prepared>,
    fstype: Option<CBuf>,
}

impl Tmpfs {
    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

/// Check the tmpfs mounts of `plan` and build them for the child. The
/// caller refused them already on an entry with no mount namespace of its
/// own (`ns_volume::prepare`). Before the banner and the fork.
pub(crate) fn prepare(plan: &Plan) -> Result<Tmpfs> {
    let mut out = Tmpfs::default();
    if plan.tmpfs.is_empty() {
        return Ok(out);
    }
    if plan.tmpfs.len() > MOST {
        return Err(Error::runtime(
            Class::Usage,
            format!("{} tmpfs mounts; a plan takes at most {MOST}", plan.tmpfs.len()),
        ));
    }
    let volumes: Vec<(Vec<&str>, &crate::VolumeBind)> =
        plan.volumes.iter().filter_map(|v| guest_parts(&v.guest).ok().map(|p| (p, v))).collect();
    let mut seen: Vec<Vec<&str>> = Vec::new();
    for t in &plan.tmpfs {
        let usage = |why: &str| Error::runtime(Class::Usage, format!("the tmpfs {} {why}", tmpfs_label(t)));
        let parts = guest_parts(&t.guest).map_err(|why| usage(&format!("has a guest path that {why}")))?;
        if seen.contains(&parts) {
            return Err(usage("repeats the guest path of another tmpfs"));
        }
        if let Some((_, v)) = volumes.iter().find(|(v, _)| parts.starts_with(v)) {
            return Err(usage(&format!(
                "is at or under the guest path of the volume {}: the volume would hide it, or its mount point would be made in the host path",
                volume_label(v)
            )));
        }
        if let Some(m) = t.mode.filter(|m| *m > MODE_MOST) {
            return Err(usage(&format!("has the mode {m:o}, over the highest mode {MODE_MOST:o}")));
        }
        let nul = || usage("has a NUL byte");
        let parts_c = parts.iter().map(|p| CBuf::new(p).ok_or_else(nul)).collect::<Result<Vec<_>>>()?;
        let bytes = t.bytes.get();
        let data = CBuf::new(&tmpfs_options(bytes, t.mode)).ok_or_else(nul)?;
        out.list.push(Prepared { parts: parts_c, data, bytes });
        seen.push(parts);
    }
    out.fstype = CBuf::new("tmpfs");
    Ok(out)
}

/// The step of a refused walk: a link of the image, or the mount point.
fn walk_step(step: u8) -> u8 {
    if step == NS_VOLUME_LINK_STEP { NS_TMPFS_LINK_STEP } else { NS_TMPFS_POINT_STEP }
}

/// One tmpfs under `root`: the mount point, the mount, and the check of
/// its limit.
fn mount_one(root: i64, t: &Prepared, fstype: &CBuf) -> std::result::Result<(), (u8, Errno)> {
    let fd = crate::ns_volume::walk(root, &t.parts, true).map_err(|(step, e)| (walk_step(step), e))?;
    let flags = sys::MS_NOSUID | sys::MS_NODEV;
    let mounted = crate::ns_dev_bind::mount_data_on_fd(fstype, fd, fstype, flags, t.data.ptr());
    let _ = sys::close(fd);
    mounted.map_err(|e| (NS_TMPFS_MOUNT_STEP, e))?;
    // A new walk crosses into the mount: `fstatfs` reads the tmpfs.
    let top = crate::ns_volume::walk(root, &t.parts, true).map_err(|(_, e)| (NS_TMPFS_LIMIT_STEP, e))?;
    let read = sys::fstatfs(top);
    let _ = sys::close(top);
    match read {
        Ok(fs) if tmpfs_enforced(&fs, t.bytes) => Ok(()),
        Ok(_) => Err((NS_TMPFS_LIMIT_STEP, Errno(0))),
        Err(e) => Err((NS_TMPFS_LIMIT_STEP, e)),
    }
}

/// In the child, after the mounts of the rung and before the volumes: each
/// tmpfs in its order under the root descriptor `root`. `Err` gives the
/// step, the errno and the index of the tmpfs. No allocation.
pub(crate) fn mount_each(root: i64, t: &Tmpfs) -> std::result::Result<(), (u8, Errno, u8)> {
    let Some(fstype) = &t.fstype else { return Ok(()) };
    for (at, one) in t.list.iter().enumerate() {
        mount_one(root, one, fstype).map_err(|(step, e)| (step, e, at as u8))?;
    }
    Ok(())
}

/// The refusal of a tmpfs that the child reported, or `None` where the
/// step is not a step of a tmpfs.
pub(crate) fn failure(plan: &Plan, step: u8, e: Errno, at: u8) -> Option<Error> {
    let words = step_name(step)?;
    let label = plan.tmpfs.get(usize::from(at)).map_or_else(|| format!("number {at}"), tmpfs_label);
    let (class, text) = match step {
        NS_TMPFS_LINK_STEP => {
            (Class::Policy, format!("the tmpfs {label} is refused: {words} ({} ({}))", e.name(), e.0))
        }
        NS_TMPFS_LIMIT_STEP if e.0 == 0 => (
            Class::Unsupported,
            format!(
                "the tmpfs {label} is refused: the kernel did not enforce its byte limit ({words} found another size)"
            ),
        ),
        _ => (
            errno_class(e),
            format!("the tmpfs {label} could not be mounted: {words} failed with {} ({})", e.name(), e.0),
        ),
    };
    Some(Error::runtime(class, format!("{text}; the payload did not run")))
}

/// Is the guest path `guest` at or under a tmpfs of `plan`, and in no
/// volume? The child then makes it in the tmpfs: a working directory there
/// needs no directory of the image.
pub(crate) fn holds(plan: &Plan, guest: &str) -> bool {
    let Ok(parts) = guest_parts(guest) else { return false };
    let under = |g: &str| guest_parts(g).is_ok_and(|p| parts.starts_with(&p));
    plan.tmpfs.iter().any(|t| under(&t.guest)) && !plan.volumes.iter().any(|v| under(&v.guest))
}

#[cfg(test)]
mod tests;
