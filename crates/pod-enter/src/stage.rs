//! TODO/packaging.md T-1003: staging a rootfs for the directory rungs.
//!
//! The run-directory, cache and tmpfs rungs all enter a rootfs that lives
//! somewhere other than the store's own extraction: a private per-run
//! directory, a persistent cache directory, or a tmpfs mount. This module
//! owns the one operation all three share: replicating the extracted tree.
//! The mount itself lives with its caller, beside the probe verdict that
//! admitted it.
//!
//! ⛔ A replication, not an extraction. Whiteouts are already resolved in
//! the extracted tree and ownership is already recorded in the sidecar,
//! so this copies what is there: directories, regular files and symlinks.
//! Anything else (a fifo, a socket, a device node) refuses naming the
//! path rather than copying something the entry cannot enter: a silent
//! skip would run a payload from a tree that is not the image's.

use std::path::{Path, PathBuf};

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
        return Err(Refusal {
            path: src.to_path_buf(),
            why: "not a directory podvm can stage from".to_string(),
        });
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
        let entries = std::fs::read_dir(&sdir).map_err(|e| Refusal {
            path: sdir.clone(),
            why: format!("the directory could not be read: {e}"),
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| Refusal {
                path: sdir.clone(),
                why: format!("a directory entry could not be read: {e}"),
            })?;
            let spath = entry.path();
            let dpath = ddir.join(entry.file_name());
            let kind = entry.file_type().map_err(|e| Refusal {
                path: spath.clone(),
                why: format!("the file type could not be read: {e}"),
            })?;
            if kind.is_dir() {
                std::fs::create_dir(&dpath).map_err(|e| Refusal {
                    path: dpath.clone(),
                    why: format!("the directory could not be created: {e}"),
                })?;
                copied += 1;
                dirs.push((spath, dpath));
            } else if kind.is_file() {
                std::fs::copy(&spath, &dpath).map_err(|e| Refusal {
                    path: dpath.clone(),
                    why: format!("the file could not be copied: {e}"),
                })?;
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
                    why: "not a file, directory or symlink: refusing rather than staging a tree that is not the image's".to_string(),
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
    use podvm_probe::sys::{mount, umount, CBuf};
    if !rootfs.is_dir() {
        return Err(Refusal {
            path: rootfs.to_path_buf(),
            why: "not a directory podvm can stage from".to_string(),
        });
    }
    let base = store_root.join("tmpfs");
    std::fs::create_dir_all(&base).map_err(|e| Refusal {
        path: base.clone(),
        why: format!("the tmpfs base could not be prepared: {e}"),
    })?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = base.join(format!("mnt-{}-{nanos}", std::process::id()));
    if dir.exists() {
        return Err(Refusal {
            path: dir,
            why: "the staging directory already exists: refusing rather than merging into a stale tree".to_string(),
        });
    }
    std::fs::create_dir_all(&dir).map_err(|e| Refusal {
        path: dir.clone(),
        why: format!("the staging directory could not be created: {e}"),
    })?;
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
    let empty = std::fs::read_dir(&dir).map_err(|e| Refusal {
        path: dir.clone(),
        why: format!("the fresh mount could not be read: {e}"),
    })?;
    if empty.count() != 0 {
        let _ = umount(&target);
        let _ = std::fs::remove_dir_all(&dir);
        return Err(Refusal {
            path: dir,
            why: "the fresh tmpfs mount is not empty: refusing rather than merging into a tree of unknown provenance"
                .to_string(),
        });
    }
    if let Err(r) = copy_contents(rootfs, &dir) {
        let _ = umount(&target);
        let _ = std::fs::remove_dir_all(&dir);
        return Err(r);
    }
    Ok(dir)
}

/// Unmount the tmpfs at `dir` and remove the staging directory.
///
/// Best-effort both halves: the unmount is what frees the pages, and the
/// removal is what keeps the next run from meeting this one. A directory
/// that was never mounted removes like any other.
pub fn release_tmpfs(dir: &Path) {
    use podvm_probe::sys::{umount, CBuf};
    if let Some(target) = CBuf::new(&dir.to_string_lossy()) {
        let _ = umount(&target);
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

/// Stage a persistent per-image root under the store's cache base:
/// copy once, reuse while the completion marker names the digest.
///
/// A marker that names another digest, or no marker at all, rebuilds
/// rather than entering a tree of unknown provenance: a cache hit is a
/// claim about bytes, and the marker is what backs it.
pub fn stage_cache(store_root: &Path, digest: &str, rootfs: &Path) -> Result<PathBuf, Refusal> {
    let base = store_root.join("cache");
    std::fs::create_dir_all(&base).map_err(|e| Refusal {
        path: base.clone(),
        why: format!("the cache base could not be prepared: {e}"),
    })?;
    let dir = cache_dir(store_root, digest);
    let marker = dir.join(".podvm-cache-complete");
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
            why:
                "the cache path exists and is not a directory: refusing rather than merging into it"
                    .to_string(),
        });
    }
    copy_tree(rootfs, &dir)?;
    std::fs::write(&marker, digest).map_err(|e| Refusal {
        path: marker,
        why: format!("the cache marker could not be written: {e}"),
    })?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("podvm-stage-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src/bin")).unwrap();
        d
    }

    /// Files, directories and symlinks replicate whole, with link
    /// targets unrewritten.
    #[test]
    fn a_tree_replicates_whole() {
        let d = tree("whole");
        std::fs::write(d.join("src/bin/prog"), b"\x7fELF-tool").unwrap();
        std::fs::write(d.join("src/note.txt"), b"hi").unwrap();
        std::os::unix::fs::symlink("prog", d.join("src/bin/sh")).unwrap();
        std::os::unix::fs::symlink("/abs/target", d.join("src/abs")).unwrap();
        let dst = d.join("dst");
        let n = copy_tree(&d.join("src"), &dst).unwrap();
        assert_eq!(n, 5, "bin, prog, note.txt, sh, abs");
        assert_eq!(
            std::fs::read(dst.join("bin/prog")).unwrap(),
            b"\x7fELF-tool"
        );
        assert_eq!(
            std::fs::read_link(dst.join("bin/sh")).unwrap(),
            std::path::Path::new("prog")
        );
        assert_eq!(
            std::fs::read_link(dst.join("abs")).unwrap(),
            std::path::Path::new("/abs/target")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A fifo refuses naming the path: the staged tree would not be the
    /// image's if special files silently dropped out of it.
    #[test]
    #[cfg(unix)]
    fn a_special_file_is_a_sentence() {
        let d = tree("fifo");
        let fifo = d.join("src/pipe");
        let st = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs on the test machine");
        assert!(st.success(), "the fixture fifo could not be made");
        let e = copy_tree(&d.join("src"), &d.join("dst")).unwrap_err();
        assert_eq!(e.path, fifo, "{e}");
        assert!(e.why.contains("not a file, directory or symlink"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A staging directory that already exists refuses rather than
    /// merging: a stale tree must never donate files to a run.
    #[test]
    fn an_existing_destination_refuses() {
        let d = tree("exists");
        std::fs::create_dir_all(d.join("dst")).unwrap();
        let e = copy_tree(&d.join("src"), &d.join("dst")).unwrap_err();
        assert!(e.why.contains("already exists"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A source that is not a directory refuses.
    #[test]
    fn a_missing_source_refuses() {
        let d = tree("missing");
        let e = copy_tree(&d.join("nope"), &d.join("dst")).unwrap_err();
        assert!(e.why.contains("not a directory"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The digest's colon spells an underscore, like the extraction
    /// paths: one digest, one directory, everywhere.
    #[test]
    fn the_cache_dir_spells_the_digest() {
        let dir = cache_dir(Path::new("/store"), "sha256:abc123");
        assert_eq!(dir, Path::new("/store/cache/sha256_abc123"));
    }

    /// The first stage copies and marks; the second reuses the marked
    /// tree without copying again.
    #[test]
    fn the_cache_copies_once_then_reuses() {
        let d = tree("cache");
        std::fs::write(d.join("src/bin/prog"), b"\x7fELF").unwrap();
        let store = d.join("store");
        let first = stage_cache(&store, "sha256:one", &d.join("src")).unwrap();
        assert_eq!(std::fs::read(first.join("bin/prog")).unwrap(), b"\x7fELF");
        assert_eq!(
            std::fs::read_to_string(first.join(".podvm-cache-complete")).unwrap(),
            "sha256:one"
        );
        // A sentinel the second stage must not copy again.
        std::fs::write(d.join("src/bin/late"), b"late").unwrap();
        let second = stage_cache(&store, "sha256:one", &d.join("src")).unwrap();
        assert_eq!(first, second);
        assert!(!second.join("bin/late").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A marker naming another digest rebuilds rather than entering a
    /// tree of unknown provenance.
    #[test]
    fn a_foreign_marker_rebuilds() {
        let d = tree("foreign");
        std::fs::write(d.join("src/bin/prog"), b"\x7fELF").unwrap();
        let store = d.join("store");
        let first = stage_cache(&store, "sha256:one", &d.join("src")).unwrap();
        std::fs::write(first.join(".podvm-cache-complete"), "sha256:other").unwrap();
        let second = stage_cache(&store, "sha256:one", &d.join("src")).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            std::fs::read_to_string(second.join(".podvm-cache-complete")).unwrap(),
            "sha256:one"
        );
        assert_eq!(std::fs::read(second.join("bin/prog")).unwrap(), b"\x7fELF");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A source that is not a directory refuses before the store is
    /// touched: no rung stages what nothing resolved, and no rung
    /// litters the store refusing it.
    #[test]
    fn a_tmpfs_over_a_missing_source_refuses_before_touching_the_store() {
        let d = tree("tmpfs-missing");
        let store = d.join("store");
        let e = stage_tmpfs(&store, &d.join("nope")).unwrap_err();
        assert!(e.why.contains("not a directory"), "{e}");
        assert!(!store.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The tmpfs rung stages onto a mount where one holds, and refuses
    /// naming the mount where one does not. Either arm cleans behind
    /// itself: entry carries the payload bytes and releases to nothing,
    /// refusal leaves no staging directory.
    #[test]
    fn tmpfs_stages_onto_a_mount_or_refuses_naming_it() {
        let d = tree("tmpfs");
        std::fs::write(d.join("src/bin/prog"), b"\x7fELF").unwrap();
        let store = d.join("store");
        match stage_tmpfs(&store, &d.join("src")) {
            Ok(dir) => {
                assert_eq!(std::fs::read(dir.join("bin/prog")).unwrap(), b"\x7fELF");
                release_tmpfs(&dir);
                assert!(!dir.exists());
            }
            Err(r) => {
                assert!(r.why.contains("tmpfs mount was refused"), "{r}");
                let staged: Vec<_> = std::fs::read_dir(store.join("tmpfs"))
                    .map(|rd| rd.filter_map(|e| e.ok()).collect())
                    .unwrap_or_default();
                assert!(staged.is_empty(), "a refused mount left staging behind");
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Releasing an unmounted directory removes it: the removal half of
    /// the release does not depend on the unmount half.
    #[test]
    fn releasing_an_unmounted_directory_removes_it() {
        let d = tree("release");
        let dir = d.join("plain");
        std::fs::create_dir_all(&dir).unwrap();
        release_tmpfs(&dir);
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
