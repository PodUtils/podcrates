//! Every write the completion layer makes into somebody else's rootfs.
//!
//! ⭐ **[`TODO/complete.md`](../../../TODO/complete.md) T-0405 is this module,
//! not a special case for `/etc/mtab`.** `printf ... > "$R/etc/mtab"` follows
//! the link, and `/etc/mtab` is a symlink in most images. If its target is
//! absolute the write lands **outside the rootfs**, on the host. The same shape
//! is `/etc/resolv.conf -> ../run/systemd/resolve/stub-resolv.conf`, and the
//! next one nobody has met yet.
//!
//! So there is one door: [`Root`], which holds a descriptor on the rootfs and
//! resolves every DIRECTORY component through
//! [`podvm_extract::safety::open_parent_with`] under
//! [`podvm_extract::safety::Resolve::InRoot`] -- `openat2(RESOLVE_IN_ROOT)`,
//! which follows a symlink but treats the rootfs as `/`, so nothing resolves
//! outside it. The FINAL component is opened `O_NOFOLLOW`, and a replacement is
//! **unlink-then-create** rather than a truncate, because truncating through a
//! link is the same escape with a different syscall.
//!
//! ⭐ **`InRoot` and not extraction's `NO_SYMLINKS`, and the difference is the
//! whole of this module's correctness.** During extraction a symlink another
//! entry created is the attack. Afterwards, a finished rootfs is full of
//! legitimate internal links -- `/etc/ssl/certs -> /var/lib/ca-certificates/pem`
//! on openSUSE, `/lib -> usr/lib` on void -- and a writer that refuses those
//! cannot reach the file the payload will read. ⚠ Measured on 2026-09-09 by
//! `experiments/240-distro-sweep.sh`: under `NO_SYMLINKS` the CA-bundle fixup
//! reported `leaves the destination` on openSUSE and the libc probe read
//! `unknown` on void, and neither link leaves the rootfs.
//!
//! ⛔ This module is the only place in `podvm-complete` that opens anything by
//! path. A second one would be the one that forgets.

use podvm_extract::safety::{self, Dir, Refusal, Resolve};
use podvm_probe::sys::{self, CBuf};

use crate::{Error, Result};

/// What is at a path, without following the last component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Missing,
    Regular {
        len: u64,
        mode: u32,
    },
    Dir,
    /// ⚠ The target is kept because the diagnostic needs it: "replaced a
    /// symlink" without saying where it pointed is a line nobody can audit.
    Symlink(String),
    /// ⭐ A character device, with the numbers it carries. T-0401 needs them:
    /// a node that is there but is `1:5` where `1:3` was wanted is a wrong
    /// answer rather than a present one.
    CharDev {
        major: u64,
        minor: u64,
    },
    /// A block device, socket or fifo.
    Other(u32),
}

impl Kind {
    pub fn is_missing(&self) -> bool {
        *self == Kind::Missing
    }
}

/// What a write did. ⛔ `Unchanged` is a distinct outcome and never folded into
/// `Wrote`: a fixup that reports a change it did not make is the `sandlock`
/// failure [`TODO/cli.md`](../../../TODO/cli.md) T-0804 names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    Created,
    Replaced,
    Unchanged,
}

/// What podvm found where it wanted a directory.
///
/// ⛔ **Three answers and the third is the one a `Kind::Dir` test lost.**
/// Measured on 2026-09-09: `/etc/ssl/certs` is a SYMLINK on openSUSE, to a real
/// directory inside the rootfs that podvm must write into, and it is a symlink
/// pointing OUT of the rootfs in door D of
/// `experiments/85-completion-symlink-escape.sh`, which podvm must not follow
/// and must not stay quiet about. A test on the last component alone answers
/// "not a directory" to both.
pub enum Reach {
    /// podvm can reach a directory there, staying inside the rootfs.
    Yes,
    /// There is nothing at this path at all. ⚠ Quiet on purpose: inventing
    /// `/etc/pki/...` in an image with no such tree is podvm guessing.
    Absent,
    /// Something is there and podvm could not reach a directory through it.
    Unreachable(String),
}

impl Root {
    /// Which of the three [`Reach`] answers describes `rel`.
    pub fn reach(&self, dir: &str) -> Result<Reach> {
        let kind = self.kind(dir)?;
        if kind.is_missing() {
            return Ok(Reach::Absent);
        }
        match self.dir_following(dir)? {
            Some(true) => Ok(Reach::Yes),
            Some(false) => Ok(Reach::Unreachable(match kind {
                Kind::Symlink(t) => format!(
                    "{dir} is a symlink to {t}, which does not resolve to a directory \
                     inside this rootfs. refused: podvm will not write through it, and a \
                     payload reading this path gets nothing"
                ),
                other => format!(
                    "{dir} is {other:?} and not a directory podvm can reach, so the \
                     bundle a payload would read here is not there"
                ),
            })),
            // ⚠ The third state: no `openat2`, so podvm could not ask. Reported as
            // unreachable with that as the reason rather than assumed either way.
            None => Ok(Reach::Unreachable(format!(
                "this kernel has no openat2(2), so podvm cannot prove that writing \
                 into {dir} stays inside the rootfs"
            ))),
        }
    }
}

