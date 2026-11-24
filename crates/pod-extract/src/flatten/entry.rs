//! The decision for each entry of a layer, and the check of the marks at
//! the end of each layer.

use std::io::{Read, Write};

use super::Flattened;
use super::links::{Ids, Links, Name};
use super::trie::{List, Local, ROOT, Trie};
use super::write::{Expand, Head, Kind, Out};
use crate::error::{Result, refuse};
use crate::safety::{self, MADE_MODE, Refusal};
use crate::sparse::Layout;
use crate::whiteout::{self, Kind as Marker};
use crate::{Layer, pax, xattr};

/// The state of one flatten.
pub(super) struct State {
    pub(super) trie: Trie,
    pub(super) total: Flattened,
    max: usize,
    /// The digest of each layer, by its number less one.
    digests: Vec<String>,
    /// The paths whose kind the current layer set, for its end.
    touched: Vec<u32>,
    /// The paths below which the current layer wrote while their kind in
    /// the layer was not known, with the node of the entry.
    pending: Vec<(u32, u32)>,
    pub(super) links: Links,
    /// The index of the next member of the current layer.
    member: u64,
}

impl State {
    pub(super) fn new(max: usize, layers: &[Layer]) -> State {
        State {
            trie: Trie::new(max),
            total: Flattened::default(),
            max,
            digests: layers.iter().map(|l| l.digest.clone()).collect(),
            touched: Vec::new(),
            pending: Vec::new(),
            links: Links::default(),
            member: 0,
        }
    }

    fn child(&mut self, parent: u32, name: &str) -> Result<u32> {
        self.trie.child(parent, name).map_err(|_| super::full(self.max))
    }

    /// One entry of the layer `layer`: a marker, or an entry that goes into
    /// the stream when no newer layer hides it.
    pub(super) fn entry<W: Write, R: Read>(
        &mut self,
        w: &mut Out<'_, W>,
        digest: &str,
        layer: u16,
        e: &mut tar::Entry<'_, R>,
        member: &pax::Member,
    ) -> Result<()> {
        let raw = member.path.clone().unwrap_or_else(|| e.path_bytes().to_vec());
        let path = String::from_utf8_lossy(&raw).into_owned();
        self.total.entries += 1;
        let index = self.member;
        self.member += 1;
        match whiteout::classify(&path) {
            Marker::Remove(name) => return self.marker(digest, layer, &path, Some(&name)),
            Marker::Opaque => return self.marker(digest, layer, &path, None),
            Marker::Normal => {}
        }
        let header = e.header().clone();
        let kind = header.entry_type();
        if safety::is_root_entry(&path, kind.is_dir()) {
            return Ok(());
        }
        let parts = safety::components(&path).map_err(|r| refuse(digest, &path, r))?;
        let sparse = crate::sparse::layout(member.sparse.as_ref(), e, digest, &path)?;
        let target = (kind.is_symlink() || kind.is_hard_link()).then(|| crate::apply::link_name(e, member));
        let mut link_parts = Vec::new();
        if let Some(t) = &target {
            let checked = if kind.is_symlink() {
                safety::rebase_symlink_target(&parts[..parts.len() - 1], t).map(|_| ())
            } else {
                safety::components(t).map(|p| link_parts = p)
            };
            checked.map_err(|r| refuse(digest, &path, r))?;
        }
        let class = match () {
            _ if kind.is_dir() => Some(Local::Dir),
            _ if kind.is_file() || kind.is_symlink() || kind.is_hard_link() => Some(Local::NonDir),
            _ => None,
        };

        let from = self.pending.len();
        let (parent, chain, above) = self.ancestors(digest, layer, &path, &parts)?;
        let leaf = self.child(parent, parts[parts.len() - 1])?;
        for p in &mut self.pending[from..] {
            p.1 = leaf;
        }
        let Some(class) = class else {
            // A device, a fifo or a socket: `extract` makes its parents and
            // not the entry.
            self.total.skipped += 1;
            let k = format!("{kind:?}");
            if !self.total.skipped_kinds.contains(&k) {
                self.total.skipped_kinds.push(k);
            }
            return Ok(());
        };
        let n = self.trie.node(leaf);
        let hidden = above || n.removed > layer || n.entry > layer;
        let prev = (n.local == layer).then_some(n.local_kind);
        if class == Local::NonDir && prev == Some(Local::Filled) {
            let why = "a file or a link over a directory that the same layer filled before it".to_string();
            return Err(refuse(digest, &path, Refusal::Stream(why)));
        }
        let n = self.trie.node_mut(leaf);
        n.entry = n.entry.max(layer);
        if class == Local::NonDir {
            n.nondir = n.nondir.max(layer);
        }
        n.local_kind = if prev == Some(Local::Filled) { Local::Filled } else { class };
        if prev.is_none() {
            n.local = layer;
            self.touched.push(leaf);
        }
        self.links.replace(leaf, !hidden);
        if kind.is_hard_link() {
            return self.hard_link(w, (digest, layer), (leaf, &parts, &path), &link_parts, &header, hidden, &chain);
        }
        let (wkind, link) = match &target {
            Some(t) => (Kind::Symlink, Some(t.as_str())),
            None if class == Local::Dir => (Kind::Dir, None),
            None => (Kind::File, None),
        };
        // A file or a symlink is an inode: the groups of hard links that
        // wait on its path take it (module `links`).
        let plan = (wkind != Kind::Dir).then(|| self.links.plan(&self.trie, leaf, layer, !hidden));
        if hidden {
            self.total.hidden += 1;
            let Some(name) = plan.as_ref().and_then(|p| p.name) else {
                if plan.is_some() {
                    self.links.passed(leaf, index);
                }
                return Ok(());
            };
            let x = put(w, &self.trie.path(name.node), (wkind, &header, member), e, sparse.as_ref(), link)?;
            self.total.xattrs.add(&x);
        } else {
            let x = put(w, &parts.join("/"), (wkind, &header, member), e, sparse.as_ref(), link)?;
            self.total.xattrs.add(&x);
            let n = self.trie.node_mut(leaf);
            n.emitted = layer;
            for a in chain {
                let n = self.trie.node_mut(a);
                if n.local_kind == Local::Dir {
                    n.local_kind = Local::Filled;
                }
            }
        }
        self.total.written += 1;
        match plan {
            Some(p) => self.links.wrote(w, &self.trie, &p, leaf, layer, index),
            None => Ok(()),
        }
    }

