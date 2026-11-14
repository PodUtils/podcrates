//! The operations of [`Root`](crate::write::Root) on two layers. Each one
//! has the result of the same operation on one root that holds the merged
//! view; the difference is where a write lands.

use std::collections::{BTreeSet, VecDeque};

use pod_extract::safety::OwnerBits;
use pod_hostprobe::sys::{self, CBuf};

use super::{Entry, Layer, LayerRefusal, Layers, Place, is_type, own, refuse, reopen, whiteout};
use crate::error::{self, Result, cbuf};
use crate::write::{Fd, Kind, READ_FLAGS, Root, clear, create, fstat, kind_of, not_through};

impl Layers<'_> {
    /// The last component of `rel`, in its directory. With `follow`, a link
    /// there is followed, held in the root, and the entry is its target.
    pub(crate) fn resolve_last(
        &self,
        rel: &str,
        follow: bool,
    ) -> Result<std::result::Result<(Place, String, Entry), String>> {
        let (mut place, mut name) = match self.resolve_parent(rel, false)? {
            Ok(x) => x,
            Err(why) => return Ok(Err(why)),
        };
        let mut links = 0;
        loop {
            let e = self.lookup(&place, &name)?;
            let link = match &e.hit {
                Some(h) if follow && is_type(&h.st, sys::S_IFLNK) => self.readlink(&place, h.layer, &name)?,
                _ => return Ok(Ok((place, name, e))),
            };
            links += 1;
            if links > super::MAX_LINKS {
                return Ok(Err(format!("{rel}: ELOOP")));
            }
            let start = if link.starts_with('/') { self.root_place()? } else { place };
            let mut todo: VecDeque<(String, bool)> =
                link.split('/').filter(|p| !p.is_empty()).map(|p| (p.to_string(), false)).collect();
            name = match todo.pop_back() {
                Some((last, _)) if last != "." && last != ".." => last,
                _ => return Ok(Err(format!("{rel}: the link names a directory"))),
            };
            place = match self.walk(start, todo, false, &mut links)? {
                Ok(p) => p,
                Err(why) => return Ok(Err(why)),
            };
        }
    }

    pub(crate) fn kind(&self, rel: &str) -> Result<Kind> {
        let Ok((place, name, e)) = self.resolve_last(rel, false)? else {
            return Ok(Kind::Missing);
        };
        Ok(match e.hit {
            Some(h) => kind_of(place.fd(h.layer), &cbuf(&name)?, &h.st),
            None => Kind::Missing,
        })
    }

    /// Open the entry at `rel` in its layer for a read, with `O_NONBLOCK`.
    pub(crate) fn open_read(&self, rel: &str, follow: bool) -> Result<Option<Fd>> {
        let Ok((place, name, e)) = self.resolve_last(rel, follow)? else {
            return Ok(None);
        };
        let Some(h) = e.hit else {
            return Ok(None);
        };
        Ok(sys::openat(place.fd(h.layer), &cbuf(&name)?, READ_FLAGS | sys::O_NOFOLLOW, 0).ok().map(Fd))
    }

    /// The write of [`Root::write`] after its check: the new file replaces
    /// the entry of the upper layer and hides the one of the lower layer.
    pub(crate) fn replace(&self, rel: &str, bytes: &[u8], mode: u32) -> Result<()> {
        let (place, name) = match self.resolve_parent(rel, true)? {
            Ok(x) => x,
            Err(why) => return Err(not_through(rel, &why)),
        };
        let e = self.lookup(&place, &name)?;
        if e.hit.as_ref().is_some_and(|h| is_type(&h.st, sys::S_IFDIR)) {
            return Err(error::policy(format!(
                "{rel} is a directory in this image; the crate does not replace a directory with a file"
            )));
        }
        let up = self.upper_dir(&place)?;
        let _open = OwnerBits::add(up.0);
        let c = cbuf(&name)?;
        if e.upper_holds {
            clear(up.0, &c, rel)?;
        }
        create(up.0, &c, rel, bytes, mode)
    }

    /// [`Root::set_mode`]: a file of the lower layer is copied up first.
    pub(crate) fn set_mode(&self, rel: &str, mode: u32) -> Result<()> {
        let (place, name) = match self.resolve_parent(rel, false)? {
            Ok(x) => x,
            Err(why) => {
                return Err(error::policy(format!("{rel}: the crate does not change a mode through this path: {why}")));
            }
        };
        match self.lookup(&place, &name)?.hit {
            None => return Err(error::sys(format!("cannot open {rel} to set its mode"), sys::ENOENT)),
            Some(h) if !is_type(&h.st, sys::S_IFREG) => {
                return Err(error::policy(format!("{rel} is not a regular file; the crate does not change its mode")));
            }
            Some(h) if h.layer == Layer::Lower => self.copy_up(&place, &name)?,
            Some(_) => {}
        }
        let up = self.upper_dir(&place)?;
        let fd = Fd(sys::openat(up.0, &cbuf(&name)?, READ_FLAGS | sys::O_NOFOLLOW, 0)
            .map_err(|e| error::sys(format!("cannot open {rel} to set its mode"), e))?);
        if !is_type(&fstat(&fd, rel)?, sys::S_IFREG) {
            return Err(error::policy(format!("{rel} is not a regular file; the crate does not change its mode")));
        }
        sys::fchmod(fd.0, mode as u64).map_err(|e| error::sys(format!("cannot set the mode of {rel}"), e))?;
        Ok(())
    }

    /// The directory of `rel` in the upper layer, made and copied up, and
    /// the last component. `Ok(Err(why))`: it is out of reach.
    pub(crate) fn upper_parent(&self, rel: &str, make: bool) -> Result<std::result::Result<(Fd, String), String>> {
        let (place, name) = match self.resolve_parent(rel, make)? {
            Ok(x) => x,
            Err(why) => return Ok(Err(why)),
        };
        Ok(Ok((self.upper_dir(&place)?, name)))
    }

    /// [`Root::dir_mode`]: the mode of the directory that the merged view
    /// shows, of the upper layer where it holds one.
    pub(crate) fn dir_mode(&self, rel: &str) -> Result<Option<u32>> {
        let Some(place) = self.dir(rel)? else {
            return Ok(None);
        };
        let fd = place.upper.as_ref().or(place.lower.as_ref());
        Ok(fd.and_then(|f| fstat(f, rel).ok()).map(|st| st.st_mode & 0o7777))
    }

    /// The directory of the merged view at `rel`, each link followed.
    pub(crate) fn dir(&self, rel: &str) -> Result<Option<Place>> {
        let parts = Root::parts(rel)?;
        let mut links = 0;
        Ok(self.walk(self.root_place()?, own(&parts), false, &mut links)?.ok())
    }

    /// Open the file at `rel` for the write of [`Root::write_following`],
    /// the last link followed: in the upper layer, after a copy-up of a file
    /// of the lower layer.
    pub(crate) fn open_write(&self, rel: &str, flags: u64, mode: u32) -> Result<std::result::Result<Fd, String>> {
        let (place, name, e) = match self.resolve_last(rel, true)? {
            Ok(x) => x,
            Err(why) => return Ok(Err(format!("{rel}: {why}"))),
        };
        match &e.hit {
            Some(h) if !is_type(&h.st, sys::S_IFREG) => {
                return Ok(Err(format!("{rel} is not a regular file; the crate does not write into it")));
            }
            Some(h) if h.layer == Layer::Lower => self.copy_up(&place, &name)?,
            _ => {}
        }
        let up = self.upper_dir(&place)?;
        let _open = OwnerBits::add(up.0);
        let c = cbuf(&name)?;
        if e.hit.is_none() && e.upper_holds {
            clear(up.0, &c, rel)?;
        }
        Ok(sys::openat(up.0, &c, flags | sys::O_NOFOLLOW, mode as u64)
            .map(Fd)
            .map_err(|x| format!("{rel}: {}", x.name())))
    }

    pub(crate) fn mkdirs(&self, rel: &str) -> Result<()> {
        let parts = Root::parts(rel)?;
        let mut links = 0;
        match self.walk(self.root_place()?, own(&parts), true, &mut links)? {
            Ok(_) => Ok(()),
            Err(why) => Err(error::policy(format!("{rel}: {why}"))),
        }
    }

    /// The union of the names of both layers at `rel`, sorted, less each
    /// name that a whiteout hides and each name of the lower layer under an
    /// opaque directory.
    pub(crate) fn list(&self, rel: &str) -> Result<Vec<String>> {
        let Some(place) = self.dir(rel)? else {
            return Ok(Vec::new());
        };
        let mut names = BTreeSet::new();
        let mut hidden = BTreeSet::new();
        for (layer, fd) in [(Layer::Upper, &place.upper), (Layer::Lower, &place.lower)] {
            let Some(fd) = fd else {
                continue;
            };
            let dir = reopen(fd.0)?;
            for d in sys::getdents64(dir.0).map_err(|e| error::sys(format!("cannot list {rel}"), e))? {
                let n = String::from_utf8_lossy(&d.name).to_string();
                if n == "." || n == ".." || hidden.contains(&n) {
                    continue;
                }
                let maybe_dev = d.d_type == sys::DT_UNKNOWN || d.d_type == 2;
                let wh = maybe_dev
                    && CBuf::new(&n)
                        .and_then(|c| sys::fstatat(fd.0, &c, sys::AT_SYMLINK_NOFOLLOW).ok())
                        .is_some_and(|st| whiteout(&st));
                if wh && layer == Layer::Upper {
                    hidden.insert(n);
                } else if !wh {
                    names.insert(n);
                }
            }
        }
        Ok(names.into_iter().collect())
    }

    /// [`Root::unlink`]: only an entry that the upper layer alone holds goes.
    pub(crate) fn unlink(&self, rel: &str) -> Result<bool> {
        let Ok((place, name, e)) = self.resolve_last(rel, false)? else {
            return Ok(false);
        };
        if e.hit.is_none() {
            return Ok(false);
        }
        if e.lower_holds {
            return Err(refuse(LayerRefusal::Remove(rel.to_string())));
        }
        let _open = OwnerBits::add(place.fd(Layer::Upper));
        match sys::unlinkat(place.fd(Layer::Upper), &cbuf(&name)?, 0) {
            Ok(_) => Ok(true),
            Err(x) if x == sys::ENOENT => Ok(false),
            Err(x) => Err(error::sys(format!("cannot remove {rel}"), x)),
        }
    }

    pub(crate) fn symlink(&self, rel: &str, target: &str) -> Result<std::result::Result<(), String>> {
        let (place, name, e) = match self.resolve_last(rel, false)? {
            Ok(x) => x,
            Err(why) => return Ok(Err(why)),
        };
        if e.hit.is_some() {
            return Ok(Err(sys::EEXIST.name()));
        }
        let up = self.upper_dir(&place)?;
        let _open = OwnerBits::add(up.0);
        let (c, t) = (cbuf(&name)?, cbuf(target)?);
        if e.upper_holds {
            clear(up.0, &c, rel)?;
        }
        Ok(sys::symlinkat(&t, up.0, &c).map(|_| ()).map_err(|x| x.name()))
    }
}
