//! The entries that are not of the image: the directories of the prefix,
//! and the entries of the caller before and after the tree
//! ([`super::flatten_with`]).
//!
//! An entry of the caller has the path that the caller gives, with no
//! prefix, the owner 0 and the time 0. A path of the caller that the
//! stream holds for the image (below the prefix), or that is below a file
//! or a symlink of the image, is refused, not merged: a reader of the
//! stream would put one over the other, or write through the symlink. An
//! entry before the tree is checked after the tree, when the paths of the
//! image are known.

use std::io::{Read, Write};

use pod_core::Failure;

use super::trie::{ROOT, Trie};
use super::write::{Head, Kind, Out};
use crate::error::{self, Result};
use crate::safety;

/// The writer of the entries of the caller, over the stream of
/// [`super::flatten_with`].
pub struct Entries<'a, 'b, W: Write> {
    pub(super) out: &'a mut Out<'b, W>,
    pub(super) caller: &'a mut Caller,
    /// The paths of the image, for the entries after the tree.
    pub(super) image: Option<&'a Trie>,
}

/// The paths of the caller, and the prefix.
#[derive(Default)]
pub(super) struct Caller {
    pub(super) prefix: Vec<String>,
    /// The paths written before the tree.
    before: Vec<String>,
    pub(super) count: u64,
}

impl<W: Write> Entries<'_, '_, W> {
    /// A directory with `mode`.
    pub fn dir(&mut self, path: &str, mode: u32) -> Result<()> {
        self.put(path, Kind::Dir, mode, None, None)
    }

    /// A file with `mode` and the bytes `data`.
    pub fn file(&mut self, path: &str, mode: u32, data: &[u8]) -> Result<()> {
        self.file_from(path, mode, data.len() as u64, &mut &data[..])
    }

    /// A file with `mode` and `size` bytes of `data`; a reader that ends
    /// first fails the stream.
    pub fn file_from(&mut self, path: &str, mode: u32, size: u64, data: &mut dyn Read) -> Result<()> {
        self.put(path, Kind::File, mode, Some((size, data)), None)
    }

    /// A symlink to `target`, stored with no change.
    pub fn symlink(&mut self, path: &str, target: &str) -> Result<()> {
        self.put(path, Kind::Symlink, 0o777, None, Some(target))
    }

    fn put(
        &mut self,
        path: &str,
        kind: Kind,
        mode: u32,
        data: Option<(u64, &mut dyn Read)>,
        link: Option<&str>,
    ) -> Result<()> {
        let parts = safety::components(path).map_err(|r| refused(path, &r.why()))?;
        let rel = parts.join("/");
        if let Some(why) = self.caller.collides(self.image, &parts) {
            return Err(refused(path, &why));
        }
        if self.image.is_none() {
            self.caller.before.push(rel.clone());
        }
        let size = data.as_ref().map_or(0, |d| d.0);
        let head = Head { path: &rel, kind, mode, uid: 0, gid: 0, mtime: 0, size, link };
        self.out.head_with(&head, &[], false)?;
        if let Some((size, r)) = data {
            self.out.data(r, size)?;
        }
        self.caller.count += 1;
        Ok(())
    }
}

impl Caller {
    /// The directory entry of each component of the prefix, first.
    pub(super) fn prefix_dirs<W: Write>(&mut self, out: &mut Out<'_, W>, mode: u32) -> Result<()> {
        for i in 1..=self.prefix.len() {
            let path = self.prefix[..i].join("/");
            let head = Head { path: &path, kind: Kind::Dir, mode, uid: 0, gid: 0, mtime: 0, size: 0, link: None };
            out.head_with(&head, &[], false)?;
            self.count += 1;
        }
        Ok(())
    }

    /// The entries of the caller before the tree, against the paths of
    /// the image.
    pub(super) fn check_before(&self, image: &Trie) -> Result<()> {
        for path in &self.before {
            let parts: Vec<&str> = path.split('/').collect();
            if let Some(why) = self.collides(Some(image), &parts) {
                return Err(refused(path, &why));
            }
        }
        Ok(())
    }

    /// Why the path `parts` of the caller is refused: a directory of the
    /// prefix, a path of the image in the stream, or a path below a file
    /// or a symlink of the image.
    fn collides(&self, image: Option<&Trie>, parts: &[&str]) -> Option<String> {
        let n = self.prefix.len().min(parts.len());
        if self.prefix[..n].iter().zip(parts).any(|(a, b)| a != b) {
            return None;
        }
        if parts.len() <= self.prefix.len() {
            return Some("a directory of the prefix, which flatten writes".into());
        }
        let trie = image?;
        let rest = &parts[self.prefix.len()..];
        let mut cur = ROOT;
        for (i, name) in rest.iter().enumerate() {
            cur = trie.lookup_child(cur, name)?;
            let node = trie.node(cur);
            let shown = node.emitted != 0;
            let nondir = shown && node.emitted != super::IMPLIED && node.nondir == node.emitted;
            if shown && i + 1 == rest.len() {
                return Some("a path of the image".into());
            }
            if nondir {
                return Some(format!("below {:?}, a file or a symlink of the image", rest[..=i].join("/")));
            }
        }
        None
    }
}

fn refused(path: &str, why: &str) -> Failure {
    error::usage(format!("the entry of the caller {path:?} is refused: {why}"))
}