    /// A hard link entry: a name of the inode at its target at the time of
    /// the entry (module `links`). A target at or below the link is gone:
    /// the link removed it first, as `extract` does.
    #[allow(clippy::too_many_arguments)]
    fn hard_link<W: Write>(
        &mut self,
        w: &mut Out<'_, W>,
        (digest, layer): (&str, u16),
        (leaf, parts, path): (u32, &[&str], &str),
        tparts: &[&str],
        header: &tar::Header,
        hidden: bool,
        chain: &[u32],
    ) -> Result<()> {
        let ids = Ids {
            mode: header.mode().unwrap_or(0o644),
            uid: header.uid().unwrap_or(0),
            gid: header.gid().unwrap_or(0),
            mtime: header.mtime().unwrap_or(0),
        };
        if hidden {
            self.total.hidden += 1;
        } else {
            self.trie.node_mut(leaf).emitted = layer;
            // A later file of the layer over a directory above the name
            // cannot replace it in the stream (`Refusal::Stream`).
            for &a in chain {
                let n = self.trie.node_mut(a);
                if n.local_kind == Local::Dir {
                    n.local_kind = Local::Filled;
                }
            }
        }
        let g = self.links.link(leaf, layer, (!hidden).then_some(Name { node: leaf, ids }));
        if tparts.len() >= parts.len() && tparts[..parts.len()] == *parts {
            // `extract` removes the name of the link first.
            self.links.doomed(g, format!("{:?}: {}", tparts.join("/"), safety::LINK_MISSING));
            return Ok(());
        }
        let mut cur = ROOT;
        for name in &tparts[..tparts.len() - 1] {
            cur = self.child(cur, name)?;
            let n = self.trie.node(cur);
            if n.local != layer {
                // An older layer must leave a directory here, or no path.
                self.pending.push((cur, leaf));
            } else if n.local_kind == Local::NonDir {
                let why = format!("{name:?}: a symlink or a file of the same layer");
                return Err(refuse(digest, path, Refusal::Escapes(why)));
            }
        }
        let t = self.child(cur, tparts[tparts.len() - 1])?;
        self.links.bind(w, &self.trie, &self.digests, g, t, layer)
    }

