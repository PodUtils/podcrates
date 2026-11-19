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
use crate::tree::Walk;
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
    let side = crate::pax::Side::default();
    let mut ar = tar::Archive::new(crate::pax::Framed::new(stream, side.clone()));
    for entry in ar.entries().map_err(error::io)? {
        let entry = entry.map_err(|e| crate::pax::failed(&side, layer, e))?;
        let raw = side.next().path.unwrap_or_else(|| entry.path_bytes().into_owned());
        let path = String::from_utf8_lossy(&raw).into_owned();
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
                // A tree of an earlier extraction can have closed
                // directories, when the caller did not run
                // `modes::open_tree` first (call C646).
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
                    if remove_bytes(d.0, &e.name)? {
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
/// The walk holds one directory open at a time ([`crate::tree`]). A tree
/// deeper than [`crate::tree::MAX_DEPTH`] below `name` is refused with the
/// cause [`crate::tree::TooDeep`], after the removal of the entries that the
/// walk reached first.
///
/// It returns true when it removed something, so that the caller can tell
/// "not there" from "removed".
pub fn remove_at(parent: i64, name: &str) -> Result<bool> {
    remove_at_to(parent, name.as_bytes(), crate::tree::MAX_DEPTH)
}

/// [`remove_at`] of a name of a listing, which need not be UTF-8.
fn remove_bytes(parent: i64, name: &[u8]) -> Result<bool> {
    remove_at_to(parent, name, crate::tree::MAX_DEPTH)
}

/// [`remove_at`] with the depth limit as a parameter, for the tests.
pub(crate) fn remove_at_to(parent: i64, name: &[u8], limit: usize) -> Result<bool> {
    if let Some(removed) = unlink(parent, name)? {
        return Ok(removed);
    }
    // A directory. Empty each level, then remove it from the level above.
    let mut w = Walk::at(parent, limit)?;
    if !w.descend(name, open_closed)? {
        return Ok(false);
    }
    loop {
        if let Some(e) = w.next() {
            if unlink(w.fd(), &e.name)?.is_none() {
                // A directory that does not open stays, and the removal of
                // its parent gives the error.
                w.descend(&e.name, open_closed)?;
            }
            continue;
        }
        let done = w.ascend()?;
        let n = sys::CBuf::from_bytes(&done).ok_or_else(|| error::usage("a name with a NUL"))?;
        let gone = match sys::unlinkat(w.fd(), &n, sys::AT_REMOVEDIR) {
            Ok(_) => true,
            Err(e) if e == sys::ENOENT => false,
            Err(e) => {
                let text = format!("cannot remove the directory {:?}", String::from_utf8_lossy(&done));
                return Err(error::sys(text, e));
            }
        };
        if w.at_start() {
            return Ok(gone);
        }
    }
}

/// Unlink one name that is not a directory. `Some(true)`: removed;
/// `Some(false)`: not there; `None`: a directory.
fn unlink(parent: i64, name: &[u8]) -> Result<Option<bool>> {
    let n = sys::CBuf::from_bytes(name).ok_or_else(|| {
        error::usage(format!("{:?} contains a NUL and cannot reach the kernel", String::from_utf8_lossy(name)))
    })?;
    match sys::unlinkat(parent, &n, 0) {
        Ok(_) => Ok(Some(true)),
        Err(e) if e == sys::ENOENT => Ok(Some(false)),
        // EISDIR on Linux, EPERM as POSIX allows: it is a directory.
        Err(e) if e == sys::EISDIR || e == sys::EPERM => Ok(None),
        Err(e) => Err(error::sys(format!("cannot remove {:?}", String::from_utf8_lossy(name)), e)),
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

/// Open the directory `n` of `parent` with `O_NOFOLLOW`, and add the owner
/// bits to it. `None`: not a directory, or it does not open.
///
/// The pass after the last layer can close a directory (0555), and the
/// owner cannot list or change a directory with no owner read or write. A
/// directory with no owner read (0311) does not open for a read: an
/// `O_PATH` descriptor needs no permission, and a mode change through its
/// link in `/proc/self/fd` changes that directory and no other.
pub(crate) fn open_closed(parent: i64, n: &sys::CBuf) -> Option<i64> {
    let flags = sys::O_RDONLY | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
    let fd = match sys::openat(parent, n, flags, 0) {
        Ok(fd) => fd,
        Err(e) if e == sys::EACCES => {
            let p =
                sys::openat(parent, n, sys::O_PATH | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC, 0).ok()?;
            let st = sys::fstatat(p, &sys::cempty(), sys::AT_EMPTY_PATH);
            if let (Ok(st), Some(link)) = (st, sys::CBuf::new(&format!("/proc/self/fd/{p}"))) {
                let _ = sys::fchmodat(sys::AT_FDCWD as i64, &link, u64::from(st.st_mode & 0o7777 | 0o700), 0);
            }
            let _ = sys::close(p);
            sys::openat(parent, n, flags, 0).ok()?
        }
        Err(_) => return None,
    };
    open_for_write(fd);
    Some(fd)
}
