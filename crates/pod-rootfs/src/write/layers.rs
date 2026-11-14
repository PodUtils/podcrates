//! A root of two layers, as overlayfs mounts them: the read-only `lower`
//! layer (an extracted image that other runs share) and the `upper` layer
//! that takes each write.
//!
//! The rules, the same as the rules of overlayfs:
//!
//! * A read sees `upper` over `lower`: an entry of `upper` hides the same
//!   name of `lower`, and a listing is the union. A directory that both hold
//!   is one merged directory. A whiteout (a character device 0:0) hides the
//!   name; an opaque directory of `upper` (`trusted.overlay.opaque` or
//!   `user.overlay.opaque` is `y`) hides each entry of `lower` under it.
//! * A write goes to `upper` only. A change of a file that only `lower`
//!   holds copies it up first (the bytes, the owner where the process may
//!   set it, the mode), then changes the copy. A new entry is made in
//!   `upper`, and each parent directory that only `lower` holds is copied up
//!   with its mode and owner.
//! * A remove of an entry that `lower` holds is refused with
//!   [`LayerRefusal::Remove`]: only a whiteout removes it, and the crate
//!   makes none. A new directory over a whiteout that hides an entry of
//!   `lower` is refused with [`LayerRefusal::Opaque`].
//!
//! Each path resolves here, one component at a time, from a descriptor of
//! each layer: a directory opens with `O_NOFOLLOW`, and a symlink of the
//! merged view is read and followed in this process, held in the root (an
//! absolute target starts at the root, and `..` at the root stays there).
//! The kernel never follows a link of a layer, so nothing resolves outside
//! the two layers, and no `openat2(2)` is necessary. `lower` only opens for
//! read.

use std::collections::VecDeque;
use std::fmt;
use std::path::{Path, PathBuf};

use pod_core::{Class, Failure, Stage};
use pod_extract::safety::{Dir, OwnerBits};
use pod_hostprobe::sys;

use super::{Fd, READ_FLAGS, Root, fstat, write_all};
use crate::error::{self, Result, cbuf};

/// The links that one resolution follows at most, as the kernel
/// (`MAXSYMLINKS`).
const MAX_LINKS: u32 = 40;

/// The attributes that mark an opaque directory of overlayfs: `trusted.` for
/// a mount of root, `user.` for a mount with `userxattr`.
const OPAQUE: [&str; 2] = ["trusted.overlay.opaque", "user.overlay.opaque"];

/// What the crate refuses on two layers. The cause of a
/// [`pod_core::Failure`] of `Class::Unsupported`: [`layer_refusal`] gives
/// it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerRefusal {
    /// A remove of an entry that the lower layer holds, at this path. Only
    /// a whiteout removes it.
    Remove(String),
    /// A new directory at this path, where a whiteout of the upper layer
    /// hides an entry of the lower layer. Only an opaque directory hides it.
    Opaque(String),
}

impl fmt::Display for LayerRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayerRefusal::Remove(p) => write!(
                f,
                "{p}: the lower layer holds this entry, and only a whiteout of overlayfs removes it; the crate \
                 does not remove it"
            ),
            LayerRefusal::Opaque(p) => write!(
                f,
                "{p}: a whiteout of the upper layer hides an entry of the lower layer, and only an opaque \
                 directory keeps it hidden under a new directory; the crate does not make it"
            ),
        }
    }
}

impl std::error::Error for LayerRefusal {}

/// The [`LayerRefusal`] of a failure, where it is one.
pub fn layer_refusal(f: &Failure) -> Option<&LayerRefusal> {
    std::error::Error::source(f)?.downcast_ref()
}

pub(crate) fn refuse(r: LayerRefusal) -> Failure {
    Failure::caused(Stage::Host, Class::Unsupported, r.to_string(), r)
}

/// The upper layer of a [`Root`].
pub(crate) struct Upper {
    pub(crate) dir: Dir,
    pub(crate) path: PathBuf,
}

impl Upper {
    /// Open `upper`. Two layers that hold each other are refused: a write
    /// to the upper layer then changes the lower one.
    pub(crate) fn open(lower: &Path, upper: &Path) -> Result<Upper> {
        let text =
            upper.to_str().ok_or_else(|| error::usage(format!("the upper layer {} is not UTF-8", upper.display())))?;
        let dir = Dir::open(text)?;
        let real =
            |p: &Path| std::fs::canonicalize(p).map_err(|e| error::io(format!("cannot resolve {}", p.display()), e));
        let (l, u) = (real(lower)?, real(upper)?);
        if u.starts_with(&l) || l.starts_with(&u) {
            return Err(error::usage(format!(
                "the upper layer {} and the lower layer {} hold each other; a write to the upper layer would \
                 change the lower one",
                u.display(),
                l.display()
            )));
        }
        Ok(Upper { dir, path: upper.to_path_buf() })
    }
}

