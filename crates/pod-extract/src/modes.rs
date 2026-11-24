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

use crate::error::{self, Result};
use crate::remove::Op;
use crate::safety::{self, Dir};
use crate::sidecar::Sidecar;
use crate::tree::Walk;

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

    /// Keep the mode of each directory entry of one layer, and forget each
    /// directory that an entry of the layer that is not a directory
    /// replaced, in the order of the layer. A directory that a later entry
    /// makes again with no directory entry does not get the mode of the
    /// replaced one.
    pub fn layer(&mut self, st: &crate::apply::LayerStats) {
        let mut gone = st.replaced.iter().peekable();
        for i in 0..=st.dir_modes.len() {
            while let Some((_, path)) = gone.next_if(|(at, _)| *at == i) {
                let below: Vec<Vec<String>> =
                    self.0.range(path.clone()..).map(|(k, _)| k).take_while(|k| k.starts_with(path)).cloned().collect();
                for k in below {
                    self.0.remove(&k);
                }
            }
            if let Some((parts, mode)) = st.dir_modes.get(i) {
                self.0.insert(parts.clone(), *mode);
            }
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
    ///
    /// The mode is read back: the kernel drops `S_ISGID` with no error where
    /// the group of the directory is not a group of the process. A mode
    /// that the kernel did not keep goes into `sidecar` as one more row of
    /// the directory, with `applied.mode`.
    pub fn set(&self, root: &Dir, sidecar: &mut Sidecar) -> Result<u64> {
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
            let want = mode & 0o7777;
            if sys::fchmod(fd, u64::from(want)).is_ok() {
                set += 1;
            }
            let kept = sys::fstatat(fd, &sys::cempty(), sys::AT_EMPTY_PATH).map(|st| st.st_mode & 0o7777);
            let _ = sys::close(fd);
            if let Ok(k) = kept
                && k != want
            {
                sidecar.kept_dir(&parts.join("/"), k).map_err(error::io)?;
            }
        }
        Ok(set)
    }
}

/// Add the owner bits to each directory of the tree that an earlier
/// extraction left at the destination, before the first layer. Give the
/// number of directories changed.
///
/// That tree has the modes of its image: a directory with no owner read
/// (0311) does not open for the walk of a later entry, and a closed parent
/// (0555) that the new layers do not name refuses each entry below it. A
/// later layer that names a directory sets its mode again, and the pass
/// after the last layer sets the mode of the image.
///
/// Each directory opens with `O_NOFOLLOW`, as the removal opens one:
/// a symlink of the old tree is not followed out of it. The walk holds one
/// directory open at a time ([`crate::tree`]), and a tree deeper than
/// [`crate::tree::MAX_DEPTH`] is refused with the cause
/// [`crate::tree::TooDeep`].
pub fn open_tree(root: &Dir) -> Result<u64> {
    open_tree_to(root, crate::tree::MAX_DEPTH)
}

/// [`open_tree`] with the depth limit as a parameter, for the tests.
pub(crate) fn open_tree_to(root: &Dir, limit: usize) -> Result<u64> {
    let mut changed = 0;
    let mut w = Walk::of(root.fd(), limit)?;
    loop {
        let Some(e) = w.next() else {
            if w.at_start() {
                return Ok(changed);
            }
            w.ascend()?;
            continue;
        };
        if e.d_type != sys::DT_DIR && e.d_type != sys::DT_UNKNOWN {
            continue;
        }
        let before = sys::CBuf::from_bytes(&e.name)
            .and_then(|n| sys::fstatat(w.fd(), &n, sys::AT_SYMLINK_NOFOLLOW).ok())
            .map(|st| st.st_mode & 0o7777);
        if w.descend(&e.name, crate::remove::open_closed)? && before.is_some_and(|m| m & 0o700 != 0o700) {
            changed += 1;
        }
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
