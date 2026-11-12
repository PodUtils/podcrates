//! The staging of a root file system for the directory rungs.
//!
//! The run directory, the cache and the tmpfs rungs enter a root file
//! system in a place other than the extraction of the store: a private
//! directory for one run, a persistent cache directory, or a tmpfs mount.
//! This module holds the one operation of the three: a copy of the
//! extracted tree. The mount stays with its caller, beside the probe
//! result that admitted it.
//!
//! A copy, not an extraction. The extracted tree has no whiteouts left,
//! and its ownership is in a sidecar, so this copies what is there:
//! directories, regular files and symbolic links. Each other kind (a fifo,
//! a socket, a device node) is refused with its path: a silent skip would
//! run a payload from a tree that is not the tree of the image.

use std::path::{Path, PathBuf};

use crate::Names;

/// What `copy_tree` refused on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The path that could not be replicated, as given.
    pub path: PathBuf,
    /// Why, in one sentence.
    pub why: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.why)
    }
}

/// Replicate the tree at `src` under `dst`, returning the entries copied.
///
/// `dst` is created with its parents; a `dst` that already exists is
/// refused rather than merged into, so a stale staging directory can
/// never donate files to a run. Symlinks replicate as symlinks, with
/// their targets unrewritten: the tree means the same under the new
/// root. Ownership is not applied: the caller runs without the ids the
/// image intended, like every other reader of the extracted tree.
pub fn copy_tree(src: &Path, dst: &Path) -> Result<usize, Refusal> {
    if !src.is_dir() {
        return Err(Refusal { path: src.to_path_buf(), why: "not a directory to stage from".to_string() });
    }
    if dst.exists() {
        return Err(Refusal {
            path: dst.to_path_buf(),
            why: "the staging directory already exists: refusing rather than merging into a stale tree".to_string(),
        });
    }
    std::fs::create_dir_all(dst).map_err(|e| Refusal {
        path: dst.to_path_buf(),
        why: format!("the staging directory could not be created: {e}"),
    })?;
    copy_contents(src, dst)
}

/// Replicate the entries of `src` into the existing directory `dst`,
/// returning the entries copied. The caller owns `dst`'s provenance: a
/// just-created directory, or a just-mounted tmpfs verified empty.
fn copy_contents(src: &Path, dst: &Path) -> Result<usize, Refusal> {
    let mut copied = 0usize;
    let mut dirs = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((sdir, ddir)) = dirs.pop() {
        let entries = std::fs::read_dir(&sdir)
            .map_err(|e| Refusal { path: sdir.clone(), why: format!("the directory could not be read: {e}") })?;
        for entry in entries {
            let entry = entry.map_err(|e| Refusal {
                path: sdir.clone(),
                why: format!("a directory entry could not be read: {e}"),
            })?;
            let spath = entry.path();
            let dpath = ddir.join(entry.file_name());
            let kind = entry
                .file_type()
                .map_err(|e| Refusal { path: spath.clone(), why: format!("the file type could not be read: {e}") })?;
            if kind.is_dir() {
                std::fs::create_dir(&dpath).map_err(|e| Refusal {
                    path: dpath.clone(),
                    why: format!("the directory could not be created: {e}"),
                })?;
                copied += 1;
                dirs.push((spath, dpath));
            } else if kind.is_file() {
                std::fs::copy(&spath, &dpath)
                    .map_err(|e| Refusal { path: dpath.clone(), why: format!("the file could not be copied: {e}") })?;
                copied += 1;
            } else if kind.is_symlink() {
                let target = std::fs::read_link(&spath).map_err(|e| Refusal {
                    path: spath.clone(),
                    why: format!("the link target could not be read: {e}"),
                })?;
                std::os::unix::fs::symlink(&target, &dpath).map_err(|e| Refusal {
                    path: dpath.clone(),
                    why: format!("the link could not be recreated: {e}"),
                })?;
                copied += 1;
            } else {
                return Err(Refusal {
                    path: spath,
                    why:
                        "not a file, directory or symlink: refusing rather than staging a tree that is not the image's"
                            .to_string(),
                });
            }
        }
    }
    Ok(copied)
}

