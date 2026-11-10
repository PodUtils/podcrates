//! Applying a layer's whiteouts to the accumulated tree, **before** that
//! layer's own entries are extracted.
//!
//! ⭐ `TODO/extract.md` T-0307, and the ordering is read out of the corpus
//! rather than guessed:
//! `references/indigo-dc__udocker/tree/udocker/container/structure.py:279`
//! applies layer N's whiteouts to the accumulated tree before extracting N.
//! That is the order that makes a layer able to both delete a path and recreate
//! it; the opposite order deletes what the same layer just wrote.
//!
//! ⚠ **This costs a second pass over the layer stream**, because a tar is a
//! stream and the whiteouts are scattered through it. The alternative is
//! holding a decompressed layer in memory, which for a distro base image is
//! hundreds of megabytes. Two passes over a blob already on disk is the cheaper
//! wrong-shaped thing, and it is stated here rather than discovered by whoever
//! profiles it.

use std::io::Read;

use podvm_probe::sys;

use crate::error::{Error, Result};
use crate::safety::{self, Dir};
use crate::whiteout::{self, Kind};

/// One thing a layer says to delete before it writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// `.wh.<name>` in some directory: remove that one name.
    Remove { dir: Vec<String>, name: String },
    /// `.wh..wh..opq` in some directory: remove that directory's CONTENTS,
    /// keeping the directory.
    Opaque { dir: Vec<String> },
}

/// First pass: read the layer and collect its whiteouts, extracting nothing.
pub fn collect(stream: &mut dyn Read) -> Result<Vec<Op>> {
    let mut ops = Vec::new();
    let mut ar = tar::Archive::new(stream);
    for entry in ar.entries()? {
        let entry = entry?;
        let path = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        // ⛔ The basename, never a path glob. T-0303, and
        // `experiments/70-whiteout-contract.sh` check D is the measurement that
        // a glob misses a whiteout at the layer root.
        match whiteout::classify(&path) {
            Kind::Normal => {}
            Kind::Remove(name) => ops.push(Op::Remove {
                dir: whiteout::dirname_parts(&path)
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
                name,
            }),
            Kind::Opaque => ops.push(Op::Opaque {
                dir: whiteout::dirname_parts(&path)
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
            }),
        }
    }
    Ok(ops)
}

/// Second half: apply them.
///
/// ⚠ A whiteout naming something that is not there is **not an error**. Layers
/// are built independently and a layer may whiteout a path an earlier layer
/// never created; refusing there would refuse ordinary images.
pub fn apply(root: &Dir, ops: &[Op]) -> Result<u64> {
    let mut removed = 0;
    for op in ops {
        match op {
            Op::Remove { dir, name } => {
                let mut parts: Vec<&str> = dir.iter().map(|s| s.as_str()).collect();
                parts.push(name);
                // ⛔ `false`: never create a directory on the way to deleting
                // something. A whiteout for a path that does not exist would
                // otherwise MAKE its parents, which is a layer's delete
                // creating directories.
                let Ok(Ok((parent, last))) = safety::open_parent(root, &parts, false) else {
                    continue;
                };
                if remove_at(parent.fd(), &last)? {
                    removed += 1;
                }
            }
            Op::Opaque { dir } => {
                let parts: Vec<&str> = dir.iter().map(|s| s.as_str()).collect();
                let d = if parts.is_empty() {
                    // The layer root itself. ⚠ Legal, and it means "this layer
                    // replaces the whole tree".
                    match reopen(root) {
                        Ok(d) => d,
                        Err(_) => continue,
                    }
                } else {
                    let Ok(Ok((parent, last))) = safety::open_parent(root, &parts, false) else {
                        continue;
                    };
                    let Ok(n) = cbuf(&last) else { continue };
                    match sys::openat(
                        parent.fd(),
                        &n,
                        sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC | sys::O_NOFOLLOW,
                        0,
                    ) {
                        Ok(fd) => DirFd(fd),
                        Err(_) => continue,
                    }
                };
                // ⛔ The CONTENTS, not the directory.
                // `references/indigo-dc__udocker/tree/udocker/container/structure.py:249-256`
                // lists the directory and removes each entry, keeping the
                // directory itself, and an opaque marker that deleted its own
                // directory would delete the mount point the next layer writes
                // into.
                for e in sys::getdents64(d.0).unwrap_or_default() {
                    let name = String::from_utf8_lossy(&e.name).into_owned();
                    if remove_at(d.0, &name)? {
                        removed += 1;
                    }
                }
            }
        }
    }
    Ok(removed)
}

struct DirFd(i64);

impl Drop for DirFd {
    fn drop(&mut self) {
        let _ = sys::close(self.0);
    }
}

fn reopen(root: &Dir) -> Result<DirFd> {
    let c = cbuf(".")?;
    let fd = sys::openat(
        root.fd(),
        &c,
        sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC,
        0,
    )
    .map_err(|e| Error::Extract(format!("cannot re-open the destination: {}", e.name())))?;
    Ok(DirFd(fd))
}

fn cbuf(s: &str) -> Result<sys::CBuf> {
    sys::CBuf::new(s)
        .ok_or_else(|| Error::Extract(format!("{s:?} contains a NUL and cannot reach the kernel")))
}

/// Remove one name below `parent`, recursing where it is a directory.
///
/// ⛔ Never follows a symlink. A symlink is removed as a symlink; the tree it
/// points at is not touched, which is the difference between deleting a link to
/// `/etc` and deleting `/etc`.
///
/// Returns whether something was actually removed, so "not there" and "removed"
/// are distinguishable by the caller.
pub fn remove_at(parent: i64, name: &str) -> Result<bool> {
    let n = cbuf(name)?;
    match sys::unlinkat(parent, &n, 0) {
        Ok(_) => return Ok(true),
        Err(e) if e == sys::ENOENT => return Ok(false),
        // EISDIR on some kernels, EPERM on others: it is a directory.
        Err(e) if e.0 == 21 || e == sys::EPERM => {}
        Err(e) => {
            return Err(Error::Extract(format!(
                "cannot remove {name:?}: {}",
                e.name()
            )))
        }
    }
    // A directory. Empty it, then remove it.
    let fd = match sys::openat(
        parent,
        &n,
        sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC | sys::O_NOFOLLOW,
        0,
    ) {
        Ok(fd) => DirFd(fd),
        Err(_) => return Ok(false),
    };
    for e in sys::getdents64(fd.0).unwrap_or_default() {
        let child = String::from_utf8_lossy(&e.name).into_owned();
        remove_at(fd.0, &child)?;
    }
    drop(fd);
    match sys::unlinkat(parent, &n, sys::AT_REMOVEDIR) {
        Ok(_) => Ok(true),
        Err(e) if e == sys::ENOENT => Ok(false),
        Err(e) => Err(Error::Extract(format!(
            "cannot remove the directory {name:?}: {}",
            e.name()
        ))),
    }
}
