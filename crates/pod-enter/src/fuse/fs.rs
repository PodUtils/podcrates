use super::*;

/// A looked-up node: the backing path and its parent's id.
///
/// The kernel resolves `.` and `..` itself and never asks for `/`, so
/// every node arrives through [`Fs::lookup`] below, which is the one
/// place paths are built: containment holds by construction, and `..`
/// answers the parent without touching the filesystem.
struct Node {
    path: PathBuf,
    parent: u64,
}

/// The served tree: the live nodes and the counters.
///
/// Node 1 is the root, pinned: FORGET never drops it. Every other id
/// comes from `next`, and a path looked up twice keeps its id while its
/// lookup count stands, which is the kernel's contract.
pub(super) struct Fs {
    nodes: HashMap<u64, Node>,
    by_path: HashMap<PathBuf, u64>,
    lookups: HashMap<u64, u64>,
    next: u64,
    next_fh: u64,
}

impl Fs {
    pub(super) fn new(root: &Path) -> Fs {
        let mut nodes = HashMap::new();
        nodes.insert(1, Node { path: root.to_path_buf(), parent: 1 });
        Fs { nodes, by_path: HashMap::new(), lookups: HashMap::new(), next: 2, next_fh: 1 }
    }

    /// Look a name up under a node: the entry bytes, or an errno.
    ///
    /// `..` answers the parent's entry without touching the filesystem,
    /// so no lookup can climb past the backing root. A name carrying `/`
    /// or NUL is `ENOENT`: the kernel never sends one, and anything else
    /// did not come from the kernel.
    pub(super) fn lookup(&mut self, parent: u64, name: &[u8]) -> Result<Vec<u8>, sys::Errno> {
        if name.is_empty() || name.contains(&0) || name.contains(&b'/') {
            return Err(errno(2));
        }
        let Some(node) = self.nodes.get(&parent) else {
            return Err(errno(2));
        };
        if name == b".." {
            let grand = node.parent;
            return self.entry_bytes(grand).ok_or(errno(2));
        }
        let text = std::str::from_utf8(name).map_err(|_| errno(2))?;
        let path = node.path.join(text);
        let meta = std::fs::symlink_metadata(&path).map_err(map_io)?;
        let id = match self.by_path.get(&path) {
            Some(id) => *id,
            None => {
                let id = self.next;
                self.next += 1;
                self.nodes.insert(id, Node { path: path.clone(), parent });
                self.by_path.insert(path, id);
                id
            }
        };
        *self.lookups.entry(id).or_insert(0) += 1;
        entry_out(id, &meta)
    }

    /// The entry bytes for a live node, or `None` where it is forgotten.
    pub(super) fn entry_bytes(&self, id: u64) -> Option<Vec<u8>> {
        let node = self.nodes.get(&id)?;
        let meta = std::fs::symlink_metadata(&node.path).ok()?;
        entry_out(id, &meta).ok()
    }

    /// Drop lookups: `FORGET` carries one id and a count, `BATCH_FORGET`
    /// carries pairs. Neither answers: the kernel does not wait.
    pub(super) fn forget(&mut self, id: u64, n: u64) {
        if id == 1 {
            return;
        }
        let left = self.lookups.get(&id).copied().unwrap_or(0).saturating_sub(n);
        if left == 0 {
            self.lookups.remove(&id);
            if let Some(node) = self.nodes.remove(&id) {
                self.by_path.remove(&node.path);
            }
        } else {
            self.lookups.insert(id, left);
        }
    }

    pub(super) fn getattr(&self, id: u64) -> Result<Vec<u8>, sys::Errno> {
        let node = self.nodes.get(&id).ok_or(errno(2))?;
        let meta = std::fs::symlink_metadata(&node.path).map_err(map_io)?;
        attr_out(&meta)
    }