/// The two layers of a [`Root`], for one operation.
pub(crate) struct Layers<'a> {
    pub(crate) lower: &'a Dir,
    pub(crate) upper: &'a Upper,
}

/// The layer that holds an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    Upper,
    Lower,
}

/// A directory of the merged view: its components from the root, and a
/// descriptor of each layer that holds it and shows it.
pub(crate) struct Place {
    pub(crate) parts: Vec<String>,
    pub(crate) upper: Option<Fd>,
    pub(crate) lower: Option<Fd>,
}

impl Place {
    /// The descriptor of `layer`. A [`Hit`] in a layer comes from its
    /// descriptor, so it is there; `-1` is `EBADF` and opens nothing.
    pub(crate) fn fd(&self, layer: Layer) -> i64 {
        let fd = match layer {
            Layer::Upper => &self.upper,
            Layer::Lower => &self.lower,
        };
        fd.as_ref().map_or(-1, |f| f.0)
    }

    /// The path of `name` in this directory, for a text.
    pub(crate) fn path_of(&self, name: &str) -> String {
        let mut p = self.parts.join("/");
        if !p.is_empty() {
            p.push('/');
        }
        p.push_str(name);
        p
    }
}

/// The entry of the merged view at a name, and the layer that gives it.
pub(crate) struct Hit {
    pub(crate) layer: Layer,
    pub(crate) st: sys::Stat,
}

/// One name of a [`Place`].
pub(crate) struct Entry {
    /// What the merged view shows. `None`: nothing, or a whiteout.
    pub(crate) hit: Option<Hit>,
    /// The upper layer holds something at the name, a whiteout too.
    pub(crate) upper_holds: bool,
    /// The lower layer shows an entry at the name, hidden or not.
    pub(crate) lower_holds: bool,
}

pub(crate) fn is_type(st: &sys::Stat, t: u32) -> bool {
    st.st_mode & sys::S_IFMT == t
}

/// A whiteout of overlayfs: a character device 0:0.
pub(crate) fn whiteout(st: &sys::Stat) -> bool {
    is_type(st, 0o020000) && st.st_rdev == 0
}

fn opaque(fd: i64) -> bool {
    OPAQUE.iter().any(|n| {
        let mut b = [0u8; 2];
        sys::CBuf::new(n).is_some_and(|c| sys::fgetxattr(fd, &c, &mut b) == Ok(1) && b[0] == b'y')
    })
}

/// The directory `name` of `fd`, with no follow of a link.
pub(crate) fn child_dir(fd: i64, name: &str) -> Result<Option<Fd>> {
    let c = cbuf(name)?;
    let flags = sys::O_RDONLY | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
    Ok(sys::openat(fd, &c, flags, 0).ok().map(Fd))
}

/// A new descriptor of the directory of `fd`.
pub(crate) fn reopen(fd: i64) -> Result<Fd> {
    let flags = sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC;
    Ok(Fd(sys::openat(fd, &cbuf(".")?, flags, 0).map_err(|e| error::sys("cannot open a layer again", e))?))
}

/// Give the file of `fd` the owner of `st`. A process that may not is not
/// stopped: the file is then of the process.
fn take_owner(fd: i64, st: &sys::Stat) {
    let _ = sys::fchownat(fd, &sys::cempty(), st.st_uid, st.st_gid, sys::AT_EMPTY_PATH);
}

/// A component still to walk, and whether the caller named it (a missing
/// one may be made) or a link target did.
pub(crate) type Todo = VecDeque<(String, bool)>;

pub(crate) fn own(parts: &[&str]) -> Todo {
    parts.iter().map(|s| (s.to_string(), true)).collect()
}

