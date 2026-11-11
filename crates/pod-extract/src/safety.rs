//! An entry lands in the destination, or it does not land.
//!
//! **Two halves.** onelf has a lexical check of a symlink target ("so it
//! cannot be defeated by filesystem races") and refuses `""`, `.`, `..`, `/`
//! and NUL as a path component. That is only half: onelf checks a symlink
//! entry when it makes the symlink, and does not check a later entry whose
//! path goes *through* that symlink. A crafted layer writes `evil -> /etc`
//! first and `evil/passwd` second: both are lexically in the destination, and
//! the second lands outside it.
//!
//! This module does both:
//!
//! 1. the path of each entry gets a **lexical** check, component by
//!    component: it finds `..` and an absolute path with no system call;
//! 2. each entry is then opened **against the tree that the layers wrote**
//!    with `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`: it finds the
//!    path through a symlink, and the kernel enforces it.
//!
//! `openat2(2)` is Linux 5.6. Where it gives `ENOSYS`, the same walk is done
//! with `O_NOFOLLOW` on each component from a held directory descriptor: the
//! same result, slower. The walk is used only on `ENOSYS` or `E2BIG`, never
//! on a refusal: a kernel that says "no" gave an answer, and a try of the
//! same question with a weaker mechanism makes the check of no use.

use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::error::{self, Result, cbuf};

/// Why a path was refused. At the call site the refusal also names the entry
/// and the digest of the layer: "refused an entry" with neither tells the
/// reader nothing to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Absolute,
    Empty,
    Component(String),
    NulByte,
    /// The kernel refused the resolution. This is the traversal case.
    Escapes(String),
}

impl Refusal {
    pub fn why(&self) -> String {
        match self {
            Refusal::Absolute => "it is absolute, so it names a path outside the \
                                  destination rather than inside it"
                .into(),
            Refusal::Empty => "it is empty".into(),
            Refusal::Component(c) => format!(
                "the component {c:?} is not a name: `.` and `..` move within the \
                 tree rather than naming a place in it"
            ),
            Refusal::NulByte => "it contains a NUL, which the kernel would \
                                 silently truncate, so the path written would \
                                 not be the path the layer named"
                .into(),
            Refusal::Escapes(e) => format!(
                "resolving it against what the layers have written so far leaves \
                 the destination ({e}). A symlink an earlier entry created is \
                 the usual cause, and it is exactly the case a lexical check \
                 cannot see"
            ),
        }
    }
}

/// Is this member name the root of the archive, not an entry in it?
///
/// `./`, `.` and `/` are the destination. GNU tar writes `./` as the first
/// member of an archive made from `.`, and the Debian base images have it.
/// It is NOT an empty name: `""` names nothing and stays a refusal, as the
/// crate cannot put it anywhere and must not drop it with no word.
pub fn is_archive_root(path: &str) -> bool {
    let t = path.trim_end_matches('/');
    t == "." || t.is_empty() && path.starts_with('/')
}

/// Is this member the root of the archive, with its type? A tar writer can
/// name the root of a layer `""` as a directory, as others write `./`
/// (the layers of `public.ecr.aws/lambda/provided:al2023`, PodUtils/podvm#261).
/// Of an empty name, only a directory is the root: each other type of
/// entry with an empty name stays refused.
pub fn is_root_entry(path: &str, is_dir: bool) -> bool {
    is_archive_root(path) || path.is_empty() && is_dir
}

/// The lexical half: the same rule for the path of an entry and for a
/// rebased symlink target.
///
/// It returns the components, not a boolean, so that the caller cannot use
/// the first string after the check and bring back what it refused.
pub fn components(path: &str) -> std::result::Result<Vec<&str>, Refusal> {
    if path.contains('\0') {
        return Err(Refusal::NulByte);
    }
    if path.starts_with('/') {
        return Err(Refusal::Absolute);
    }
    let mut out = Vec::new();
    for c in path.split('/') {
        // A tar path often has a trailing slash on a directory and `./` in a
        // prefixed archive. Neither is a defect or a component: they are
        // dropped, not refused. `..` is refused.
        if c.is_empty() || c == "." {
            continue;
        }
        if c == ".." {
            return Err(Refusal::Component(c.into()));
        }
        out.push(c);
    }
    if out.is_empty() {
        return Err(Refusal::Empty);
    }
    Ok(out)
}

/// An absolute symlink target is **relative to the root file system**, not a
/// refusal.
///
/// The root file system of a distribution has many legal absolute symlinks
/// (void: `var/cache/xbps -> /var/cache/xbps`). onelf refuses each absolute
/// target, and that refuses almost each real image.
///
/// The target is read relative to the root file system, which is what it
/// means for the payload in the chroot, and the result then gets the
/// containment check. Rebase, never dereference: a dereference at extraction
/// puts the tree of the build host into the image.
///
/// `link_dir` is the directory of the link, as components: a relative target
/// resolves from there, and an absolute target does not.
pub fn rebase_symlink_target(link_dir: &[&str], target: &str) -> std::result::Result<Vec<String>, Refusal> {
    if target.contains('\0') {
        return Err(Refusal::NulByte);
    }
    if target.is_empty() {
        return Err(Refusal::Empty);
    }
    let mut stack: Vec<String> = if target.starts_with('/') {
        // Absolute: relative to the root file system, so the walk starts at
        // its root.
        Vec::new()
    } else {
        link_dir.iter().map(|s| (*s).to_string()).collect()
    };
    for c in target.split('/') {
        if c.is_empty() || c == "." {
            continue;
        }
        if c == ".." {
            // A pop above the root is the escape. `/..` is `/` on a real file
            // system, but the meaning of a target that needs that many `..`
            // depends on where the root file system is, and the crate cannot
            // keep that link.
            if stack.pop().is_none() {
                return Err(Refusal::Escapes(format!("the target {target:?} climbs above the rootfs root")));
            }
            continue;
        }
        stack.push(c.to_string());
    }
    Ok(stack)
}

