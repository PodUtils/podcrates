//! The view of the guest root for the checks before the fork: the volumes
//! of the plan over the image, and, on an overlay root with an upper
//! directory of the caller (`Upper::Dir`), the upper layer over the lower
//! one, as overlayfs shows them to the payload.
//!
//! The rules of overlayfs, as `pod-rootfs` reads a root of two layers: an
//! entry of the upper layer hides the same name of the lower layer; a
//! whiteout (a character device 0:0) hides the name; a directory of the
//! upper layer whose opaque attribute is `y` hides each entry of the lower
//! layer under it. The attribute is the one that the mount reads:
//! `trusted.overlay.opaque` for a mount of a host root,
//! `user.overlay.opaque` for a mount in a user namespace, which has
//! `userxattr` (`ns_overlay`). A mount in a user namespace with no
//! `userxattr` reads neither (kernel 6.18). A volume hides both
//! layers at its guest path, as the child binds it over the merged root.
//! `Upper::Tmpfs` has no upper directory here: its upper layer starts
//! empty, so the lower layer is the view. An overlay volume
//! (`VolumeBind::overlay`, PodUtils/podvm#689) shows its upper directory
//! over its host path by the same rules.

use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use pod_hostprobe::sys::{self, CBuf};

use crate::abi::GuestRoot;
use crate::userland::Maps;
use crate::{Plan, RootDir};

/// The upper directory of an overlay root, where it has one, and whether
/// the overlay mounts (the root and each overlay volume) have `userxattr`:
/// a mount then reads `user.overlay.opaque`, else `trusted.overlay.opaque`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UpperDir<'a> {
    pub(crate) path: Option<&'a str>,
    pub(crate) userxattr: bool,
}

impl UpperDir<'_> {
    /// The attribute of an opaque directory that the mount reads.
    fn opaque(&self) -> &'static str {
        if self.userxattr { "user.overlay.opaque" } else { "trusted.overlay.opaque" }
    }
}

/// The guest root of the checks before the fork.
pub(crate) struct View {
    /// The image (the lower layer) and the volumes.
    maps: Maps,
    /// The upper directory of an overlay root.
    upper: Option<PathBuf>,
    /// The upper directory of each overlay volume, in the order of the
    /// volumes of the plan; `None` for a bind.
    layers: Vec<Option<PathBuf>>,
    /// The attribute of an opaque directory that the mounts read.
    opaque: &'static str,
}

impl View {
    /// The view of `root` with the volumes of `plan` and the upper
    /// directory `upper`, where the root is an overlay with one.
    pub(crate) fn new(root: &RootDir, plan: &Plan, upper: Option<UpperDir<'_>>) -> View {
        let layers = plan.volumes.iter().map(|v| v.overlay.as_ref().map(|l| PathBuf::from(&l.upper))).collect();
        View {
            maps: Maps::of_binds(&root.path, &plan.volumes),
            upper: upper.and_then(|u| u.path).map(PathBuf::from),
            layers,
            opaque: upper.map_or("trusted.overlay.opaque", |u| u.opaque()),
        }
    }

    /// The host path that the two layers show at `parts` (relative to the
    /// root of the layers), or `None` where they show nothing: no entry, a
    /// whiteout, or an entry of the lower layer under an opaque directory.
    fn merged(&self, upper: &Path, lower: &Path, parts: &[String]) -> Option<PathBuf> {
        let lower = lower.to_path_buf();
        let Some((last, dirs)) = parts.split_last() else { return Some(lower) };
        let opaque = |dir: &Path| opaque(dir, self.opaque);
        let mut layers = (Some(upper.to_path_buf()), (!opaque(upper)).then_some(lower));
        for name in dirs {
            let (path, m, in_upper) = entry(&layers, name)?;
            if !m.is_dir() {
                return None;
            }
            layers = if in_upper {
                let dir = |l: &PathBuf| std::fs::symlink_metadata(l).is_ok_and(|m| m.is_dir());
                let below = layers.1.map(|l| l.join(name)).filter(|l| !opaque(&path) && dir(l));
                (Some(path), below)
            } else {
                (None, Some(path))
            };
        }
        entry(&layers, last).map(|(path, _, _)| path)
    }

    /// Where an upper directory takes part in the view of `parts`: the
    /// upper and the lower directory, and the rest of `parts` under them.
    /// An overlay volume for a path in it; the overlay root for a path that
    /// no volume serves.
    fn layered<'p>(&self, parts: &'p [String]) -> Option<(&Path, PathBuf, &'p [String])> {
        match self.maps.volume_of(parts) {
            Some((at, depth)) => {
                let upper = self.layers.get(at)?.as_deref()?;
                Some((upper, PathBuf::from(&self.maps.volumes()[at].host), &parts[depth..]))
            }
            None => self.upper.as_deref().map(|u| (u, self.maps.root().to_path_buf(), parts)),
        }
    }
}

/// A whiteout of overlayfs: a character device 0:0.
fn whiteout(m: &std::fs::Metadata) -> bool {
    m.file_type().is_char_device() && m.rdev() == 0
}

/// Is the directory `dir` opaque by the attribute `attr`? A directory that
/// does not open (no read permission) is not.
fn opaque(dir: &Path, attr: &str) -> bool {
    let Some(c) = CBuf::from_bytes(dir.as_os_str().as_bytes()) else { return false };
    let Ok(fd) = sys::open(&c, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC, 0) else {
        return false;
    };
    let mut b = [0u8; 2];
    let set = CBuf::new(attr).is_some_and(|c| sys::fgetxattr(fd, &c, &mut b) == Ok(1) && b[0] == b'y');
    let _ = sys::close(fd);
    set
}

/// The entry `name` of the merged directory of `layers` (its upper and its
/// lower directory, each where it takes part): its path, its metadata, and
/// whether the upper layer holds it.
fn entry(layers: &(Option<PathBuf>, Option<PathBuf>), name: &str) -> Option<(PathBuf, std::fs::Metadata, bool)> {
    let read = |dir: &Option<PathBuf>| {
        let p = dir.as_ref()?.join(name);
        std::fs::symlink_metadata(&p).ok().map(|m| (p, m))
    };
    if let Some((p, m)) = read(&layers.0) {
        return (!whiteout(&m)).then_some((p, m, true));
    }
    read(&layers.1).filter(|(_, m)| !whiteout(m)).map(|(p, m)| (p, m, false))
}

impl GuestRoot for View {
    fn root(&self) -> &Path {
        self.maps.root()
    }

    fn host_of(&self, parts: &[String]) -> (PathBuf, bool) {
        match self.layered(parts) {
            // A path that the layers do not show is no host path: `hidden`
            // says so to the walk. The root of an overlay volume is its
            // mount, as the root of a bind is.
            Some((upper, lower, rest)) => {
                let shown = self.merged(upper, &lower, rest).unwrap_or_else(|| upper.join(rest.join("/")));
                (shown, rest.is_empty() && rest.len() < parts.len())
            }
            None => self.maps.host_of(parts),
        }
    }

    fn hidden(&self, parts: &[String]) -> bool {
        self.layered(parts).is_some_and(|(upper, lower, rest)| self.merged(upper, &lower, rest).is_none())
    }

    fn above_mount(&self, parts: &[String]) -> bool {
        self.maps.above_mount(parts)
    }

    fn volume_label(&self, guest: &str) -> Option<String> {
        self.maps.volume_label(guest)
    }
}

#[cfg(test)]
mod tests;
