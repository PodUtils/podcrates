//! The decision for each entry of a layer, and the check of the marks at
//! the end of each layer.

use std::io::{Read, Write};

use super::Flattened;
use super::trie::{List, Local, ROOT, Trie};
use super::write::{Expand, Head, Kind, Out};
use crate::error::{self, Result, refuse};
use crate::safety::{self, Refusal};
use crate::whiteout::{self, Kind as Marker};
use crate::{Layer, pax};

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
    /// the layer was not known yet, with the node of the entry.
    pending: Vec<(u32, u32)>,
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
        if hidden {
            self.total.hidden += 1;
            return Ok(());
        }

        let (wkind, link) = match &target {
            Some(t) if kind.is_symlink() => (Kind::Symlink, Some(t.clone())),
            Some(t) => {
                let ok = self.trie.find(&link_parts).map(|n| self.trie.node(n));
                if !ok.is_some_and(|n| n.emitted == layer && n.emitted_file) {
                    return Err(error::unsupported(format!(
                        "layer {digest}: the hard link {path:?} names {t:?}, which is not a file of the same layer \
                         before it; flatten does not write that hard link yet"
                    )));
                }
                (Kind::Link, Some(link_parts.join("/")))
            }
            None if class == Local::Dir => (Kind::Dir, None),
            None => (Kind::File, None),
        };
        let size = match (&sparse, wkind) {
            (Some(l), _) => l.real_size,
            (None, Kind::File) => e.size(),
            _ => 0,
        };
        let rel = parts.join("/");
        let head = Head {
            path: &rel,
            kind: wkind,
            mode: header.mode().unwrap_or(0o644),
            uid: header.uid().unwrap_or(0),
            gid: header.gid().unwrap_or(0),
            mtime: header.mtime().unwrap_or(0),
            size,
            link: link.as_deref(),
        };
        w.head(&head)?;
        match &sparse {
            Some(l) => w.data(&mut Expand::new(e, l), size)?,
            None if wkind == Kind::File => w.data(e, size)?,
            None => {}
        }
        self.total.written += 1;
        let n = self.trie.node_mut(leaf);
        (n.emitted, n.emitted_file) = (layer, wkind == Kind::File);
        for a in chain {
            let n = self.trie.node_mut(a);
            if n.local_kind == Local::Dir {
                n.local_kind = Local::Filled;
            }
        }
        Ok(())
    }

    /// The directories above an entry: the node of its parent, each node
    /// above it, and whether a newer layer hides the entry there.
    ///
    /// A directory whose kind the layer set as a non-directory refuses the
    /// entry, as `extract` refuses a path through a symlink of the same
    /// layer. A directory whose kind the layer did not set yet takes a mark
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
                Head { path: &path, kind: Kind::Dir, mode: 0o755, uid: 0, gid: 0, mtime: 0, size: 0, link: None };
            w.head(&head)?;
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
    /// marks of this layer.
    pub(super) fn end_layer(&mut self, layer: u16) -> Result<()> {
        for n in std::mem::take(&mut self.touched) {
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
        for (n, e) in std::mem::take(&mut self.pending) {
            let node = self.trie.node_mut(n);
            (node.mark_layer, node.mark_entry) = (layer, e);
        }
        Ok(())
    }
}
