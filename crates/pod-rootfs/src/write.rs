//! Each write of the completion into the root file system of an image.
//!
//! `printf ... > "$R/etc/mtab"` follows the link, and `/etc/mtab` is a
//! symlink in most images. With an absolute target the write lands
//! **outside the root**, on the host. `/etc/resolv.conf ->
//! ../run/systemd/resolve/stub-resolv.conf` has the same shape.
//!
//! So there is one door: [`Root`]. It holds a descriptor of the root and
//! resolves each DIRECTORY component with
//! [`pod_extract::safety::open_parent_with`] under
//! [`pod_extract::safety::Resolve::InRoot`]: `openat2(RESOLVE_IN_ROOT)`
//! follows a symlink but takes the root as `/`, so nothing resolves outside
//! it. The LAST component opens with `O_NOFOLLOW`, and a replacement is an
//! unlink and a create, not a truncate: a truncate through a link is the
//! same escape with another system call.
//!
//! `InRoot`, not the `NO_SYMLINKS` of an extraction. During an extraction
//! a symlink that another entry made is the attack. After it, a root has many
//! legal internal links (`/etc/ssl/certs -> /var/lib/ca-certificates/pem` on
//! openSUSE, `/lib -> usr/lib` on void), and a writer that refuses them does
//! not reach the file that the payload reads.
//!
//! Each open of a file that the crate reads has `O_NONBLOCK`, and the crate
//! reads a regular file only: a fifo or a device of the image at a path of a
//! configuration file does not stop the completion.
//!
//! A root of two layers ([`Root::layers`]) has the same operations: a read
//! sees the upper layer over the lower one, and each write goes to the upper
//! layer ([`layers`]).
//!
//! This module is the only place of the crate that opens a path of the root.

use std::path::{Path, PathBuf};

use pod_extract::safety::{self, Dir, OwnerBits, Resolve};
use pod_hostprobe::sys::{self, CBuf};

use crate::error::{self, Result, cbuf};

/// The largest file that the crate reads. A configuration file of 8 MiB is
/// not one that the crate edits correctly.
pub const READ_LIMIT: usize = 8 * 1024 * 1024;

/// What is at a path, without a follow of the last component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Missing,
    Regular {
        len: u64,
        mode: u32,
    },
    Dir,
    /// The target is kept for the row: "replaced a symlink" with no target
    /// cannot be audited.
    Symlink(String),
    /// A character device, with its numbers: a node `1:5` where `1:3` is
    /// wanted is a wrong answer, not a present one.
    CharDev {
        major: u64,
        minor: u64,
    },
    /// A block device, a socket or a fifo, with its type bits.
    Other(u32),
}

impl Kind {
    pub fn is_missing(&self) -> bool {
        *self == Kind::Missing
    }
}

/// What a write did. `Unchanged` is a result of its own: a fixup that
/// reports a change that it did not make is a false row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    Created,
    Replaced,
    Unchanged,
}

/// What the crate found where it wants a directory.
///
/// Three answers. `/etc/ssl/certs` is a SYMLINK on openSUSE, to a real
/// directory inside the root that the crate must write into; a symlink that
/// points OUT of the root is one that the crate must not follow and must
/// report. A test of the last component alone answers "not a directory" to
/// both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// A directory is there, inside the root.
    Yes,
    /// Nothing is at this path. Quiet: to make `/etc/pki/...` in an image
    /// with no such tree is a guess.
    Absent,
    /// Something is there, and the crate cannot reach a directory through
    /// it. The text says why.
    Unreachable(String),
}

/// A descriptor, closed on drop.
pub(crate) struct Fd(pub(crate) i64);

impl Drop for Fd {
    fn drop(&mut self) {
        let _ = sys::close(self.0);
    }
}

/// A descriptor of the root, and the operations of the completion. With an
/// upper layer, `dir` is the lower layer, and only reads open it.
pub struct Root {
    dir: Dir,
    path: PathBuf,
    upper: Option<layers::Upper>,
}

/// Refuse a path that is not relative to the root: `..`, an absolute path,
/// an empty path, a NUL.
pub(crate) fn check_relative(rel: &str) -> Result<()> {
    safety::components(rel).map(|_| ()).map_err(|r| error::policy(format!("the path {rel:?} is refused: {}", r.why())))
}

impl Root {
    pub fn open(rootfs: &Path) -> Result<Root> {
        let text =
            rootfs.to_str().ok_or_else(|| error::usage(format!("the root {} is not UTF-8", rootfs.display())))?;
        let dir = Dir::open(text)?;
        Ok(Root { dir, path: rootfs.to_path_buf(), upper: None })
    }

