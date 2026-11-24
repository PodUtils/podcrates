//! The paths that the layers name, as a trie, with what each layer did at
//! each path.
//!
//! **Memory.** The trie is input of an untrusted image. A node is 32 bytes,
//! its slot in the index about 20 bytes, and its name is in one arena of
//! names (255 bytes at most, [`crate::safety::COMPONENT_MAX`]): about 52
//! bytes and the bytes of the name for each path, and up to twice that
//! while a table grows. The count of paths, with the layers of each marker,
//! is at most the bound that the caller gives
//! ([`super::Options::paths_max`]); the trie refuses the path after it.
//!
//! **The layers.** A layer is a number from 1 (the oldest) up; 0 is "no
//! layer". The pass goes from the newest layer, so a field that holds "the
//! newest layer that did X" gets its value once and keeps it.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher, RandomState};

/// The root of the tree: the empty path.
pub(super) const ROOT: u32 = 0;

/// What the entries of the current layer made of a path, in their order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Local {
    /// A directory: a directory entry, or an entry below it that the
    /// stream does not hold.
    Dir,
    /// A directory with an entry below it that the stream holds.
    Filled,
    /// A file, a symlink or a hard link.
    NonDir,
}

/// The lists of layers that a check after the pass reads. Few paths have
/// one: the markers of the layers, and a non-directory over a path with
/// children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum List {
    /// A whiteout of the path (`.wh.NAME`).
    Removed,
    /// An opaque marker in the directory (`.wh..wh..opq`).
    Opaque,
    /// The last entry of the layer at the path is not a directory, and the
    /// path has children.
    Cut,
}

/// One path. 32 bytes.
#[derive(Clone, Debug)]
pub(super) struct Node {
    parent: u32,
    name_at: u32,
    name_len: u8,
    /// Another node has this node as its parent.
    pub(super) has_child: bool,
    /// An entry of a layer below this path made it a directory, and no
    /// newer layer removed it: `extract` leaves a directory here.
    pub(super) implied: bool,
    pub(super) local_kind: Local,
    /// The newest layer with an entry at this path.
    pub(super) entry: u16,
    /// The newest layer with an entry that is not a directory at this path.
    pub(super) nondir: u16,
    /// The newest layer with a whiteout of this path.
    pub(super) removed: u16,
    /// The newest layer with an opaque marker in this directory.
    pub(super) opaque: u16,
    /// The layer of `local_kind`: the kind is of no other layer.
    pub(super) local: u16,
    /// The layer of the entry of the stream at this path.
    pub(super) emitted: u16,
    /// A newer layer wrote below this path, and needs a directory (or no
    /// path) here at the end of the layer before it: the layer and the
    /// node of its entry. 0: none.
    pub(super) mark_layer: u16,
    pub(super) mark_entry: u32,
}

/// The trie is full: more paths than the bound.
#[derive(Debug)]
pub(super) struct Full;

pub(super) struct Trie {
    nodes: Vec<Node>,
    names: Vec<u8>,
    /// A hash of the parent and the name, to the node. A collision of two
    /// keys goes to the next key.
    index: HashMap<u64, u32>,
    keys: RandomState,
    lists: HashMap<(u32, List), Vec<u16>>,
    items: usize,
    max: usize,
}

impl Node {
    fn new(parent: u32, name_at: u32, name_len: u8) -> Node {
        Node {
            parent,
            name_at,
            name_len,
            has_child: false,
            implied: false,
            local_kind: Local::Dir,
            entry: 0,
            nondir: 0,
            removed: 0,
            opaque: 0,
            local: 0,
            emitted: 0,
            mark_layer: 0,
            mark_entry: 0,
        }
    }
}

impl Trie {
    pub(super) fn new(max: usize) -> Trie {
        Trie {
            nodes: vec![Node::new(ROOT, 0, 0)],
            names: Vec::new(),
            index: HashMap::new(),
            keys: RandomState::new(),
            lists: HashMap::new(),
            items: 1,
            max,
        }
    }

    /// The count of nodes, the root included.
    pub(super) fn nodes(&self) -> u32 {
        self.nodes.len() as u32
    }

    /// The count of paths and list items.
    pub(super) fn len(&self) -> usize {
        self.items
    }

    pub(super) fn node(&self, n: u32) -> &Node {
        &self.nodes[n as usize]
    }

    pub(super) fn parent(&self, n: u32) -> u32 {
        self.nodes[n as usize].parent
    }

    pub(super) fn node_mut(&mut self, n: u32) -> &mut Node {
        &mut self.nodes[n as usize]
    }

    fn name(&self, n: u32) -> &[u8] {
        let node = &self.nodes[n as usize];
        &self.names[node.name_at as usize..node.name_at as usize + usize::from(node.name_len)]
    }

    /// The node of `name` below `parent`, or the free key of the index.
    fn lookup(&self, parent: u32, name: &str) -> Result<u32, u64> {
        let mut h = self.keys.build_hasher();
        h.write_u32(parent);
        h.write(name.as_bytes());
        let mut key = h.finish();
        loop {
            match self.index.get(&key) {
                None => return Err(key),
                Some(&n) if self.nodes[n as usize].parent == parent && self.name(n) == name.as_bytes() => return Ok(n),
                Some(_) => key = key.wrapping_add(1),
            }
        }
    }

