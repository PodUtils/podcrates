//! Where an extraction writes: the root file system, its sidecar and its
//! completion marker. The caller chooses the directory.

use std::path::{Path, PathBuf};

use crate::error::{self, Result};
use crate::{remove, safety};

/// The name of the root file system in [`Place::under`].
pub const ROOTFS_NAME: &str = "rootfs";

/// The name of the sidecar in [`Place::under`].
pub const SIDECAR_NAME: &str = ".meta.jsonl";

/// The name of the completion marker in [`Place::under`]. The crate writes it
/// last, after each other byte.
///
/// The extraction of a hostile layer is refused partway, and it can leave a
/// directory and a sidecar. A check of only those two then tells the next
/// call that the image is extracted, and gives it a half tree with the
/// symlink of the attacker in it. A `SIGKILL` between two entries leaves the
/// same directory and sidecar as a success. Only a marker that is written
/// after the last byte tells a finished extraction from an interrupted one.
pub const DONE_MARKER: &str = ".extracted";

/// The three paths of one extraction.
///
/// The sidecar is beside the root file system, not in it: the payload can
/// read and write a file in the root file system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub rootfs: PathBuf,
    pub sidecar: PathBuf,
    pub marker: PathBuf,
}

impl Place {
    /// `base/rootfs`, `base/.meta.jsonl` and `base/.extracted`.
    pub fn under(base: &Path) -> Place {
        Place { rootfs: base.join(ROOTFS_NAME), sidecar: base.join(SIDECAR_NAME), marker: base.join(DONE_MARKER) }
    }

    /// Is the extraction complete? The marker tells, not the directory
    /// ([`DONE_MARKER`]).
    pub fn is_extracted(&self) -> bool {
        self.rootfs.is_dir() && self.marker.is_file()
    }

    /// Remove the marker first, then the root file system and the sidecar.
    /// An interrupted removal then never reads as extracted. True when
    /// something was removed.
    pub fn remove(&self) -> Result<bool> {
        let marker = remove_file(&self.marker)?;
        let rootfs = remove_tree(&self.rootfs)?;
        let sidecar = remove_file(&self.sidecar)?;
        Ok(marker || rootfs || sidecar)
    }
}

fn remove_file(path: &Path) -> Result<bool> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(error::io(e)),
    }
}

/// Remove `path` and each entry under it, and never follow a symlink out of
/// it. `std::fs::remove_dir_all` is not used: a hostile archive wrote this
/// tree. True when something was removed.
pub fn remove_tree(path: &Path) -> Result<bool> {
    let Some(parent) = path.parent() else {
        return Ok(false);
    };
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return Ok(false);
    };
    if std::fs::symlink_metadata(path).is_err() {
        return Ok(false);
    }
    let d = safety::Dir::open(&parent.to_string_lossy())?;
    remove::remove_at(d.fd(), name)
}