    /// Read a link's target: the bytes the kernel resolves itself.
    pub(super) fn readlink(&self, id: u64) -> Result<Vec<u8>, sys::Errno> {
        let node = self.nodes.get(&id).ok_or(errno(2))?;
        let meta = std::fs::symlink_metadata(&node.path).map_err(map_io)?;
        if !meta.file_type().is_symlink() {
            return Err(errno(22));
        }
        let target = std::fs::read_link(&node.path).map_err(map_io)?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Ok(target.as_os_str().as_bytes().to_vec())
        }
        #[cfg(not(unix))]
        {
            target.into_os_string().into_string().map_err(|_| errno(22)).map(|s| s.into_bytes())
        }
    }

    /// Open a file or a directory for reading: the handle, or an errno.
    ///
    /// Only regular files and directories open. Anything else (a link,
    /// which the kernel resolves itself; a fifo, which would block) is
    /// `EACCES`: present in the listing, never opened.
    pub(super) fn open(&mut self, id: u64, dir: bool) -> Result<Vec<u8>, sys::Errno> {
        let node = self.nodes.get(&id).ok_or(errno(2))?;
        let meta = std::fs::symlink_metadata(&node.path).map_err(map_io)?;
        let ok = if dir { meta.file_type().is_dir() } else { meta.file_type().is_file() };
        if !ok {
            return Err(errno(13));
        }
        let fh = self.next_fh;
        self.next_fh += 1;
        let mut out = vec![0u8; 16];
        put_u64(&mut out, 0, fh);
        put_u32(&mut out, 8, 0);
        put_u32(&mut out, 12, 0);
        Ok(out)
    }

    /// Read file bytes: exactly the asked span, or an errno.
    ///
    /// Exactly the span, never a capped one: a short reply reads as
    /// EOF to the kernel, so capping here would truncate the payload.
    /// The [`MAX_READ`] bound above refuses the oversized ask instead.
    pub(super) fn read(&self, id: u64, offset: u64, size: u64) -> Result<Vec<u8>, sys::Errno> {
        if size > MAX_READ {
            return Err(errno(22));
        }
        let node = self.nodes.get(&id).ok_or(errno(2))?;
        let meta = std::fs::symlink_metadata(&node.path).map_err(map_io)?;
        if meta.file_type().is_dir() {
            return Err(errno(21));
        }
        if !meta.file_type().is_file() {
            return Err(errno(13));
        }
        let mut f = std::fs::File::open(&node.path).map_err(map_io)?;
        use std::io::{Read, Seek, SeekFrom};
        f.seek(SeekFrom::Start(offset)).map_err(map_io)?;
        let mut buf = vec![0u8; size as usize];
        let mut got = 0usize;
        while got < buf.len() {
            match f.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(map_io(e)),
            }
        }
        buf.truncate(got);
        Ok(buf)
    }

    /// List a directory: `.`, `..`, then the entries, each with a
    /// 1-based cookie. Resuming past `offset` skips that many entries,
    /// and a reply never splits an entry: what does not fit waits for
    /// the next ask with a later cookie.
    pub(super) fn readdir(&self, id: u64, offset: u64, size: u64) -> Result<Vec<u8>, sys::Errno> {
        use std::os::unix::fs::MetadataExt as _;
        let node = self.nodes.get(&id).ok_or(errno(2))?;
        let meta = std::fs::symlink_metadata(&node.path).map_err(map_io)?;
        if !meta.file_type().is_dir() {
            return Err(errno(20));
        }
        let self_ino = meta.ino();
        let parent_ino =
            self.nodes.get(&node.parent).and_then(|p| std::fs::symlink_metadata(&p.path).ok()).map_or(1, |m| {
                use std::os::unix::fs::MetadataExt;
                m.ino()
            });
        let mut entries: Vec<(u64, u32, Vec<u8>)> =
            vec![(self_ino, DT_DIR, b".".to_vec()), (parent_ino, DT_DIR, b"..".to_vec())];
        let mut children: Vec<_> = std::fs::read_dir(&node.path).map_err(map_io)?.filter_map(|e| e.ok()).collect();
        // Sorted by name: `read_dir` promises no order, and the resume
        // cookies below are positional, so an unstable order would skip
        // or repeat entries across asks.
        children.sort_by_key(|e| e.file_name());
        for child in children {
            #[cfg(unix)]
            let ino = {
                use std::os::unix::fs::DirEntryExt;
                child.ino()
            };
            #[cfg(not(unix))]
            let ino = 0;
            let dtype = child
                .file_type()
                .map(|t| {
                    if t.is_dir() {
                        DT_DIR
                    } else if t.is_file() {
                        DT_REG
                    } else if t.is_symlink() {
                        DT_LNK
                    } else {
                        DT_UNKNOWN
                    }
                })
                .unwrap_or(DT_UNKNOWN);
            #[cfg(unix)]
            let name = {
                use std::os::unix::ffi::OsStrExt;
                child.file_name().as_os_str().as_bytes().to_vec()
            };
            #[cfg(not(unix))]
            let name = child.file_name().to_string_lossy().into_bytes();
            entries.push((ino, dtype, name));
        }
        let mut out = Vec::new();
        for (index, (ino, dtype, name)) in entries.iter().enumerate() {
            let off = (index as u64) + 1;
            if (index as u64) < offset {
                continue;
            }
            let len = align8(24 + name.len());
            if out.len() + len > size as usize {
                break;
            }
            let mut ent = vec![0u8; len];
            put_u64(&mut ent, 0, *ino);
            put_u64(&mut ent, 8, off);
            put_u32(&mut ent, 16, name.len() as u32);
            put_u32(&mut ent, 20, *dtype);
            ent[24..24 + name.len()].copy_from_slice(name);
            out.extend_from_slice(&ent);
        }
        Ok(out)
    }

    /// Answer the access check against the backing file: 0 where the
    /// mode bits allow it, `EACCES` where they do not. The server runs
    /// as the mounter, so the bits are the verdict.
    pub(super) fn access(&self, id: u64, mask: u32) -> Result<(), sys::Errno> {
        let node = self.nodes.get(&id).ok_or(errno(2))?;
        let meta = std::fs::symlink_metadata(&node.path).map_err(map_io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let mode = meta.mode();
            // Owner bits: the mounter owns the serving, so its word
            // decides. Execute on a directory is search, on a file is
            // exec: the same bits answer both.
            let bits = if meta.uid() == euid_sync() {
                (mode >> 6) & 0o7
            } else if meta.gid() == egid_sync() {
                (mode >> 3) & 0o7
            } else {
                mode & 0o7
            };
            if (mask & 0o7) & !(bits) == 0 { Ok(()) } else { Err(errno(13)) }
        }
        #[cfg(not(unix))]
        {
            let _ = (meta, mask);
            Ok(())
        }
    }
}
