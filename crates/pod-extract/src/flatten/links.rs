//! The hard links of the stream: each inode once, at its first name in
//! the stream, and a link entry after it for each other name.
//!
//! **The model.** A hard link entry of layer `L` names the inode that is at
//! its target at the time of the entry in `extract`: the last entry of the
//! target before it in `L`, else the target at the start of `L` (the last
//! entry of the target in the newest older layer that sets it, when no
//! layer between removed it), else the last entry of the target after it in
//! `L` (the link waits for the end of its layer). An inode is a file or a
//! symlink entry; a chain of links names the inode of its last link.
//!
//! **A group** holds the visible names of one inode, and what they wait
//! for. The pass goes from the newest layer, so a link is read before an
//! older target: its group waits on the target path, and the end of each
//! older layer that sets the path decides it. A file entry of the target
//! goes out under the first name of each group that waits on it, as it is
//! read (with its data): a target that a newer layer hides then needs no
//! second read. A group that a later decision binds to another entry
//! writes its names again (the last entry of a path in the stream holds).
//!
//! **A second read.** The data of a hidden entry that a link of the same
//! layer names after it went by before the link was read: the layer is read
//! again after the last layer, and the entry goes out under the first name
//! of its group ([`Links::rereads`]). The same holds for a file that went
//! out at its own path when a later entry of the path in its layer replaced
//! it there, and a link names its inode through a link with no name in the
//! stream. A layer is read again at most once.

use std::collections::HashMap;
use std::io::Write;

use super::trie::{Local, Trie};
use super::write::{Head, Kind, Out};
use crate::error::{self, Result, refuse};
use crate::safety::Refusal;
use crate::xattr::{self, Xattr};

#[path = "links_end.rs"]
mod end;
#[path = "links_recs.rs"]
mod recs;
use recs::Rec;
pub(super) use recs::over;

/// An inode: the last entry of a visible path in a layer, written at that
/// path, or a hidden entry (layer, member) of a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Src {
    Own(u32, u16),
    Hid(u16, u64),
}

/// The values of the header of a name: the link entry of the image.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ids {
    pub(super) mode: u32,
    pub(super) uid: u64,
    pub(super) gid: u64,
    pub(super) mtime: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Name {
    pub(super) node: u32,
    pub(super) ids: Ids,
}

/// The last entry of a path in the current layer, when it is not a file
/// written at its own path.
#[derive(Clone, Copy, Debug)]
enum Last {
    /// A hidden file or symlink: its member, and a name that got its data.
    Data(u64, Option<u32>),
    Link(u32),
}

#[derive(Clone, Copy, Debug)]
enum Bind {
    /// Waits on `target` as of the start of `layer`; `older`: an older
    /// layer can hold the target (not so for a target at or below the link).
    Wait {
        target: u32,
        layer: u16,
        older: bool,
    },
    /// Each name is in the stream as a name of the inode; `lead` holds it.
    Lead(u32),
    /// No name of the stream holds the inode any more, and its member is
    /// not known: a guard, as each file that goes out at its own path keeps
    /// its member for the layer ([`Links::own`]).
    Lost,
    Reread(Src),
    Merged(u32),
}

/// What the target of a waiting group is at the end of the layer of its
/// link, for the case that no older layer holds it.
#[derive(Clone, Debug)]
enum Fallback {
    Open,
    Missing,
    Src(Src, Option<u32>),
    Chain(u32),
    Fail(Refusal),
}

struct Group {
    names: Vec<Name>,
    bind: Bind,
    fallback: Fallback,
    /// The inode that the names show in the stream now, and its holder.
    shows: Option<(Src, u32)>,
    src: Option<Src>,
    /// The node of the link entry and its layer, for a refusal.
    entry: u32,
    layer: u16,
    /// The records of the link entries of the group (module `recs`).
    recs: Vec<Rec>,
}