    /// A root of two layers, as overlayfs mounts them: each read sees
    /// `upper` over `lower`, and each write goes to `upper`. `lower` is
    /// never opened for write.
    pub fn layers(lower: &Path, upper: &Path) -> Result<Root> {
        let mut root = Root::open(lower)?;
        root.upper = Some(layers::Upper::open(lower, upper)?);
        Ok(root)
    }

    /// The root, or the lower layer of two.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The upper layer, where a root has two.
    pub fn upper(&self) -> Option<&Path> {
        self.upper.as_ref().map(|u| u.path.as_path())
    }

    /// The directory that takes each write: the root, or the upper layer.
    pub fn written(&self) -> &Path {
        self.upper().unwrap_or(&self.path)
    }

    /// The two layers, where the root has them. Each operation asks first.
    fn two(&self) -> Option<layers::Layers<'_>> {
        self.upper.as_ref().map(|upper| layers::Layers { lower: &self.dir, upper })
    }

    pub(crate) fn fd(&self) -> i64 {
        self.dir.fd()
    }

    /// The components of a relative path, or a refusal. Lexical, and only
    /// half of the check: the kernel does the other half.
    pub(crate) fn parts(rel: &str) -> Result<Vec<&str>> {
        safety::components(rel).map_err(|r| error::policy(format!("the path {rel:?} is refused: {}", r.why())))
    }

    /// Open the parent of `rel`, and make the missing directories when
    /// `make_dirs`.
    ///
    /// Three results: `Ok(Ok(..))` reached it; `Ok(Err(why))` did not, and
    /// `why` says why; `Err` is a refusal of the path itself or a refused
    /// system call. A missing ancestor is not an error: `etc/apt/apt.conf.d`
    /// is absent in each root that is not of the Debian family, and a
    /// [`Root::kind`] of a file in it answers "missing".
    pub(crate) fn parent(&self, rel: &str, make_dirs: bool) -> Result<std::result::Result<(Dir, String), String>> {
        let parts = Self::parts(rel)?;
        match safety::open_parent_with(&self.dir, &parts, make_dirs, Resolve::InRoot) {
            Ok(Ok(x)) => Ok(Ok(x)),
            Ok(Err(r)) => Ok(Err(r.why())),
            Err(e) => Ok(Err(e.to_string())),
        }
    }

    /// What is at `rel`, with no follow of the last component.
    pub fn kind(&self, rel: &str) -> Result<Kind> {
        if let Some(l) = self.two() {
            return l.kind(rel);
        }
        let Ok((parent, name)) = self.parent(rel, false)? else {
            return Ok(Kind::Missing);
        };
        let c = cbuf(&name)?;
        let Ok(st) = sys::fstatat(parent.fd(), &c, sys::AT_SYMLINK_NOFOLLOW) else {
            return Ok(Kind::Missing);
        };
        Ok(kind_of(parent.fd(), &c, &st))
    }

    /// Read the regular file at `rel`, with no follow of the last component.
    /// `None`: no file, or not a regular file. A file over [`READ_LIMIT`] is
    /// a failure.
    pub fn read(&self, rel: &str) -> Result<Option<Vec<u8>>> {
        if let Some(l) = self.two() {
            return match l.open_read(rel, false)? {
                Some(fd) => read_regular(&fd, rel),
                None => Ok(None),
            };
        }
        let Ok((parent, name)) = self.parent(rel, false)? else {
            return Ok(None);
        };
        let c = cbuf(&name)?;
        match sys::openat(parent.fd(), &c, READ_FLAGS | sys::O_NOFOLLOW, 0) {
            Ok(fd) => read_regular(&Fd(fd), rel),
            Err(_) => Ok(None),
        }
    }

    /// Read the same way, as text. `None` also for a file that is not UTF-8:
    /// each caller treats a file that it cannot parse as a file that is not
    /// there.
    pub fn read_text(&self, rel: &str) -> Result<Option<String>> {
        Ok(self.read(rel)?.and_then(|b| String::from_utf8(b).ok()))
    }

    /// Write `bytes` to `rel` with `mode`, and replace what is there.
    ///
    /// An unlink first, then a create with `O_EXCL | O_NOFOLLOW`: a truncate
    /// of an existing name follows a symlink as each open does, so "the file
    /// is already there" is the case that escapes.
    pub fn write(&self, rel: &str, bytes: &[u8], mode: u32) -> Result<Wrote> {
        let existing = self.kind(rel)?;
        // The mode is part of "unchanged": a passwd of the image with the
        // right bytes and no write bit still needs the write bit.
        if let Kind::Regular { mode: have, .. } = existing
            && self.read(rel)?.as_deref() == Some(bytes)
        {
            if have == mode {
                return Ok(Wrote::Unchanged);
            }
            self.set_mode(rel, mode)?;
            return Ok(Wrote::Replaced);
        }
        if let Some(l) = self.two() {
            l.replace(rel, bytes, mode)?;
        } else {
            let (parent, name) = match self.parent(rel, true)? {
                Ok(x) => x,
                Err(why) => return Err(not_through(rel, &why)),
            };
            let c = cbuf(&name)?;
            let _open = OwnerBits::add(parent.fd());
            clear(parent.fd(), &c, rel)?;
            create(parent.fd(), &c, rel, bytes, mode)?;
        }
        Ok(match existing {
            Kind::Missing => Wrote::Created,
            _ => Wrote::Replaced,
        })
    }

    /// Set the mode of the regular file at `rel` through a descriptor that
    /// opened with `O_NOFOLLOW`. A `fchmodat` by name follows a symlink that
    /// replaced the file after the check, also to an absolute target on the
    /// host.
    pub(crate) fn set_mode(&self, rel: &str, mode: u32) -> Result<()> {
        if let Some(l) = self.two() {
            return l.set_mode(rel, mode);
        }
        let (parent, name) = match self.parent(rel, false)? {
            Ok(x) => x,
            Err(why) => {
                return Err(error::policy(format!("{rel}: the crate does not change a mode through this path: {why}")));
            }
        };
        let c = cbuf(&name)?;
        let fd = Fd(sys::openat(parent.fd(), &c, READ_FLAGS | sys::O_NOFOLLOW, 0)
            .map_err(|e| error::sys(format!("cannot open {rel} to set its mode"), e))?);
        let st = fstat(&fd, rel)?;
        if st.st_mode & sys::S_IFMT != sys::S_IFREG {
            return Err(error::policy(format!("{rel} is not a regular file; the crate does not change its mode")));
        }
        sys::fchmod(fd.0, mode as u64).map_err(|e| error::sys(format!("cannot set the mode of {rel}"), e))?;
        Ok(())
    }

    /// The mode of the directory at `rel`, each link followed and held in
    /// the root. `None`: no directory that the crate can reach.
    pub fn dir_mode(&self, rel: &str) -> Result<Option<u32>> {
        if let Some(l) = self.two() {
            return l.dir_mode(rel);
        }
        let mut all = Self::parts(rel)?;
        all.push(".");
        let Ok(Ok((dir, _))) = safety::open_parent_with(&self.dir, &all, false, Resolve::InRoot) else {
            return Ok(None);
        };
        let empty = CBuf::new("").ok_or_else(|| error::usage("an empty name"))?;
        Ok(sys::fstatat(dir.fd(), &empty, sys::AT_EMPTY_PATH).ok().map(|st| st.st_mode & 0o7777))
    }

    /// Which of the three [`Reach`] answers is true of `dir`.
    pub fn reach(&self, dir: &str) -> Result<Reach> {
        let kind = self.kind(dir)?;
        if kind.is_missing() {
            return Ok(Reach::Absent);
        }
        Ok(match self.dir_following(dir)? {
            Some(true) => Reach::Yes,
            Some(false) => Reach::Unreachable(match kind {
                Kind::Symlink(t) => format!(
                    "{dir} is a symlink to {t}, which does not resolve to a directory inside this root. The crate \
                     does not write through it, and a payload that reads this path gets nothing"
                ),
                other => format!(
                    "{dir} is {other:?} and not a directory that the crate can reach, so the file that a payload \
                     reads here is not there"
                ),
            }),
            // No `openat2`: the crate cannot ask. Unreachable with that reason,
            // not a guess either way.
            None => Reach::Unreachable(format!(
                "this kernel has no openat2(2), so the crate cannot prove that a write into {dir} stays inside \
                 the root"
            )),
        })
    }
}

