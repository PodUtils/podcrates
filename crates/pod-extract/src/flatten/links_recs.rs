//! The extended attributes of the hard-link entries (module `links`).
//!
//! `extract` sets the records of a link entry on the inode of its target,
//! after the records of the file entry, so each name of the inode has them.
//! GNU tar with the arguments of incusd does not set a record of a link
//! entry: the records of the links of an inode go with its data, a record of
//! a link over the record of the same name of the file, a newer link over an
//! older one. Where the data went out before the link was read, a record
//! that the data entry does not have is named and not written.

use super::{Links, Name};
use crate::flatten::trie::Trie;
use crate::xattr::{Applied, Unset, Xattr};

/// The reason of a record of a link that the stream does not have.
const AFTER: &str = "a record of a hard-link entry after the data of its inode in the stream";

/// A record of a link entry: its layer and its member, which give the order
/// of the sets of `extract`, and the attribute.
pub(super) type Rec = (u16, u64, Xattr);

/// The records of `recs` in the order of the sets of `extract`: the last set
/// of a name holds.
pub(super) fn union<'a>(recs: impl IntoIterator<Item = &'a Rec>) -> Vec<Xattr> {
    let mut all: Vec<&Rec> = recs.into_iter().collect();
    all.sort_by_key(|r| (r.0, r.1));
    let mut out: Vec<Xattr> = Vec::new();
    for (_, _, x) in all {
        over(&mut out, x);
    }
    out
}

/// Set `x` in `out`: it replaces the attribute of the same name.
pub(in crate::flatten) fn over(out: &mut Vec<Xattr>, x: &Xattr) {
    match out.iter_mut().find(|o| o.name == x.name) {
        Some(o) => o.value.clone_from(&x.value),
        None => out.push(x.clone()),
    }
}

impl Links {
    /// The records that went out with the data at `node`, to compare with
    /// the records of a link that comes after it.
    pub(in crate::flatten) fn put_records(&mut self, node: u32, recs: &[Xattr]) {
        if recs.is_empty() {
            self.recs_out.remove(&node);
        } else {
            self.recs_out.insert(node, recs.to_vec());
        }
    }

    /// The names of `g` go out as links to `lead`, whose data went out
    /// before: each record of the links of `g` that the data entry does not
    /// have is named and not written.
    pub(super) fn unwritten(&mut self, trie: &Trie, g: u32, lead: u32) {
        let gr = &self.groups[g as usize];
        if gr.recs.is_empty() {
            return;
        }
        let out = self.recs_out.get(&lead).map(Vec::as_slice).unwrap_or_default();
        let path = trie.path(gr.names.first().map_or(gr.entry, |n: &Name| n.node));
        let unset: Vec<Unset> = union(&gr.recs)
            .into_iter()
            .filter(|x| !out.contains(x))
            .map(|x| Unset { path: path.clone(), name: x.name, value: Some(x.value), reason: AFTER.into() })
            .collect();
        self.xattrs.add(&Applied { set: 0, unset });
        self.group(g).recs.clear();
    }
}