#[derive(Default)]
pub(super) struct Links {
    groups: Vec<Group>,
    waiters: HashMap<u32, Vec<u32>>,
    /// The groups that wait from the current layer.
    fresh: Vec<u32>,
    last: HashMap<u32, Last>,
    /// The names that groups wrote with the data of a hidden entry.
    holds: HashMap<u32, Src>,
    /// The visible paths of the current layer that hold the inode of a
    /// group with no name.
    leads: HashMap<u32, Vec<u32>>,
    /// The member of the last file or symlink of the current layer that
    /// went out at its own path: a later entry of the path takes the name
    /// from a group of [`Links::leads`], which then reads it again.
    own: HashMap<u32, u64>,
    rereads: Vec<u32>,
    /// The records that went out with the data of a name of the current
    /// layer, where it had some. A record of a link of an older layer whose
    /// data went out before it is named with no compare.
    recs_out: HashMap<u32, Vec<Xattr>>,
    /// The records of link entries that the stream does not have.
    pub(super) xattrs: xattr::Count,
    /// The link entries of the stream.
    pub(super) written: u64,
}

/// The groups that take one file entry, and where its data goes.
pub(super) struct Plan {
    groups: Vec<u32>,
    /// The name that gets the data when the entry is hidden.
    pub(super) name: Option<Name>,
    /// The records of the links of the groups, which go with the data.
    pub(super) recs: Vec<Xattr>,
}

impl Links {
    fn find(&self, mut g: u32) -> u32 {
        while let Bind::Merged(h) = self.groups[g as usize].bind {
            g = h;
        }
        g
    }

    fn group(&mut self, g: u32) -> &mut Group {
        &mut self.groups[g as usize]
    }

    /// An entry at `node` in `layer` that is not the first of the path in
    /// the layer replaces the earlier one: the earlier link loses the name.
    pub(super) fn replace(&mut self, node: u32, visible: bool) {
        if let Some(Last::Link(g)) = self.last.remove(&node) {
            let g = self.find(g);
            self.group(g).names.retain(|n| n.node != node);
        }
        let own = self.own.remove(&node);
        if visible {
            self.holds.remove(&node);
            for g in self.leads.remove(&node).unwrap_or_default() {
                if matches!(self.groups[g as usize].bind, Bind::Lead(l) if l == node) {
                    let first = self.groups[g as usize].names.first().map(|n| n.node);
                    self.group(g).bind = match (first, self.groups[g as usize].src, own) {
                        (Some(f), ..) => Bind::Lead(f),
                        // No name of the stream holds the inode after this
                        // entry: a later link to the group reads the member
                        // again after the last layer.
                        (None, Some(Src::Own(_, layer)), Some(m)) => {
                            let src = Src::Hid(layer, m);
                            self.group(g).src = Some(src);
                            self.rereads.push(g);
                            Bind::Reread(src)
                        }
                        (None, ..) => Bind::Lost,
                    };
                }
            }
        }
    }

    /// The groups that wait on `target` and take its file entry of `layer`.
    pub(super) fn plan(&self, trie: &Trie, target: u32, layer: u16, visible: bool) -> Plan {
        let mut groups = Vec::new();
        for &g in self.waiters.get(&target).into_iter().flatten() {
            // A group of this layer (its link came before the entry) takes
            // the entry only when no older layer holds the target, which
            // the end of the pass decides (`Fallback`).
            if let Bind::Wait { layer: j, older: true, .. } = self.groups[g as usize].bind
                && j > layer
                && !trie.reset(target, layer, j)
            {
                groups.push(g);
            }
        }
        let first = groups.iter().flat_map(|g| self.groups[*g as usize].names.first()).next();
        let recs = recs::union(groups.iter().flat_map(|g| &self.groups[*g as usize].recs));
        Plan { name: if visible { None } else { first.copied() }, groups, recs }
    }