/// Stage an ephemeral per-run root on a tmpfs mount under the store.
///
/// A unique directory under `store/tmpfs/` is created, a tmpfs with kernel
/// defaults is mounted on it (flags 0, no data: the kernel sizes it, so no
/// invented size rides here), and the extracted tree is replicated onto
/// the mount. Any failure unmounts where mounted and removes the
/// directory, refusing with the reason: a half-staged tmpfs must never
/// survive to meet the next run. The caller unmounts and removes with
/// [`release_tmpfs`] when the payload exits, on success and on failure
/// alike.
pub fn stage_tmpfs(store_root: &Path, rootfs: &Path) -> Result<PathBuf, Refusal> {
    use pod_hostprobe::sys::{CBuf, mount, umount_detach};
    if !rootfs.is_dir() {
        return Err(Refusal { path: rootfs.to_path_buf(), why: "not a directory to stage from".to_string() });
    }
    let base = store_root.join("tmpfs");
    std::fs::create_dir_all(&base)
        .map_err(|e| Refusal { path: base.clone(), why: format!("the tmpfs base could not be prepared: {e}") })?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = base.join(format!("mnt-{}-{nanos}", std::process::id()));
    if dir.exists() {
        return Err(Refusal {
            path: dir,
            why: "the staging directory already exists: refusing rather than merging into a stale tree".to_string(),
        });
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| Refusal { path: dir.clone(), why: format!("the staging directory could not be created: {e}") })?;
    let target = CBuf::new(&dir.to_string_lossy()).ok_or_else(|| Refusal {
        path: dir.clone(),
        why: "the staging path cannot be handed to the kernel".to_string(),
    })?;
    let source = CBuf::new("tmpfs").expect("a literal carries no NUL");
    let fstype = CBuf::new("tmpfs").expect("a literal carries no NUL");
    if let Err(e) = mount(&source, &target, &fstype, 0) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(Refusal {
            path: dir,
            why: format!(
                "the tmpfs mount was refused ({}): the ephemeral rung needs a mount this runtime does not grant",
                e.name()
            ),
        });
    }
    // The mount needs a directory to land on, and `copy_tree` refuses an
    // existing one so a stale tree never donates files: the fresh mount
    // is verified empty first, which is the same invariant by a shorter
    // route (a just-mounted tmpfs is empty unless something raced it).
    let empty = std::fs::read_dir(&dir)
        .map_err(|e| Refusal { path: dir.clone(), why: format!("the fresh mount could not be read: {e}") })?;
    if empty.count() != 0 {
        let _ = umount_detach(&target);
        let _ = std::fs::remove_dir_all(&dir);
        return Err(Refusal {
            path: dir,
            why: "the fresh tmpfs mount is not empty: refusing rather than merging into a tree of unknown provenance"
                .to_string(),
        });
    }
    if let Err(r) = copy_contents(rootfs, &dir) {
        let _ = umount_detach(&target);
        let _ = std::fs::remove_dir_all(&dir);
        return Err(r);
    }
    Ok(dir)
}

/// Unmount the tmpfs at `dir` and remove the staging directory.
///
/// Best-effort both halves: the unmount is what frees the pages, and the
/// removal is what keeps the next run from meeting this one. A directory
/// that was never mounted removes like any other. The unmount detaches:
/// a process that still holds a descriptor in the tmpfs (a child of the
/// payload, or a fork of another thread of the caller) makes a plain
/// unmount fail with `EBUSY`, and the mount and its directory would stay.
pub fn release_tmpfs(dir: &Path) {
    use pod_hostprobe::sys::{CBuf, umount_detach};
    if let Some(target) = CBuf::new(&dir.to_string_lossy()) {
        let _ = umount_detach(&target);
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// The cache directory for an image digest under the store.
///
/// The digest's `:` spells `_`, the same spelling the extraction paths
/// use, so one digest names one directory everywhere.
pub fn cache_dir(store_root: &Path, digest: &str) -> PathBuf {
    store_root.join("cache").join(digest.replace(':', "_"))
}

/// Stage a persistent root for one image under the cache base of the
/// store: copy one time, and use it again while the marker file
/// (`names.cache_marker`) names the digest.
///
/// A marker that names another digest, or no marker at all, rebuilds
/// rather than entering a tree of unknown provenance: a cache hit is a
/// claim about bytes, and the marker is what backs it.
pub fn stage_cache(names: &Names, store_root: &Path, digest: &str, rootfs: &Path) -> Result<PathBuf, Refusal> {
    let base = store_root.join("cache");
    std::fs::create_dir_all(&base)
        .map_err(|e| Refusal { path: base.clone(), why: format!("the cache base could not be prepared: {e}") })?;
    let dir = cache_dir(store_root, digest);
    let marker = dir.join(names.cache_marker);
    if dir.is_dir() {
        match std::fs::read_to_string(&marker) {
            Ok(text) if text.trim() == digest => return Ok(dir),
            _ => {
                std::fs::remove_dir_all(&dir).map_err(|e| Refusal {
                    path: dir.clone(),
                    why: format!("the cache names another tree and could not be cleared: {e}"),
                })?;
            }
        }
    }
    if dir.exists() {
        return Err(Refusal {
            path: dir,
            why: "the cache path exists and is not a directory: refusing rather than merging into it".to_string(),
        });
    }
    copy_tree(rootfs, &dir)?;
    std::fs::write(&marker, digest)
        .map_err(|e| Refusal { path: marker, why: format!("the cache marker could not be written: {e}") })?;
    Ok(dir)
}

#[cfg(test)]
mod tests;
