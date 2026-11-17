//! The hard links of a layer whose target comes later in the same layer
//! (PodUtils/podvm#569): each one waits for the end of its layer.

use pod_hostprobe::sys;

use super::replace;
use crate::error::{self, Result, cbuf, refuse};
use crate::safety::{self, Dir};

/// A hard link whose target was not there yet.
pub(super) struct Later {
    pub(super) path: String,
    pub(super) target: String,
    pub(super) parts: Vec<String>,
    pub(super) tparts: Vec<String>,
    pub(super) uid: u64,
    pub(super) gid: u64,
    pub(super) mode: u32,
}

/// A hard link of [`Later`], at the end of its layer, with the checks of
/// the first try. A target that is still missing is named as such.
pub(super) fn link_later(root: &Dir, digest: &str, l: &Later) -> Result<()> {
    let parts: Vec<&str> = l.parts.iter().map(String::as_str).collect();
    let tparts: Vec<&str> = l.tparts.iter().map(String::as_str).collect();
    let (parent, name) = match safety::open_parent(root, &parts, true)? {
        Ok(x) => x,
        Err(r) => return Err(refuse(digest, &l.path, r)),
    };
    let (tparent, tname) = match safety::open_parent(root, &tparts, false)? {
        Ok(x) => x,
        Err(r) => return Err(refuse(digest, &l.path, r)),
    };
    replace(&parent, &name)?;
    let (tn, n) = (cbuf(&tname)?, cbuf(&name)?);
    sys::linkat(tparent.fd(), &tn, parent.fd(), &n, 0).map(drop).map_err(|e| {
        let why = if e == sys::ENOENT { "the target is not in the layer or before it" } else { "the link failed" };
        error::sys(format!("cannot link {:?} to {:?}: {why}", l.path, l.target), e)
    })
}