/// A directory descriptor, closed on drop.
///
/// An extraction holds one on the destination for the whole run. Each write is
/// relative to it, so no path resolves again from `/` through components that
/// an earlier entry can have replaced.
pub struct Dir(i64);

impl Dir {
    pub fn open(path: &str) -> Result<Dir> {
        let c = cbuf(path)?;
        let fd = sys::open(&c, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC, 0)
            .map_err(|e| error::sys(format!("cannot open {path}"), e))?;
        Ok(Dir(fd))
    }

    pub fn fd(&self) -> i64 {
        self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = sys::close(self.0);
    }
}

/// Which mechanism resolves a component.
///
/// **Without this enum the walk is dead code on each machine that runs the
/// tests**: a current kernel has `openat2(2)`, so an `Auto` run never goes
/// into the `O_NOFOLLOW` walk, and the walk protects each kernel before 5.6.
/// A safety mechanism that nobody saw work is not a safety mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolve {
    /// `openat2` where the kernel has it, the walk on the others. For an
    /// EXTRACTION.
    Auto,
    /// The `O_NOFOLLOW` walk always. The tests use it to run the path of the
    /// kernels before 5.6 on a kernel that does not need it.
    Walk,
    /// **Follow a symlink, held in the root**: what a FINISHED root file
    /// system needs, and what an extraction must never do.
    ///
    /// The two are different questions. During an extraction, a symlink that
    /// another entry of the same layer made is the attack ([`crate`] rule 4),
    /// so [`Resolve::Auto`] refuses each one. After it, the root file system
    /// has many legal internal links (`/etc/ssl/certs ->
    /// /var/lib/ca-certificates/pem` on openSUSE, `/lib -> usr/lib` on void),
    /// and a writer that refuses them cannot reach the file that the payload
    /// reads. With `Auto`, the CA bundle of openSUSE "leaves the destination"
    /// and the C library of void is not found; both are internal links.
    ///
    /// It applies to the DIRECTORY components only. The caller still opens
    /// the last component with `O_NOFOLLOW` and replaces it with an unlink and
    /// a create: to follow THAT component is an escape.
    InRoot,
}

impl Resolve {
    fn use_openat2(self) -> bool {
        self != Resolve::Walk && have_openat2()
    }

    /// The `openat2` resolve flags this mode asks the kernel for.
    fn flags(self) -> u64 {
        match self {
            // `RESOLVE_IN_ROOT` takes the start descriptor as `/`: an absolute
            // symlink target is rebased onto it and `..` at the top stays at
            // the top, so nothing resolves outside the root file system, and
            // the kernel enforces it.
            Resolve::InRoot => sys::RESOLVE_IN_ROOT,
            _ => sys::RESOLVE_BENEATH | sys::RESOLVE_NO_SYMLINKS,
        }
    }
}

/// Does this kernel have `openat2(2)`?
///
/// Measured by a call, not read from a release string: a kernel version is a
/// claim, and the answer of the call is the fact. The answer is kept for the
/// process: the kernel does not change while the process runs.
pub fn have_openat2() -> bool {
    use std::sync::OnceLock;
    static HAVE: OnceLock<bool> = OnceLock::new();
    *HAVE.get_or_init(|| {
        let Some(c) = CBuf::new(".") else {
            return false;
        };
        let how = sys::OpenHow {
            flags: sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC,
            mode: 0,
            resolve: sys::RESOLVE_BENEATH | sys::RESOLVE_NO_SYMLINKS,
        };
        match sys::openat2(sys::AT_FDCWD as i64, &c, &how) {
            Ok(fd) => {
                let _ = sys::close(fd);
                true
            }
            // Only these two mean "this kernel cannot answer". Each other errno
            // is an answer about `.`, and the call exists.
            Err(e) if e == sys::ENOSYS || e == sys::E2BIG => false,
            Err(_) => true,
        }
    })
}

/// Open the **parent** of `parts` under `root`, make the directories that are
/// missing when `make_dirs`, and return the descriptor and the last component.
///
/// The containment turns on this function. Each component resolves with
/// symlinks refused, so `evil/passwd` after `evil -> /etc` cannot land: the
/// resolution of `evil` fails, it does not go elsewhere.
pub fn open_parent(root: &Dir, parts: &[&str], make_dirs: bool) -> Result<std::result::Result<(Dir, String), Refusal>> {
    open_parent_with(root, parts, make_dirs, Resolve::Auto)
}

/// As [`open_parent`], with the mechanism of the resolution named. An
/// extraction calls [`open_parent`]; the tests run BOTH mechanisms on one
/// kernel with this function, and a writer of a finished root file system
/// gives [`Resolve::InRoot`].
pub fn open_parent_with(
    root: &Dir,
    parts: &[&str],
    make_dirs: bool,
    how: Resolve,
) -> Result<std::result::Result<(Dir, String), Refusal>> {
    let (last, dirs) = match parts.split_last() {
        Some(x) => x,
        None => return Ok(Err(Refusal::Empty)),
    };
    if how == Resolve::InRoot {
        return in_root_parent(root, dirs, last, make_dirs);
    }
    let mut cur = dup_dir(root, how)?;
    for d in dirs {
        match step(&cur, d, make_dirs, how)? {
            Ok(next) => cur = next,
            Err(r) => return Ok(Err(r)),
        }
    }
    Ok(Ok((cur, (*last).to_string())))
}

mod walk;
use walk::*;

#[cfg(test)]
mod tests;
