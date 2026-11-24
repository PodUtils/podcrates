//! The end of a layer and the end of the pass for the hard links: each
//! group that waits on a path that the layer set gets its inode, and after
//! the last layer each group that still waits takes the entry of its own
//! layer (module `links`).

use std::collections::BTreeMap;
use std::io::Write;

use super::{Bind, Fallback, Last, Links, Name, Src, put_links};
use crate::error::Result;
use crate::flatten::trie::{Local, ROOT, Trie};
use crate::flatten::write::Out;
use crate::safety::Refusal;

/// The state of a path at the end of a layer.
enum End {
    /// A directory above it is not a directory.
    Cut,
    /// The layer did not set it.
    Gone,
    Dir,
    Src(Src, Option<u32>),
    Chain(u32),
}

impl Links {
    /// The state of `t` at the end of `layer`, which set it.
    fn end(&self, trie: &Trie, t: u32, layer: u16) -> End {
        let mut cur = trie.parent(t);
        while cur != ROOT {
            let n = trie.node(cur);
            if n.local == layer && n.local_kind == Local::NonDir {
                return End::Cut;
            }
            cur = trie.parent(cur);
        }
        let n = trie.node(t);
        if n.local != layer {
            return End::Gone;
        }
        if n.local_kind != Local::NonDir {
            return End::Dir;
        }
        match self.last.get(&t) {
            Some(Last::Data(m, hint)) => End::Src(Src::Hid(layer, *m), *hint),
            Some(Last::Link(h)) => End::Chain(*h),
            None => End::Src(Src::Own(t, layer), Some(t)),
        }
    }

    /// The end of `layer`: the fallback of each group of the layer, then
    /// each group that waits on a path that the layer set.
    pub(in crate::flatten) fn end_layer<W: Write>(
        &mut self,
        w: &mut Out<'_, W>,
        trie: &Trie,
        digests: &[String],
        touched: &[u32],
        layer: u16,
    ) -> Result<()> {
        for g in std::mem::take(&mut self.fresh) {
            if let Bind::Wait { target, older, .. } = self.groups[g as usize].bind {
                if !matches!(self.groups[g as usize].fallback, Fallback::Fail(_)) {
                    self.group(g).fallback = match self.end(trie, target, layer) {
                        End::Cut => {
                            let why = format!("{:?}: a symlink or a file above it", trie.path(target));
                            Fallback::Fail(Refusal::Escapes(why))
                        }
                        End::Gone => Fallback::Missing,
                        End::Dir => Fallback::Fail(Refusal::Link(format!("{:?} is a directory", trie.path(target)))),
                        End::Src(s, hint) => Fallback::Src(s, hint),
                        End::Chain(h) => Fallback::Chain(h),
                    };
                }
                if !older {
                    self.missing(w, trie, digests, g, false)?;
                }
            }
        }
        for &t in touched {
            let Some(list) = self.waiters.remove(&t) else { continue };
            let mut keep = Vec::new();
            for g in list {
                let Bind::Wait { layer: j, .. } = self.groups[g as usize].bind else { continue };
                if j <= layer {
                    keep.push(g);
                    continue;
                }
                if trie.reset(t, layer, j) {
                    self.missing(w, trie, digests, g, false)?;
                    continue;
                }
                match self.end(trie, t, layer) {
                    End::Cut | End::Gone => self.missing(w, trie, digests, g, false)?,
                    End::Dir => {
                        let why = format!("{:?} is a directory", trie.path(t));
                        return Err(self.refusal(trie, digests, g, Refusal::Link(why)));
                    }
                    End::Src(s, hint) => self.settle(w, trie, g, s, hint)?,
                    End::Chain(h) => self.merge(w, trie, digests, g, h)?,
                }
            }
            if !keep.is_empty() {
                self.waiters.insert(t, keep);
            }
        }
        self.last.clear();
        self.leads.clear();
        Ok(())
    }

    /// After the last layer: each group that still waits has no older
    /// target, and its layer decides.
    pub(in crate::flatten) fn finish<W: Write>(
        &mut self,
        w: &mut Out<'_, W>,
        trie: &Trie,
        digests: &[String],
    ) -> Result<()> {
        let mut waiting: Vec<u32> = self.waiters.drain().flat_map(|(_, l)| l).collect();
        waiting.sort_unstable();
        for g in waiting {
            if matches!(self.groups[g as usize].bind, Bind::Wait { .. }) {
                self.missing(w, trie, digests, g, false)?;
            }
        }
        Ok(())
    }

    /// The hidden entries to read again, by layer: each member with the
    /// names of its inode, the first one to get the data.
    pub(in crate::flatten) fn rereads(&self) -> BTreeMap<u16, BTreeMap<u64, Vec<Name>>> {
        let mut out: BTreeMap<u16, BTreeMap<u64, Vec<Name>>> = BTreeMap::new();
        for &g in &self.rereads {
            let gr = &self.groups[g as usize];
            if let (Bind::Reread(Src::Hid(layer, m)), false) = (gr.bind, gr.names.is_empty()) {
                out.entry(layer).or_default().entry(m).or_default().extend(gr.names.iter().copied());
            }
        }
        out
    }

    /// The names after the first of a member read again, as links to it.
    pub(in crate::flatten) fn reread_links<W: Write>(
        &mut self,
        w: &mut Out<'_, W>,
        trie: &Trie,
        names: &[Name],
    ) -> Result<()> {
        self.written += put_links(w, trie, names, names[0].node)?;
        Ok(())
    }
}
