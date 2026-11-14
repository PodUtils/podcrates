//! The apply of the whiteouts of a layer to the tree of the layers before it,
//! **before** the entries of that layer.
//!
//! udocker applies the whiteouts of layer N before it extracts N. In that
//! order a layer can remove a path and make it again; the other order removes
//! what the same layer wrote.
//!
//! **This costs a second pass over the layer stream**: a tar is a stream, and
//! the whiteouts are anywhere in it. The other way keeps a decompressed layer
//! in memory, hundreds of megabytes for the base image of a distribution. Two
//! passes over a blob on disk cost less.

use std::io::Read;

use pod_hostprobe::sys;

use crate::error::{self, Result, cbuf, refuse};
use crate::safety::{self, Dir, Refusal};
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

/// First pass: read the layer and collect its whiteouts, and extract nothing.
/// `layer` names the layer in a refusal.
///
/// The path of a whiteout gets the lexical check of each other entry
/// ([`safety::components`]), and its name must be a name: `.wh..` and
/// `.wh...` name `.` and `..`. A whiteout of `..` at the root of a layer
/// names the directory above the destination, and its removal would go
/// there. Such a whiteout refuses the layer, as an entry does.
pub fn collect(layer: &str, stream: &mut dyn Read) -> Result<Vec<Op>> {
    let mut ops = Vec::new();
    let mut ar = tar::Archive::new(stream);
    for entry in ar.entries().map_err(error::io)? {
        let entry = entry.map_err(error::io)?;
        let path = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        // The basename, not a path glob: a glob does not find a whiteout at
        // the root of a layer.
        let kind = whiteout::classify(&path);
        if kind == Kind::Normal {
            continue;
        }
        let mut dir: Vec<String> = match safety::components(&path) {
            Ok(parts) => parts.iter().map(|s| (*s).to_string()).collect(),
            Err(r) => return Err(refuse(layer, &path, r)),
        };
        // The marker itself.
        dir.pop();
        match kind {
            Kind::Normal => {}
            Kind::Remove(name) if name == "." || name == ".." => {
                return Err(refuse(layer, &path, Refusal::Component(name)));
            }
            Kind::Remove(name) => ops.push(Op::Remove { dir, name }),
            Kind::Opaque => ops.push(Op::Opaque { dir }),
        }
    }
    Ok(ops)
}

/// The second half: apply them.
///
/// A whiteout of a name that is not there is **not an error**. Each layer is
/// built alone, and a layer can whiteout a path that no earlier layer made;
/// a refusal there refuses ordinary images.
pub fn apply(root: &Dir, ops: &[Op]) -> Result<u64> {
    let mut removed = 0;
    for op in ops {
        match op {
            Op::Remove { dir, name } => {
                let mut parts: Vec<&str> = dir.iter().map(|s| s.as_str()).collect();
                parts.push(name);
                // `false`: make no directory on the way to a removal. A
                // whiteout of a path that does not exist would otherwise MAKE
                // its parents.
                let Ok(Ok((parent, last))) = safety::open_parent(root, &parts, false) else {
                    continue;
                };
                // A tree of an earlier extraction can have closed directories.
                open_for_write(parent.fd());
                if remove_at(parent.fd(), &last)? {
                    removed += 1;
                }
            }
            Op::Opaque { dir } => {
                let parts: Vec<&str> = dir.iter().map(|s| s.as_str()).collect();
                let d = if parts.is_empty() {
                    // The root of the layer: legal, and it means "this layer
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
                // The CONTENTS, not the directory (as udocker: list the
                // directory and remove each entry). An opaque marker that
                // removed its own directory would remove the place that the
                // next entries write into.
                open_for_write(d.0);
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
    let fd = sys::openat(root.fd(), &c, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC, 0)
        .map_err(|e| error::sys("cannot re-open the destination", e))?;
    Ok(DirFd(fd))
}

/// Remove one name under `parent`, and each entry under it when it is a
/// directory.
///
/// It does not follow a symlink. A symlink is removed as a symlink, and the
/// tree that it points to stays: the removal of a link to `/etc` is not the
/// removal of `/etc`.
///
/// It returns true when it removed something, so that the caller can tell
/// "not there" from "removed".
pub fn remove_at(parent: i64, name: &str) -> Result<bool> {
    let n = cbuf(name)?;
    match sys::unlinkat(parent, &n, 0) {
        Ok(_) => return Ok(true),
        Err(e) if e == sys::ENOENT => return Ok(false),
        // EISDIR on Linux, EPERM as POSIX allows: it is a directory.
        Err(e) if e == sys::EISDIR || e == sys::EPERM => {}
        Err(e) => return Err(error::sys(format!("cannot remove {name:?}"), e)),
    }
    // A directory. Empty it, then remove it.
    let fd = match sys::openat(parent, &n, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC | sys::O_NOFOLLOW, 0) {
        Ok(fd) => DirFd(fd),
        Err(_) => return Ok(false),
    };
    // The pass after the last layer can close a directory (0555), and the
    // owner cannot remove an entry of a directory that has no owner write.
    open_for_write(fd.0);
    for e in sys::getdents64(fd.0).unwrap_or_default() {
        let child = String::from_utf8_lossy(&e.name).into_owned();
        remove_at(fd.0, &child)?;
    }
    drop(fd);
    match sys::unlinkat(parent, &n, sys::AT_REMOVEDIR) {
        Ok(_) => Ok(true),
        Err(e) if e == sys::ENOENT => Ok(false),
        Err(e) => Err(error::sys(format!("cannot remove the directory {name:?}"), e)),
    }
}

/// Add the owner bits to the directory of `fd` when one is off. A failure
/// shows at the first removal in it.
fn open_for_write(fd: i64) {
    if let Ok(st) = sys::fstatat(fd, &sys::cempty(), sys::AT_EMPTY_PATH)
        && st.st_mode & 0o700 != 0o700
    {
        let _ = sys::fchmod(fd, u64::from(st.st_mode & 0o7777 | 0o700));
    }
}
