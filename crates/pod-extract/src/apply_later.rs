//! The hard links of a layer whose target comes later in the same layer
//! (PodUtils/podvm#569): each one waits for the end of its layer.
//!
//! A link waits when its target, or a directory above its target, is not
//! there at its entry. At the end of the layer a target that is still not
//! there, or a target that is a directory, refuses the layer
//! ([`Refusal::Link`]): the layer is not whole, and `crate::flatten` gives
//! the same refusal for it.

use pod_core::Failure;
use pod_hostprobe::sys::{self, Errno};

use super::replace;
use crate::error::{self, Result, cbuf, refuse};
use crate::safety::{self, Dir, LINK_MISSING, Refusal};
use crate::xattr::Wanted;

/// A hard link whose target was not there yet.
pub(super) struct Later {
    pub(super) path: String,
    pub(super) target: String,
    pub(super) parts: Vec<String>,
    pub(super) tparts: Vec<String>,
    pub(super) uid: u64,
    pub(super) gid: u64,
    pub(super) mode: u32,
    /// Set on the link after it is made.
    pub(super) wanted: Wanted,
}

/// The directory of the target of a hard link, and its name; `None` when a
/// directory above the target is not there yet (the link waits).
pub(super) fn open_target(root: &Dir, digest: &str, path: &str, tparts: &[&str]) -> Result<Option<(Dir, String)>> {
    match safety::open_parent(root, tparts, false) {
        Ok(Ok(x)) => Ok(Some(x)),
        Ok(Err(r)) => Err(refuse(digest, path, r)),
        Err(f) if missing(&f) => Ok(None),
        Err(f) => Err(f),
    }
}

/// A failed `linkat` of the hard link `path`: a target that is a directory
/// refuses the layer.
pub(super) fn failed(digest: &str, path: &str, target: &str, e: Errno) -> Failure {
    if e == sys::EPERM {
        return refuse(digest, path, Refusal::Link(format!("{target:?} is a directory")));
    }
    error::sys(format!("cannot link {path:?} to {target:?}"), e)
}

/// An entry below a hard link of its layer that waits: a path through a
/// file of the same layer, refused as when the link is there.
pub(super) fn through(later: &[Later], parts: &[&str]) -> std::result::Result<(), Refusal> {
    for l in later {
        if l.parts.len() < parts.len() && l.parts.iter().zip(parts).all(|(a, b)| a == b) {
            return Err(Refusal::Escapes(format!("{:?}: a hard link of the same layer", l.path)));
        }
    }
    Ok(())
}

/// The failure of a walk is a directory that is not there.
fn missing(f: &Failure) -> bool {
    error::cause::<std::io::Error>(f).and_then(std::io::Error::raw_os_error) == Some(sys::ENOENT.0)
}

/// A hard link of [`Later`], at the end of its layer, with the checks of
/// the first try. A target that is still missing is named as such. Returns
/// the parent and the name of the link.
pub(super) fn link_later(root: &Dir, digest: &str, l: &Later) -> Result<(Dir, String)> {
    let parts: Vec<&str> = l.parts.iter().map(String::as_str).collect();
    let tparts: Vec<&str> = l.tparts.iter().map(String::as_str).collect();
    let (parent, name) = match safety::open_parent(root, &parts, true)? {
        Ok(x) => x,
        Err(r) => return Err(refuse(digest, &l.path, r)),
    };
    let gone = || refuse(digest, &l.path, Refusal::Link(format!("{:?}: {LINK_MISSING}", l.target)));
    let Some((tparent, tname)) = open_target(root, digest, &l.path, &tparts)? else {
        return Err(gone());
    };
    replace(&parent, &name)?;
    let (tn, n) = (cbuf(&tname)?, cbuf(&name)?);
    match sys::linkat(tparent.fd(), &tn, parent.fd(), &n, 0) {
        Ok(_) => Ok((parent, name)),
        Err(e) if e == sys::ENOENT => Err(gone()),
        Err(e) => Err(failed(digest, &l.path, &l.target, e)),
    }
}