/// The flags of each open of a file that the crate reads: no wait on a fifo,
/// no controlling terminal.
pub(crate) const READ_FLAGS: u64 = sys::O_RDONLY | sys::O_NONBLOCK | sys::O_NOCTTY | sys::O_CLOEXEC;

/// The [`Kind`] of the entry `c` of `parent`, from its stat.
pub(crate) fn kind_of(parent: i64, c: &CBuf, st: &sys::Stat) -> Kind {
    match st.st_mode & sys::S_IFMT {
        sys::S_IFREG => Kind::Regular { len: st.st_size as u64, mode: st.st_mode & 0o7777 },
        sys::S_IFDIR => Kind::Dir,
        0o020000 => {
            let (major, minor) = split_dev(st.st_rdev);
            Kind::CharDev { major, minor }
        }
        sys::S_IFLNK => {
            let t = sys::readlinkat(parent, c).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
            Kind::Symlink(t)
        }
        other => Kind::Other(other),
    }
}

/// The refusal of a write whose directory the crate does not reach.
pub(crate) fn not_through(rel: &str, why: &str) -> pod_core::Failure {
    error::policy(format!("{rel}: the crate does not write through this path: {why}"))
}

/// Remove what is at `c` in `parent` before a create: one unlink for a
/// symlink, a dangling symlink and a file. ENOENT is the usual case.
pub(crate) fn clear(parent: i64, c: &CBuf, rel: &str) -> Result<()> {
    match sys::unlinkat(parent, c, 0) {
        Ok(_) => Ok(()),
        Err(e) if e == sys::ENOENT => Ok(()),
        Err(e) if e == sys::EISDIR => Err(error::policy(format!(
            "{rel} is a directory in this image; the crate does not replace a directory with a file"
        ))),
        Err(e) => Err(error::sys(format!("cannot remove the existing {rel}"), e)),
    }
}