    /// After the file entry (`member`) of `target` went out at its own
    /// path (`visible`) or under `plan.name`: the other names as links.
    pub(super) fn wrote<W: Write>(
        &mut self,
        w: &mut Out<'_, W>,
        trie: &Trie,
        plan: &Plan,
        target: u32,
        layer: u16,
        member: u64,
    ) -> Result<()> {
        let (src, lead) = match plan.name {
            Some(n) => {
                self.last.insert(target, Last::Data(member, Some(n.node)));
                (Src::Hid(layer, member), n.node)
            }
            None => {
                self.own.insert(target, member);
                (Src::Own(target, layer), target)
            }
        };
        for &g in &plan.groups {
            self.links(w, trie, g, lead, src)?;
            self.group(g).shows = Some((src, lead));
        }
        Ok(())
    }

    /// A hidden file entry that no group takes.
    pub(super) fn passed(&mut self, target: u32, member: u64) {
        self.last.insert(target, Last::Data(member, None));
    }

    /// Write each name of `g` but `lead` as a link to `lead`.
    fn links<W: Write>(&mut self, w: &mut Out<'_, W>, trie: &Trie, g: u32, lead: u32, src: Src) -> Result<()> {
        if let Src::Hid(..) = src {
            for n in &self.groups[g as usize].names {
                self.holds.insert(n.node, src);
            }
        }
        self.written += put_links(w, trie, &self.groups[g as usize].names, lead)?;
        Ok(())
    }

    /// A link entry at `node` of `layer` (its `member`, with the records
    /// `attrs`): a new group, bound now when its target is set in the layer
    /// before it, else waiting.
    pub(super) fn link(
        &mut self,
        (node, layer, member): (u32, u16, u64),
        name: Option<Name>,
        attrs: Vec<Xattr>,
    ) -> u32 {
        let g = self.groups.len() as u32;
        let names = name.into_iter().collect();
        let wait = Bind::Wait { target: node, layer, older: false };
        let fallback = Fallback::Open;
        let recs = attrs.into_iter().map(|x| (layer, member, x)).collect();
        let group = Group { names, bind: wait, fallback, shows: None, src: None, entry: node, layer, recs };
        self.groups.push(group);
        self.last.insert(node, Last::Link(g));
        g
    }

    /// The target of `g` is at or below its own path: the link removed it.
    pub(super) fn doomed(&mut self, g: u32, why: String) {
        self.group(g).fallback = Fallback::Fail(Refusal::Link(why));
        self.fresh.push(g);
    }

    /// Bind `g` to `target` at the time of its entry in `layer`.
    pub(super) fn bind<W: Write>(
        &mut self,
        w: &mut Out<'_, W>,
        trie: &Trie,
        digests: &[String],
        g: u32,
        target: u32,
        layer: u16,
    ) -> Result<()> {
        let n = trie.node(target);
        if n.local != layer {
            self.group(g).bind = Bind::Wait { target, layer, older: true };
            self.waiters.entry(target).or_default().push(g);
            self.fresh.push(g);
            return Ok(());
        }
        if n.local_kind != Local::NonDir {
            return Err(self.refusal(
                trie,
                digests,
                g,
                Refusal::Link(format!("{:?} is a directory", trie.path(target))),
            ));
        }
        match self.last.get(&target).copied() {
            Some(Last::Data(m, hint)) => self.settle(w, trie, g, Src::Hid(layer, m), hint),
            Some(Last::Link(h)) => self.merge(w, trie, digests, g, h),
            None => self.settle(w, trie, g, Src::Own(target, layer), Some(target)),
        }
    }

    fn refusal(&self, trie: &Trie, digests: &[String], g: u32, r: Refusal) -> pod_core::Failure {
        let gr = &self.groups[g as usize];
        refuse(&digests[usize::from(gr.layer) - 1], &trie.path(gr.entry), r)
    }