impl Layers<'_> {
    pub(crate) fn root_place(&self) -> Result<Place> {
        let upper = reopen(self.upper.dir.fd())?;
        let lower = if opaque(upper.0) { None } else { Some(reopen(self.lower.fd())?) };
        Ok(Place { parts: Vec::new(), upper: Some(upper), lower })
    }

    /// What the merged view shows at `name` in `place`.
    pub(crate) fn lookup(&self, place: &Place, name: &str) -> Result<Entry> {
        let c = cbuf(name)?;
        let stat = |fd: &Option<Fd>| fd.as_ref().and_then(|f| sys::fstatat(f.0, &c, sys::AT_SYMLINK_NOFOLLOW).ok());
        let up = stat(&place.upper);
        let low = stat(&place.lower).filter(|st| !whiteout(st));
        let (upper_holds, lower_holds) = (up.is_some(), low.is_some());
        let hit = match up {
            Some(st) if whiteout(&st) => None,
            Some(st) => Some(Hit { layer: Layer::Upper, st }),
            None => low.map(|st| Hit { layer: Layer::Lower, st }),
        };
        Ok(Entry { hit, upper_holds, lower_holds })
    }

    /// The directory `name` of `place`, where the merged view shows one.
    fn descend(&self, place: &Place, name: &str) -> Result<Option<Place>> {
        let Some(h) = self.lookup(place, name)?.hit else {
            return Ok(None);
        };
        if !is_type(&h.st, sys::S_IFDIR) {
            return Ok(None);
        }
        let in_layer = |fd: &Option<Fd>| -> Result<Option<Fd>> {
            match fd {
                Some(f) => child_dir(f.0, name),
                None => Ok(None),
            }
        };
        let upper = if h.layer == Layer::Upper { in_layer(&place.upper)? } else { None };
        // An upper directory over a lower file hides the file; an opaque one
        // hides the lower directory.
        let lower = if upper.as_ref().is_some_and(|u| opaque(u.0)) { None } else { in_layer(&place.lower)? };
        if upper.is_none() && lower.is_none() {
            return Ok(None);
        }
        let mut parts = place.parts.clone();
        parts.push(name.to_string());
        Ok(Some(Place { parts, upper, lower }))
    }

    /// The place of the components `parts`, each a directory.
    fn place_of(&self, parts: &[String]) -> Result<Option<Place>> {
        let mut p = self.root_place()?;
        for c in parts {
            match self.descend(&p, c)? {
                Some(next) => p = next,
                None => return Ok(None),
            }
        }
        Ok(Some(p))
    }

    pub(crate) fn readlink(&self, place: &Place, layer: Layer, name: &str) -> Result<String> {
        let b = sys::readlinkat(place.fd(layer), &cbuf(name)?)
            .map_err(|e| error::sys(format!("cannot read the link {}", place.path_of(name)), e))?;
        Ok(String::from_utf8_lossy(&b).to_string())
    }

    /// Walk `todo` from `place`, each component a directory of the merged
    /// view; a symlink is followed. With `make`, a missing component that
    /// the caller named is made in the upper layer. `Ok(Err(why))`: a
    /// component is missing or not a directory, or the links loop.
    pub(crate) fn walk(
        &self,
        mut place: Place,
        mut todo: Todo,
        make: bool,
        links: &mut u32,
    ) -> Result<std::result::Result<Place, String>> {
        while let Some((c, named)) = todo.pop_front() {
            if c == "." {
                continue;
            }
            if c == ".." {
                let mut up = place.parts.clone();
                up.pop();
                match self.place_of(&up)? {
                    Some(p) => place = p,
                    None => return Ok(Err(format!("{:?}: the parent is gone", place.parts.join("/")))),
                }
                continue;
            }
            if let Some(next) = self.descend(&place, &c)? {
                place = next;
                continue;
            }
            let e = self.lookup(&place, &c)?;
            match &e.hit {
                Some(h) if is_type(&h.st, sys::S_IFLNK) => {
                    *links += 1;
                    if *links > MAX_LINKS {
                        return Ok(Err(format!("{:?}: ELOOP", place.path_of(&c))));
                    }
                    let t = self.readlink(&place, h.layer, &c)?;
                    if t.starts_with('/') {
                        place = self.root_place()?;
                    }
                    for p in t.split('/').filter(|p| !p.is_empty()).rev() {
                        todo.push_front((p.to_string(), false));
                    }
                }
                Some(_) => return Ok(Err(format!("{:?}: ENOTDIR", place.path_of(&c)))),
                None if make && named => {
                    self.make_dir(&place, &c, &e)?;
                    // The upper layer may hold this directory only now.
                    place = match self.place_of(&place.parts)? {
                        Some(p) => p,
                        None => return Ok(Err(format!("{:?}: the directory is gone", place.parts.join("/")))),
                    };
                    match self.descend(&place, &c)? {
                        Some(next) => place = next,
                        None => return Ok(Err(format!("{:?}: ENOENT", place.path_of(&c)))),
                    }
                }
                None => return Ok(Err(format!("{:?}: ENOENT", place.path_of(&c)))),
            }
        }
        Ok(Ok(place))
    }

    /// The parent of `rel` and its last component.
    pub(crate) fn resolve_parent(&self, rel: &str, make: bool) -> Result<std::result::Result<(Place, String), String>> {
        let parts = Root::parts(rel)?;
        let Some((last, dirs)) = parts.split_last() else {
            return Ok(Err(format!("{rel:?}: an empty path")));
        };
        let mut links = 0;
        Ok(self.walk(self.root_place()?, own(dirs), make, &mut links)?.map(|p| (p, last.to_string())))
    }

    /// Make the directory `name` in the upper layer.
    fn make_dir(&self, place: &Place, name: &str, e: &Entry) -> Result<()> {
        let rel = place.path_of(name);
        if e.upper_holds && e.lower_holds {
            return Err(refuse(LayerRefusal::Opaque(rel)));
        }
        let up = self.upper_dir(place)?;
        let _open = OwnerBits::add(up.0);
        let c = cbuf(name)?;
        if e.upper_holds {
            super::clear(up.0, &c, &rel)?;
        }
        match sys::mkdirat(up.0, &c, 0o755) {
            Ok(_) => Ok(()),
            Err(x) if x == sys::EEXIST => Ok(()),
            Err(x) => Err(error::sys(format!("cannot create the directory {rel}"), x)),
        }
    }

    /// The directory of `place` in the upper layer. Each component that
    /// only the lower layer holds is copied up with its mode and owner.
    pub(crate) fn upper_dir(&self, place: &Place) -> Result<Fd> {
        if let Some(u) = &place.upper {
            return reopen(u.0);
        }
        let mut cur = reopen(self.upper.dir.fd())?;
        let mut low = if opaque(cur.0) { None } else { Some(reopen(self.lower.fd())?) };
        for c in &place.parts {
            let mut lc = match &low {
                Some(l) => child_dir(l.0, c)?,
                None => None,
            };
            cur = match child_dir(cur.0, c)? {
                Some(u) => {
                    if opaque(u.0) {
                        lc = None;
                    }
                    u
                }
                None => {
                    let cc = cbuf(c)?;
                    // A copy of a closed directory of the lower layer keeps
                    // its mode (0555): it opens for this one make.
                    let open = OwnerBits::add(cur.0);
                    match sys::mkdirat(cur.0, &cc, 0o700) {
                        Ok(_) => {}
                        Err(e) if e == sys::EEXIST => {}
                        Err(e) => return Err(error::sys(format!("cannot copy up the directory {c:?}"), e)),
                    }
                    drop(open);
                    let u = child_dir(cur.0, c)?
                        .ok_or_else(|| error::policy(format!("{c:?} of the upper layer is not a directory")))?;
                    let mode = match &lc {
                        Some(l) => {
                            let st = fstat(l, c)?;
                            take_owner(u.0, &st);
                            st.st_mode & 0o7777
                        }
                        None => 0o755,
                    };
                    sys::fchmod(u.0, mode as u64)
                        .map_err(|e| error::sys(format!("cannot set the mode of {c:?}"), e))?;
                    u
                }
            };
            low = lc;
        }
        Ok(cur)
    }

    /// Copy up the regular file `name` that only the lower layer holds: its
    /// bytes, its owner where the process may set it, and its mode. A copy
    /// that fails is removed: a part of a file would hide the whole one.
    pub(crate) fn copy_up(&self, place: &Place, name: &str) -> Result<()> {
        let rel = place.path_of(name);
        let Some(h) = self.lookup(place, name)?.hit else {
            return Ok(());
        };
        if h.layer == Layer::Upper {
            return Ok(());
        }
        let c = cbuf(name)?;
        let src = Fd(sys::openat(place.fd(Layer::Lower), &c, READ_FLAGS | sys::O_NOFOLLOW, 0)
            .map_err(|e| error::sys(format!("cannot open {rel} of the lower layer"), e))?);
        let st = fstat(&src, &rel)?;
        if !is_type(&st, sys::S_IFREG) {
            return Err(error::policy(format!("{rel} is not a regular file; the crate does not copy it up")));
        }
        let up = self.upper_dir(place)?;
        let _open = OwnerBits::add(up.0);
        let flags = sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL | sys::O_NOFOLLOW | sys::O_CLOEXEC;
        let dst = Fd(sys::openat(up.0, &c, flags, 0o600).map_err(|e| error::sys(format!("cannot copy up {rel}"), e))?);
        let copied = copy_bytes(&src, &dst, &rel).and_then(|()| {
            // The owner first: a change of the owner clears the set-id bits.
            take_owner(dst.0, &st);
            sys::fchmod(dst.0, (st.st_mode & 0o7777) as u64)
                .map(|_| ())
                .map_err(|e| error::sys(format!("cannot set the mode of {rel}"), e))
        });
        if copied.is_err() {
            let _ = sys::unlinkat(up.0, &c, 0);
        }
        copied
    }
}

fn copy_bytes(src: &Fd, dst: &Fd, rel: &str) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    loop {
        match sys::read(src.0, &mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => write_all(dst, &buf[..n as usize], rel)?,
            Err(e) if e == sys::EINTR => continue,
            Err(e) => return Err(error::sys(format!("cannot read {rel} of the lower layer"), e)),
        }
    }
}

mod ops;

#[cfg(test)]
mod tests;