    /// The directories above an entry: the node of its parent, each node
    /// above it, and whether a newer layer hides the entry there.
    ///
    /// A directory whose kind the layer set as a non-directory refuses the
    /// entry, as `extract` refuses a path through a symlink of the same
    /// layer. A directory whose kind the layer did not set takes a mark
    /// at the end of the layer: the older layers must leave a directory
    /// there, or no path.
    fn ancestors(&mut self, digest: &str, layer: u16, path: &str, parts: &[&str]) -> Result<(u32, Vec<u32>, bool)> {
        let mut hidden = self.trie.node(ROOT).opaque > layer;
        let mut cur = ROOT;
        let mut chain = Vec::with_capacity(parts.len());
        for name in &parts[..parts.len() - 1] {
            cur = self.child(cur, name)?;
            let n = self.trie.node(cur);
            let implied = !(hidden || n.removed > layer || n.nondir > layer);
            hidden |= n.removed > layer || n.opaque > layer || n.nondir > layer;
            if implied {
                self.trie.node_mut(cur).implied = true;
            }
            let n = self.trie.node(cur);
            if n.local == layer {
                if n.local_kind == Local::NonDir {
                    let why = format!("{name:?}: a symlink or a file of the same layer");
                    return Err(refuse(digest, path, Refusal::Escapes(why)));
                }
            } else {
                self.pending.push((cur, ROOT));
                self.touched.push(cur);
                let n = self.trie.node_mut(cur);
                (n.local, n.local_kind) = (layer, Local::Dir);
            }
            chain.push(cur);
        }
        Ok((cur, chain, hidden))
    }

    /// After the last layer: a directory entry for each directory that the
    /// layers make with no entry of their own (a file below it) and that
    /// the stream does not hold, as `extract` leaves it: mode 0755, owner
    /// 0, time 0.
    pub(super) fn implied<W: Write>(&mut self, w: &mut Out<'_, W>) -> Result<()> {
        for n in 1..self.trie.nodes() {
            let node = self.trie.node(n);
            if !node.implied || node.emitted != 0 {
                continue;
            }
            let path = self.trie.path(n);
            let head =
                Head { path: &path, kind: Kind::Dir, mode: MADE_MODE, uid: 0, gid: 0, mtime: 0, size: 0, link: None };
            w.head(&head)?;
            self.trie.node_mut(n).emitted = super::IMPLIED;
            self.total.implied += 1;
        }
        Ok(())
    }

    /// A whiteout (`name`) or an opaque marker (no name), with the checks
    /// of [`crate::remove::collect`].
    fn marker(&mut self, digest: &str, layer: u16, path: &str, name: Option<&str>) -> Result<()> {
        self.total.whiteouts += 1;
        let mut parts = safety::components(path).map_err(|r| refuse(digest, path, r))?;
        parts.pop();
        if let Some(n @ ("." | "..")) = name {
            return Err(refuse(digest, path, Refusal::Component(n.to_string())));
        }
        let mut cur = ROOT;
        for p in &parts {
            cur = self.child(cur, p)?;
        }
        let (at, list) = match name {
            Some(n) => (self.child(cur, n)?, List::Removed),
            None => (cur, List::Opaque),
        };
        let node = self.trie.node_mut(at);
        match list {
            List::Removed => node.removed = node.removed.max(layer),
            _ => node.opaque = node.opaque.max(layer),
        }
        self.trie.push(at, list, layer).map_err(|_| super::full(self.max))
    }