    /// The final inode of `g`, and a name that holds it in the stream (the
    /// name of a hidden entry holds it while no write replaced it). The
    /// names go out as links to it; with no holder the layer of the entry
    /// is read again.
    fn settle<W: Write>(
        &mut self,
        w: &mut Out<'_, W>,
        trie: &Trie,
        g: u32,
        src: Src,
        holder: Option<u32>,
    ) -> Result<()> {
        let holds = &self.holds;
        let held = |l: &u32| match src {
            Src::Own(..) => true,
            Src::Hid(..) => holds.get(l) == Some(&src),
        };
        let shown = self.groups[g as usize].shows.filter(|(s, l)| *s == src && held(l));
        let holder = holder.filter(held);
        self.group(g).src = Some(src);
        let first = self.groups[g as usize].names.first().map(|n| n.node);
        let lead = match shown {
            Some((_, l)) => l,
            _ => {
                let Some(l) = holder else {
                    self.group(g).bind = Bind::Reread(src);
                    self.rereads.push(g);
                    return Ok(());
                };
                self.links(w, trie, g, l, src)?;
                self.unwritten(trie, g, l);
                l
            }
        };
        self.group(g).bind = Bind::Lead(first.unwrap_or(lead));
        if first.is_none() && matches!(src, Src::Own(..)) {
            self.leads.entry(lead).or_default().push(g);
        }
        Ok(())
    }

    /// `g` names the inode of the link of `h`: a group that is bound gives
    /// its inode, and the names of `g` join a group that waits.
    fn merge<W: Write>(&mut self, w: &mut Out<'_, W>, trie: &Trie, digests: &[String], g: u32, h: u32) -> Result<()> {
        let h = self.find(h);
        if h == g {
            return self.missing(w, trie, digests, g, true);
        }
        match self.groups[h as usize].bind {
            Bind::Lost => Err(error::unsupported(format!(
                "the hard link {:?} names an inode that no name of the stream holds any more (a second entry of \
                 its path in one layer); flatten cannot write it",
                trie.path(self.groups[g as usize].entry)
            ))),
            Bind::Lead(l) => {
                let src = self.groups[h as usize].src.unwrap_or(Src::Own(l, 0));
                self.settle(w, trie, g, src, Some(l))
            }
            _ => {
                let names = std::mem::take(&mut self.group(g).names);
                let recs = std::mem::take(&mut self.group(g).recs);
                self.group(g).bind = Bind::Merged(h);
                self.group(h).shows = None;
                self.group(h).names.extend(names);
                self.group(h).recs.extend(recs);
                Ok(())
            }
        }
    }

    /// No older layer holds the target of `g` at the start of its layer:
    /// the end of its layer decides.
    fn missing<W: Write>(
        &mut self,
        w: &mut Out<'_, W>,
        trie: &Trie,
        digests: &[String],
        g: u32,
        cycle: bool,
    ) -> Result<()> {
        let fallback = if cycle { Fallback::Missing } else { self.groups[g as usize].fallback.clone() };
        let path = |t: u32| trie.path(t);
        match fallback {
            Fallback::Open | Fallback::Missing => {
                let t = match self.groups[g as usize].bind {
                    Bind::Wait { target, .. } => path(target),
                    _ => String::new(),
                };
                let why = format!("{t:?}: {}", crate::safety::LINK_MISSING);
                Err(self.refusal(trie, digests, g, Refusal::Link(why)))
            }
            Fallback::Fail(r) => Err(self.refusal(trie, digests, g, r)),
            Fallback::Src(s, hint) => self.settle(w, trie, g, s, hint),
            Fallback::Chain(h) => self.merge(w, trie, digests, g, h),
        }
    }
}

/// Each name but `lead` as a link entry that names `lead`, with the values
/// of the header of its own link entry. Gives the count.
fn put_links<W: Write>(w: &mut Out<'_, W>, trie: &Trie, names: &[Name], lead: u32) -> Result<u64> {
    let to = trie.path(lead);
    let mut count = 0;
    for n in names.iter().filter(|n| n.node != lead) {
        let (path, i) = (trie.path(n.node), n.ids);
        let link = Some(to.as_str());
        let head =
            Head { path: &path, kind: Kind::Link, mode: i.mode, uid: i.uid, gid: i.gid, mtime: i.mtime, size: 0, link };
        w.head(&head)?;
        count += 1;
    }
    Ok(count)
}
