//! The modes of the directories after the last layer.
//!
//! A directory is made with the owner bits on, and the permission pass after
//! each layer keeps them, or the next layer cannot write into a directory of
//! mode 0555. So until the last layer is applied, a directory of mode 0555
//! in the image is 0755 in the tree, while the sidecar records 0555. The
//! pass here runs one time, after the last layer, and sets the mode of the
//! image on each directory that the extraction made.

use std::collections::BTreeMap;

use pod_hostprobe::sys;

use crate::error::Result;
use crate::remove::Op;
use crate::safety::{self, Dir};

/// The mode of the image of each directory entry of the extraction, by its
/// path. The last layer that names a directory wins: a later layer can name
/// a directory again with another mode.
#[derive(Debug, Default, Clone)]
pub struct DirModes(BTreeMap<Vec<String>, u32>);

impl DirModes {
    pub fn new() -> DirModes {
        DirModes::default()
    }

    /// Keep the mode of each directory entry of one layer
    /// ([`crate::apply::LayerStats::dir_modes`]).
    pub fn record(&mut self, dirs: &[(Vec<String>, u32)]) {
        for (parts, mode) in dirs {
            self.0.insert(parts.clone(), *mode);
        }
    }

    /// Forget each directory that the whiteouts of a layer removed. Call it
    /// with the whiteouts of a layer before [`DirModes::record`] of the same
    /// layer. A directory that a later entry makes again with no directory
    /// entry (a file below it) is not the directory of the earlier layer,
    /// and it does not get that mode.
    pub fn forget(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Remove { dir, name } => {
                    let mut gone = dir.clone();
                    gone.push(name.clone());
                    self.0.retain(|p, _| !p.starts_with(&gone));
                }
                Op::Opaque { dir } => self.0.retain(|p, _| p.len() <= dir.len() || !p.starts_with(dir)),
            }
        }
    }

    /// The number of directories that the pass sets.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Set the mode of the image on each directory, the deepest first, and
    /// give the number of directories set.
    ///
    /// The deepest first: the walk to a directory opens each directory above
    /// it, and a directory above it that has its mode of the image already
    /// (0311, with no owner read) stops that walk.
    ///
    /// Through a descriptor that `openat` gives with `O_DIRECTORY` and
    /// `O_NOFOLLOW`: `fchmodat` of the name follows a symlink out of the
    /// tree. A path that is no longer a directory is not the directory of
    /// the image, and the pass skips it.
    pub fn set(&self, root: &Dir) -> Result<u64> {
        let mut order: Vec<(&Vec<String>, u32)> = self.0.iter().map(|(p, m)| (p, *m)).collect();
        order.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(b.0)));
        let mut set = 0;
        for (parts, mode) in order {
            let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
            let Ok(Ok((parent, name))) = safety::open_parent(root, &refs, false) else {
                continue;
            };
            let Some(n) = sys::CBuf::new(&name) else {
                continue;
            };
            let flags = sys::O_RDONLY | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
            let Ok(fd) = sys::openat(parent.fd(), &n, flags, 0) else {
                continue;
            };
            if sys::fchmod(fd, u64::from(mode & 0o7777)).is_ok() {
                set += 1;
            }
            let _ = sys::close(fd);
        }
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> Vec<String> {
        p.split('/').map(str::to_string).collect()
    }

    #[test]
    fn the_last_layer_that_names_a_directory_wins() {
        let mut m = DirModes::new();
        m.record(&[(path("d"), 0o755), (path("e"), 0o700)]);
        m.record(&[(path("d"), 0o555)]);
        assert_eq!(m.0.get(&path("d")), Some(&0o555));
        assert_eq!(m.0.get(&path("e")), Some(&0o700));
    }

    #[test]
    fn a_whiteout_forgets_the_directory_and_each_directory_below_it() {
        let mut m = DirModes::new();
        m.record(&[(path("d"), 0o555), (path("d/e"), 0o555), (path("dx"), 0o555), (path("o"), 0o555)]);
        m.record(&[(path("o/p"), 0o555), (path("o/p/q"), 0o555)]);
        m.forget(&[Op::Remove { dir: vec![], name: "d".into() }, Op::Opaque { dir: path("o") }]);
        let left: Vec<&Vec<String>> = m.0.keys().collect();
        assert_eq!(left, [&path("dx"), &path("o")], "an opaque marker keeps its own directory");
    }
}
