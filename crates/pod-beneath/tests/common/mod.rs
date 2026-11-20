//! The tree and the calls of the tests.
#![allow(dead_code)]

use pod_beneath::{Calls, Libc, OpenHow};
use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::os::raw::c_int;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

pub const ENOENT: c_int = 2;
pub const EAGAIN: c_int = 11;
pub const ENOTDIR: c_int = 20;
pub const EXDEV: c_int = 18;
pub const ENOSYS: c_int = 38;
pub const ELOOP: c_int = 40;

/// A scratch tree, removed when it goes:
///
/// ```text
/// base/w/file ("inside"), base/w/sub/f, base/w/rel -> file
/// base/w2/f
/// base/work/sub, base/file
/// base/away -> OUT (absolute), base/near -> base/work (absolute, in the base)
/// out/f ("OUTSIDE"), out/sub/f
/// ```
pub struct Tree {
    pub dir: PathBuf,
    pub base: PathBuf,
    pub out: PathBuf,
}

impl Tree {
    pub fn new(name: &str) -> Tree {
        let dir = std::env::temp_dir().join(format!("pod-beneath-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (base, out) = (dir.join("base"), dir.join("out"));
        for d in ["base/w/sub", "base/w2", "base/work/sub", "out/sub"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::write(base.join("w/file"), b"inside").unwrap();
        std::fs::write(base.join("w/sub/f"), b"inside").unwrap();
        std::fs::write(base.join("w2/f"), b"inside").unwrap();
        std::fs::write(base.join("file"), b"inside").unwrap();
        std::fs::write(out.join("f"), b"OUTSIDE").unwrap();
        std::fs::write(out.join("sub/f"), b"OUTSIDE").unwrap();
        symlink("file", base.join("w/rel")).unwrap();
        symlink(&out, base.join("away")).unwrap();
        symlink(base.join("work"), base.join("near")).unwrap();
        Tree { dir, base, out }
    }

    /// The bytes of the base.
    pub fn b(&self) -> Vec<u8> {
        self.base.as_os_str().as_bytes().to_vec()
    }

    /// The bytes of `BASE/rel`.
    pub fn at(&self, rel: &str) -> Vec<u8> {
        let mut p = self.b();
        p.push(b'/');
        p.extend_from_slice(rel.as_bytes());
        p
    }

    /// Replace the directory `BASE/rel` by a link to `to`, as a process of
    /// the host does.
    pub fn link_over(&self, rel: &str, to: &Path) {
        std::fs::remove_dir_all(self.base.join(rel)).unwrap();
        symlink(to, self.base.join(rel)).unwrap();
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The text of the file of an `O_PATH` descriptor, read through its
/// magic link.
pub fn text_of(fd: c_int) -> String {
    std::fs::read_to_string(format!("/proc/self/fd/{fd}")).unwrap_or_default()
}

/// [`Libc`], and the count of each open and each close.
#[derive(Default)]
pub struct Count {
    pub opens: Cell<i32>,
    pub closes: Cell<i32>,
}

impl Count {
    /// The descriptors that the walk gave and did not close.
    pub fn held(&self) -> i32 {
        self.opens.get() - self.closes.get()
    }
}

impl Calls for Count {
    fn openat2(&self, dirfd: c_int, path: &CStr, how: &OpenHow) -> Result<c_int, c_int> {
        let r = Libc.openat2(dirfd, path, how);
        if r.is_ok() {
            self.opens.set(self.opens.get() + 1);
        }
        r
    }

    fn close(&self, fd: c_int) {
        self.closes.set(self.closes.get() + 1);
        Libc.close(fd)
    }
}

/// [`Libc`], with a change of the tree just before the first walk under a
/// descriptor: a process of the host that wins the race between a check
/// and the call.
pub struct Race {
    pub change: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl Race {
    pub fn new(change: impl FnOnce() + 'static) -> Race {
        Race { change: RefCell::new(Some(Box::new(change))) }
    }
}

impl Calls for Race {
    fn openat2(&self, dirfd: c_int, path: &CStr, how: &OpenHow) -> Result<c_int, c_int> {
        if how.resolve == pod_beneath::BENEATH
            && let Some(change) = self.change.borrow_mut().take()
        {
            change();
        }
        Libc.openat2(dirfd, path, how)
    }

    fn close(&self, fd: c_int) {
        Libc.close(fd)
    }
}

/// A kernel that answers each `openat2` with one errno, or that answers
/// the walk under a descriptor alone with it (`walk_only`).
pub struct Answer {
    pub errno: c_int,
    pub walk_only: bool,
    pub calls: Cell<usize>,
    pub closes: Cell<usize>,
}

impl Answer {
    pub fn new(errno: c_int, walk_only: bool) -> Answer {
        Answer { errno, walk_only, calls: Cell::new(0), closes: Cell::new(0) }
    }
}

impl Calls for Answer {
    fn openat2(&self, dirfd: c_int, path: &CStr, how: &OpenHow) -> Result<c_int, c_int> {
        self.calls.set(self.calls.get() + 1);
        if self.walk_only && how.resolve == 0 {
            return Libc.openat2(dirfd, path, how);
        }
        Err(self.errno)
    }

    fn close(&self, fd: c_int) {
        self.closes.set(self.closes.get() + 1);
        Libc.close(fd)
    }
}