/// Make the file `c` in `parent` with `O_EXCL | O_NOFOLLOW`, write `bytes`
/// and set `mode`.
pub(crate) fn create(parent: i64, c: &CBuf, rel: &str, bytes: &[u8], mode: u32) -> Result<()> {
    let flags = sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL | sys::O_NOFOLLOW | sys::O_CLOEXEC;
    let fd = Fd(sys::openat(parent, c, flags, mode as u64).map_err(|e| error::sys(format!("cannot create {rel}"), e))?);
    write_all(&fd, bytes, rel)?;
    // The mode is set after the create: the umask removes bits from the
    // mode of an open, and the payload must be able to write `/etc/passwd`
    // again.
    sys::fchmod(fd.0, mode as u64).map_err(|e| error::sys(format!("cannot set the mode of {rel}"), e))?;
    Ok(())
}

/// The major and the minor number of a device number of the kernel.
pub(crate) fn split_dev(rdev: u64) -> (u64, u64) {
    (((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfffu64), (rdev & 0xff) | ((rdev >> 12) & !0xffu64))
}

pub(crate) fn fstat(fd: &Fd, rel: &str) -> Result<sys::Stat> {
    let empty = CBuf::new("").ok_or_else(|| error::usage("an empty name"))?;
    sys::fstatat(fd.0, &empty, sys::AT_EMPTY_PATH).map_err(|e| error::sys(format!("cannot stat {rel}"), e))
}

/// The bytes of the regular file of `fd`, at most [`READ_LIMIT`]. `None`
/// when it is not a regular file.
pub(crate) fn read_regular(fd: &Fd, rel: &str) -> Result<Option<Vec<u8>>> {
    if fstat(fd, rel)?.st_mode & sys::S_IFMT != sys::S_IFREG {
        return Ok(None);
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 65536];
    loop {
        match sys::read(fd.0, &mut buf) {
            Ok(0) => return Ok(Some(out)),
            Ok(n) => {
                out.extend_from_slice(&buf[..n as usize]);
                if out.len() > READ_LIMIT {
                    return Err(error::policy(format!(
                        "{rel} is larger than {READ_LIMIT} bytes; the crate does not read a configuration file of \
                         that size"
                    )));
                }
            }
            Err(e) if e == sys::EINTR => continue,
            Err(e) => return Err(error::sys(format!("cannot read {rel}"), e)),
        }
    }
}

pub(crate) fn write_all(fd: &Fd, bytes: &[u8], rel: &str) -> Result<()> {
    let mut off = 0;
    while off < bytes.len() {
        match sys::write(fd.0, &bytes[off..]) {
            Ok(0) => return Err(error::policy(format!("cannot write {rel}: the kernel took no byte"))),
            Ok(n) => off += n as usize,
            Err(e) if e == sys::EINTR => continue,
            Err(e) => return Err(error::sys(format!("cannot write {rel}"), e)),
        }
    }
    Ok(())
}

mod follow;
pub mod layers;

pub use layers::{LayerRefusal, layer_refusal};

#[cfg(test)]
mod tests;