    /// The node of `name` below `parent`, made when it is not there. The
    /// name has at most [`crate::safety::COMPONENT_MAX`] bytes.
    pub(super) fn child(&mut self, parent: u32, name: &str) -> Result<u32, Full> {
        let key = match self.lookup(parent, name) {
            Ok(n) => return Ok(n),
            Err(key) => key,
        };
        let name_len = u8::try_from(name.len()).map_err(|_| Full)?;
        let name_at = u32::try_from(self.names.len()).map_err(|_| Full)?;
        if self.items >= self.max || self.names.len() + name.len() > u32::MAX as usize {
            return Err(Full);
        }
        let n = u32::try_from(self.nodes.len()).map_err(|_| Full)?;
        self.names.extend_from_slice(name.as_bytes());
        self.nodes.push(Node::new(parent, name_at, name_len));
        self.nodes[parent as usize].has_child = true;
        self.index.insert(key, n);
        self.items += 1;
        Ok(n)
    }

    /// Add `layer` to a list of `n`.
    pub(super) fn push(&mut self, n: u32, list: List, layer: u16) -> Result<(), Full> {
        if self.items >= self.max {
            return Err(Full);
        }
        self.items += 1;
        self.lists.entry((n, list)).or_default().push(layer);
        Ok(())
    }

    fn any(&self, n: u32, list: List, within: impl Fn(u16) -> bool) -> bool {
        self.lists.get(&(n, list)).is_some_and(|l| l.iter().any(|x| within(*x)))
    }

    /// Did a layer after `lo` remove the path `n` before the entries of the
    /// layer `hi`? A whiteout of the path or of a directory above it in
    /// `lo+1..=hi` (a whiteout applies before the entries of its layer), an
    /// opaque marker above it in `lo+1..=hi`, or a non-directory above it
    /// at the end of a layer in `lo+1..hi`.
    pub(super) fn reset(&self, n: u32, lo: u16, hi: u16) -> bool {
        let (mut cur, mut above) = (n, false);
        loop {
            if self.any(cur, List::Removed, |l| lo < l && l <= hi) {
                return true;
            }
            if above
                && (self.any(cur, List::Opaque, |l| lo < l && l <= hi)
                    || self.any(cur, List::Cut, |l| lo < l && l < hi))
            {
                return true;
            }
            if cur == ROOT {
                return false;
            }
            (cur, above) = (self.nodes[cur as usize].parent, true);
        }
    }

    /// The path of `n`, its components joined by `/`.
    pub(super) fn path(&self, n: u32) -> String {
        let mut parts = Vec::new();
        let mut cur = n;
        while cur != ROOT {
            parts.push(String::from_utf8_lossy(self.name(cur)).into_owned());
            cur = self.nodes[cur as usize].parent;
        }
        parts.reverse();
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_one_node_and_its_name_comes_back() {
        let mut t = Trie::new(16);
        let a = t.child(ROOT, "a").unwrap();
        let b = t.child(a, "b").unwrap();
        assert_eq!((t.child(ROOT, "a").unwrap(), t.child(a, "b").unwrap()), (a, b));
        assert_ne!(t.child(ROOT, "b").unwrap(), b, "the same name below another parent is another node");
        assert_eq!(t.path(b), "a/b");
        assert!(t.node(ROOT).has_child && t.node(a).has_child && !t.node(b).has_child);
        assert_eq!(std::mem::size_of::<Node>(), 32);
    }

    #[test]
    fn the_trie_refuses_the_path_after_its_bound() {
        let mut t = Trie::new(3);
        t.child(ROOT, "a").unwrap();
        t.child(ROOT, "b").unwrap();
        assert!(t.child(ROOT, "c").is_err());
        // A path that is there costs nothing.
        assert!(t.child(ROOT, "a").is_ok());
        assert!(t.push(ROOT, List::Opaque, 1).is_err());
    }

    #[test]
    fn a_reset_is_a_marker_inside_its_layers() {
        let mut t = Trie::new(64);
        let d = t.child(ROOT, "d").unwrap();
        let x = t.child(d, "x").unwrap();
        t.push(d, List::Removed, 4).unwrap();
        t.push(ROOT, List::Opaque, 7).unwrap();
        t.push(d, List::Cut, 5).unwrap();
        // The whiteout of `d` at 4 removes `d/x` for a write of 4 to 6.
        assert!(t.reset(x, 2, 4) && t.reset(d, 3, 6));
        assert!(!t.reset(d, 4, 4), "a whiteout of the layer `lo` is before the entries of `lo`");
        // The cut of `d` at 5 removes `d/x`, not `d`, and only before `hi`.
        assert!(t.reset(x, 4, 6) && !t.reset(x, 4, 5));
        // The opaque marker of the root at 7 removes each path below it.
        assert!(t.reset(d, 6, 7) && !t.reset(ROOT, 6, 7));
    }
}