    /// The end of the layer `layer`: the kind that the layer left at each
    /// path that it set, against the marks of the newer layers, then the
    /// hard links that wait on these paths, then the marks of this layer.
    pub(super) fn end_layer<W: Write>(&mut self, w: &mut Out<'_, W>, layer: u16) -> Result<()> {
        let touched = std::mem::take(&mut self.touched);
        for &n in &touched {
            let node = self.trie.node(n);
            let (j, e, nondir) = (node.mark_layer, node.mark_entry, node.local_kind == Local::NonDir);
            if j > layer && nondir && !self.trie.reset(n, layer, j) {
                let older = crate::short(&self.digests[usize::from(layer) - 1]);
                let why = format!("{:?}: a symlink or a file of the older layer {older}", self.trie.path(n));
                return Err(refuse(&self.digests[usize::from(j) - 1], &self.trie.path(e), Refusal::Escapes(why)));
            }
            let has_child = node.has_child;
            self.trie.node_mut(n).mark_layer = 0;
            // The paths below a non-directory are gone at the end of this
            // layer, for each mark below it ([`Trie::reset`]).
            if nondir && has_child {
                self.trie.push(n, List::Cut, layer).map_err(|_| super::full(self.max))?;
            }
        }
        self.links.end_layer(w, &self.trie, &self.digests, &touched, layer)?;
        for (n, e) in std::mem::take(&mut self.pending) {
            let node = self.trie.node_mut(n);
            (node.mark_layer, node.mark_entry) = (layer, e);
        }
        self.member = 0;
        Ok(())
    }

    /// After the last layer: the hard links that still wait, with no older
    /// target.
    pub(super) fn finish<W: Write>(&mut self, w: &mut Out<'_, W>) -> Result<()> {
        self.links.finish(w, &self.trie, &self.digests)
    }

    /// One member of a layer read again: a hidden file or symlink that a
    /// hard link of its layer names, under the first name of its inode,
    /// the other names as links to it.
    pub(super) fn reread<W: Write, R: Read>(
        &mut self,
        w: &mut Out<'_, W>,
        digest: &str,
        e: &mut tar::Entry<'_, R>,
        member: &pax::Member,
        names: &[Name],
    ) -> Result<()> {
        let raw = member.path.clone().unwrap_or_else(|| e.path_bytes().to_vec());
        let path = String::from_utf8_lossy(&raw).into_owned();
        let header = e.header().clone();
        let sparse = crate::sparse::layout(member.sparse.as_ref(), e, digest, &path)?;
        let target = header.entry_type().is_symlink().then(|| crate::apply::link_name(e, member));
        let kind = if target.is_some() { Kind::Symlink } else { Kind::File };
        let x = put(w, &self.trie.path(names[0].node), (kind, &header, member), e, sparse.as_ref(), target.as_deref())?;
        self.total.xattrs.add(&x);
        self.total.written += 1;
        self.links.reread_links(w, &self.trie, names)
    }
}

/// One entry of the stream at `path`: the header with the values of the
/// image and the extended attributes that `extract` sets
/// ([`crate::xattr::read`], an ACL in its binary form) as `SCHILY.xattr.`
/// records, then the data of a file (its holes as zero bytes). Gives the
/// attributes written and those named and not written.
fn put<W: Write, R: Read>(
    w: &mut Out<'_, W>,
    path: &str,
    (kind, header, member): (Kind, &tar::Header, &pax::Member),
    e: &mut tar::Entry<'_, R>,
    sparse: Option<&Layout>,
    link: Option<&str>,
) -> Result<xattr::Applied> {
    let size = match (sparse, kind) {
        (Some(l), _) => l.real_size,
        (None, Kind::File) => e.size(),
        _ => 0,
    };
    let wanted = xattr::read(&member.xattrs, path);
    let mut recs = Vec::new();
    for a in &wanted.attrs {
        let key = format!("{}{}", xattr::PAX_PREFIX, a.name);
        pax::push_record(&mut recs, key.as_bytes(), &a.value);
    }
    let head = Head {
        path,
        kind,
        mode: header.mode().unwrap_or(0o644),
        uid: header.uid().unwrap_or(0),
        gid: header.gid().unwrap_or(0),
        mtime: header.mtime().unwrap_or(0),
        size,
        link,
    };
    w.head_with(&head, &recs, true)?;
    match sparse {
        Some(l) => w.data(&mut Expand::new(e, l), size)?,
        None if kind == Kind::File => w.data(e, size)?,
        None => {}
    }
    Ok(xattr::Applied { set: wanted.attrs.len() as u64, unset: wanted.refused })
}