/// A descriptor on the rootfs, plus the two operations the completion layer
/// needs.
pub struct Root {
    dir: Dir,
    path: String,
}

impl Root {
    pub fn open(rootfs: &str) -> Result<Root> {
        let dir = Dir::open(rootfs)
            .map_err(|e| Error::Complete(format!("cannot open the rootfs {rootfs}: {e}")))?;
        Ok(Root {
            dir,
            path: rootfs.to_string(),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Split a rootfs-relative path into components, refusing anything that is
    /// not one. ⚠ Lexical, and it is only half the check: the other half is the
    /// kernel's, in [`safety::open_parent`].
    fn parts(rel: &str) -> Result<Vec<&str>> {
        safety::components(rel).map_err(|r: Refusal| Error::Complete(r.why()))
    }

    /// Open the parent of `rel`, creating directories where asked.
    ///
    /// ⛔ **Three outcomes, not two.** `Ok(Ok(..))` reached it; `Ok(Err(why))`
    /// did not, and `why` says which of the two reasons it was; `Err` is a
    /// lexical refusal of the path itself, which is a defect in podvm rather
    /// than a property of the image.
    ///
    /// ⚠ An ancestor that does not exist is NOT an error. `etc/apt/apt.conf.d`
    /// is absent in every rootfs that is not Debian-family, and a `kind()` on a
    /// file inside it must answer "missing" rather than fail the whole
    /// completion run. An ancestor that is a symlink OUT of the rootfs is the
    /// other reason, and the two are told apart because only one of them is a
    /// refusal the caller must report.
    fn parent(
        &self,
        rel: &str,
        make_dirs: bool,
    ) -> Result<std::result::Result<(Dir, String), String>> {
        let parts = Self::parts(rel)?;
        match safety::open_parent_with(&self.dir, &parts, make_dirs, Resolve::InRoot) {
            Ok(Ok(x)) => Ok(Ok(x)),
            // A refusal: a component resolved outside the rootfs, or the kernel
            // declined the resolution.
            Ok(Err(r)) => Ok(Err(r.why())),
            Err(e) => Ok(Err(format!("{e}"))),
        }
    }

    /// What is at `rel`, with the last component not followed.
    pub fn kind(&self, rel: &str) -> Result<Kind> {
        let Ok((parent, name)) = self.parent(rel, false)? else {
            return Ok(Kind::Missing);
        };
        let Some(c) = CBuf::new(&name) else {
            return Ok(Kind::Missing);
        };
        let st = match sys::fstatat(parent.fd(), &c, sys::AT_SYMLINK_NOFOLLOW) {
            Ok(st) => st,
            Err(_) => return Ok(Kind::Missing),
        };
        let fmt = st.st_mode & sys::S_IFMT;
        Ok(match fmt {
            0o100000 => Kind::Regular {
                len: st.st_size as u64,
                mode: st.st_mode & 0o7777,
            },
            0o040000 => Kind::Dir,
            0o020000 => Kind::CharDev {
                // ⚠ The kernel's wide encoding, which is what `st_rdev`
                // carries and what `makedev` builds.
                major: ((st.st_rdev >> 8) & 0xfff) | ((st.st_rdev >> 32) & !0xfffu64),
                minor: (st.st_rdev & 0xff) | ((st.st_rdev >> 12) & !0xffu64),
            },
            0o120000 => {
                let t = sys::readlinkat(parent.fd(), &c)
                    .map(|b| String::from_utf8_lossy(&b).to_string())
                    .unwrap_or_default();
                Kind::Symlink(t)
            }
            other => Kind::Other(other),
        })
    }

    /// Read a regular file at `rel`, never following the last component.
    ///
    /// ⛔ Bounded. `std::fs::read` on a character device reads to an EOF that
    /// never comes: `/dev/urandom` allocated 13 GB here before the OOM killer
    /// took it ([`TODO/supervise.md`](../../../TODO/supervise.md) T-0602). The
    /// completion layer reads configuration files, and a configuration file
    /// that is 8 MiB is not one podvm is going to edit correctly anyway.
    pub fn read(&self, rel: &str) -> Result<Option<Vec<u8>>> {
        const CEILING: usize = 8 * 1024 * 1024;
        let Ok((parent, name)) = self.parent(rel, false)? else {
            return Ok(None);
        };
        let Some(c) = CBuf::new(&name) else {
            return Ok(None);
        };
        let fd = match sys::openat(
            parent.fd(),
            &c,
            sys::O_RDONLY | sys::O_NOFOLLOW | sys::O_CLOEXEC,
            0,
        ) {
            Ok(fd) => fd,
            Err(_) => return Ok(None),
        };
        let mut out = Vec::new();
        let mut buf = [0u8; 65536];
        loop {
            match sys::read(fd, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    out.extend_from_slice(&buf[..n as usize]);
                    if out.len() > CEILING {
                        let _ = sys::close(fd);
                        return Err(Error::Complete(format!(
                            "{rel} is larger than {CEILING} bytes; podvm will not \
                             read a configuration file that size"
                        )));
                    }
                }
                Err(e) if e == sys::EINTR => continue,
                Err(e) => {
                    let _ = sys::close(fd);
                    return Err(Error::Complete(format!("cannot read {rel}: {}", e.name())));
                }
            }
        }
        let _ = sys::close(fd);
        Ok(Some(out))
    }

    /// Read the same way, as text, dropping a file that is not UTF-8.
    ///
    /// ⚠ `None` for both "absent" and "not text" is deliberate here: every
    /// caller of this treats a configuration file it cannot parse exactly like
    /// one that is not there, and the two would otherwise be one `match` arm
    /// written twice.
    pub fn read_text(&self, rel: &str) -> Result<Option<String>> {
        Ok(self
            .read(rel)?
            .and_then(|b| String::from_utf8(b).ok().map(|s| s.to_string())))
    }

    /// Write `bytes` to `rel`, replacing whatever is there.
    ///
    /// ⛔ **Unlink first, then create with `O_EXCL | O_NOFOLLOW`.** A truncate
    /// of an existing name follows a symlink exactly as an ordinary open does,
    /// so "the file is already there" is the case that escapes.
    pub fn write(&self, rel: &str, bytes: &[u8], mode: u32) -> Result<Wrote> {
        let existing = self.kind(rel)?;
        // ⛔ The MODE is part of "unchanged". T-0404 widens the owner write bit
        // of an `/etc/passwd` whose content it does not touch, and a comparison
        // on the bytes alone reported that as a no-op and left the file
        // read-only, which is the failure the fixup exists for.
        if let Kind::Regular { mode: have, .. } = existing
            && self.read(rel)?.as_deref() == Some(bytes)
        {
            if have == mode {
                return Ok(Wrote::Unchanged);
            }
            let Ok((parent, name)) = self.parent(rel, false)? else {
                return Ok(Wrote::Unchanged);
            };
            let c = CBuf::new(&name).ok_or_else(|| {
                Error::Complete(format!("{rel} contains a NUL and cannot be written"))
            })?;
            sys::fchmodat(parent.fd(), &c, mode as u64, 0).map_err(|e| {
                Error::Complete(format!("cannot set the mode of {rel}: {}", e.name()))
            })?;
            return Ok(Wrote::Replaced);
        }
        let (parent, name) = match self.parent(rel, true)? {
            Ok(x) => x,
            Err(why) => {
                return Err(Error::Complete(format!(
                    "{rel}: podvm will not write through this path: {why}"
                )))
            }
        };
        let c = CBuf::new(&name).ok_or_else(|| {
            Error::Complete(format!("{rel} contains a NUL and cannot be written"))
        })?;
        // ⛔ Unconditional, and it covers the symlink, the dangling symlink and
        // the ordinary file in one call. ENOENT here is the ordinary case.
        match sys::unlinkat(parent.fd(), &c, 0) {
            Ok(_) => {}
            Err(e) if e == sys::ENOENT => {}
            Err(e) if e.0 == 21 => {
                // EISDIR: a directory where the completion layer wants a file.
                return Err(Error::Complete(format!(
                    "{rel} is a directory in this image; podvm will not replace a \
                     directory with a file"
                )));
            }
            Err(e) => {
                return Err(Error::Complete(format!(
                    "cannot remove the existing {rel}: {}",
                    e.name()
                )))
            }
        }
        let fd = sys::openat(
            parent.fd(),
            &c,
            sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL | sys::O_NOFOLLOW | sys::O_CLOEXEC,
            mode as u64,
        )
        .map_err(|e| Error::Complete(format!("cannot create {rel}: {}", e.name())))?;
        let mut off = 0;
        while off < bytes.len() {
            match sys::write(fd, &bytes[off..]) {
                Ok(0) => break,
                Ok(n) => off += n as usize,
                Err(e) if e == sys::EINTR => continue,
                Err(e) => {
                    let _ = sys::close(fd);
                    return Err(Error::Complete(format!("cannot write {rel}: {}", e.name())));
                }
            }
        }
        // ⚠ The mode is set explicitly rather than left to the open: umask
        // subtracts from the mode an open asks for, and T-0404 needs the
        // payload's own `useradd` to be able to write `/etc/passwd` back.
        let _ = sys::fchmod(fd, mode as u64);
        let _ = sys::close(fd);
        Ok(match existing {
            Kind::Missing => Wrote::Created,
            _ => Wrote::Replaced,
        })
    }
}

mod follow;

#[cfg(test)]
mod tests;
