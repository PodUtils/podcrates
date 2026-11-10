//! `TODO/extract.md` T-0304 and T-0305: an entry lands inside the destination
//! or it does not land at all.
//!
//! ⭐ **Two halves, and the corpus carries one of them.**
//! `references/qaidvoid__onelf/tree/crates/onelf-format/src/manifest.rs:19-48`
//! is `symlink_target_within_root`, a purely lexical check whose own comment
//! says why it is lexical: "so it cannot be defeated by filesystem races".
//! Beside it,
//! `references/qaidvoid__onelf/tree/crates/onelf-format/src/manifest.rs:10-12`
//! rejects `""`, `.`, `..`, `/` and NUL as a path component.
//!
//! ⛔ **That is only half of what M2 acceptance 3 needs, and the missing half
//! is the attack.** onelf validates a symlink entry when the symlink is
//! created. It does not validate a later regular entry whose path *traverses*
//! that symlink. The crafted layer writes `evil -> /etc` as its first entry and
//! `evil/passwd` as its second: both are lexically inside the destination, and
//! the second lands outside it.
//!
//! So this module does both:
//!
//! 1. every entry's path is checked **lexically**, component by component,
//!    which is what catches `..` and an absolute path with no syscall at all;
//! 2. every entry is then opened **against the accumulated tree** with
//!    `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`, which is what catches
//!    the traversal, and the kernel enforces it rather than this code.
//!
//! ⚠ `openat2(2)` is Linux 5.6. Where it answers `ENOSYS` the same walk is done
//! by hand with `O_NOFOLLOW` on every component while holding a directory
//! descriptor, which is equivalent and slower. ⛔ The fallback is entered only
//! on `ENOSYS` or `E2BIG`, never on a refusal: a kernel that says "no" is
//! answering, and retrying an answer with a weaker mechanism is how a safety
//! check becomes a formality.

use podvm_probe::sys::{self, CBuf, Errno};

use crate::error::{Error, Result};

/// Why a path was refused. Each names the entry and, at the call site, the
/// layer digest, because "refused an entry" without either is a message nobody
/// can act on.
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

/// The lexical half, and it is the same rule for an entry path and for a
/// rebased symlink target.
///
/// ⛔ Returns the components rather than a boolean, so the caller cannot use
/// the original string after the check and reintroduce what was rejected.
/// Is this member name the archive's own root rather than something in it?
///
/// ⭐ `./`, `.` and `/` are the destination. GNU tar writes `./` as the first
/// member of an archive created from `.`, and Debian's base images carry it.
/// ⛔ It is NOT the same as an empty name: `""` names nothing and stays a
/// refusal, because a member podvm cannot place is a member it may not
/// silently drop.
pub fn is_archive_root(path: &str) -> bool {
    let t = path.trim_end_matches('/');
    t == "." || t.is_empty() && path.starts_with('/')
}

pub fn components(path: &str) -> std::result::Result<Vec<&str>, Refusal> {
    if path.contains('\0') {
        return Err(Refusal::NulByte);
    }
    if path.starts_with('/') {
        return Err(Refusal::Absolute);
    }
    let mut out = Vec::new();
    for c in path.split('/') {
        // ⚠ A tar path routinely carries a trailing slash on a directory and
        // `./` on a prefixed archive. Neither is a defect and neither is a
        // component: they are dropped, not refused. `..` is refused.
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

/// ⭐ T-0305: an absolute symlink target is **rootfs-relative**, not a refusal.
///
/// A distro rootfs is full of legitimate absolute symlinks, and the refusal
/// that must not be copied is
/// `references/qaidvoid__onelf/tree/crates/onelf-format/src/manifest.rs:27-29`,
/// which returns false for any absolute target. Rejecting them would refuse
/// almost every real image: `experiments/results/whiteout-contract.txt` check C
/// reads `var/cache/xbps -> /var/cache/xbps` out of a real layer.
///
/// The target is interpreted relative to the rootfs, which is what it will mean
/// once the payload is inside the chroot, and the result is then containment
/// checked like anything else. ⛔ Rebase, never dereference: dereferencing at
/// extraction time bakes the build host's tree into the image.
///
/// `link_dir` is the directory the link itself sits in, as components, because
/// a relative target is resolved from there and an absolute one is not.
pub fn rebase_symlink_target(
    link_dir: &[&str],
    target: &str,
) -> std::result::Result<Vec<String>, Refusal> {
    if target.contains('\0') {
        return Err(Refusal::NulByte);
    }
    if target.is_empty() {
        return Err(Refusal::Empty);
    }
    let mut stack: Vec<String> = if target.starts_with('/') {
        // Absolute: rootfs-relative, so the walk starts at the rootfs root.
        Vec::new()
    } else {
        link_dir.iter().map(|s| (*s).to_string()).collect()
    };
    for c in target.split('/') {
        if c.is_empty() || c == "." {
            continue;
        }
        if c == ".." {
            // ⛔ Popping past the root is the escape. `/..` is `/` on a real
            // filesystem, but a target that needs that many `..` to be legal is
            // one whose meaning depends on where the rootfs is mounted, and
            // that is not a link this extractor can honour.
            if stack.pop().is_none() {
                return Err(Refusal::Escapes(format!(
                    "the target {target:?} climbs above the rootfs root"
                )));
            }
            continue;
        }
        stack.push(c.to_string());
    }
    Ok(stack)
}

/// A directory descriptor, closed on drop.
///
/// ⛔ Extraction holds one of these on the destination for the whole run. Every
/// write is relative to it, so no path is ever re-resolved from `/` through
/// components a previous entry may have replaced in the meantime.
pub struct Dir(i64);

impl Dir {
    pub fn open(path: &str) -> Result<Dir> {
        let c = cbuf(path)?;
        let fd = sys::open(&c, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC, 0)
            .map_err(|e| Error::Extract(format!("cannot open {path}: {}", e.name())))?;
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
/// ⛔ **This enum exists because the fallback is otherwise dead code on every
/// machine that can run the tests.** Measured on 2026-09-08: this host is
/// kernel 6.18.44 and `openat2(2)` is present, so an `Auto` run never enters
/// the `O_NOFOLLOW` walk, and the walk is what protects every kernel before
/// 5.6. A safety mechanism nobody has seen work is not a safety mechanism,
/// which is `podvm-plant`'s whole argument applied to this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolve {
    /// `openat2` where the kernel has it, the walk otherwise. Production, for
    /// EXTRACTION.
    Auto,
    /// ⛔ Force the `O_NOFOLLOW` walk. Used by the tests to drive the pre-5.6
    /// path on a kernel that does not need it.
    Walk,
    /// ⭐ **Follow a symlink, confined to the root**, which is what a FINISHED
    /// rootfs needs and what an extraction must never do.
    ///
    /// ⛔ The two are different questions and conflating them broke both ways.
    /// During extraction a symlink another entry in the same layer created is
    /// the attack ([`crate`] rule 4), so [`Resolve::Auto`] refuses every one of
    /// them. Afterwards, the rootfs is full of legitimate internal links  --
    /// `/etc/ssl/certs -> /var/lib/ca-certificates/pem` on openSUSE, `/lib ->
    /// usr/lib` on void -- and a writer that refuses those cannot reach the file
    /// the payload will actually read.
    ///
    /// ⚠ Measured on 2026-09-09 by `experiments/240-distro-sweep.sh`: with
    /// `Auto`, `podvm-complete`'s CA-bundle fixup reported
    /// `etc/ssl/certs/ca-certificates.crt: ... leaves the destination` on
    /// openSUSE, and its libc probe read `unknown` on void because `/lib` is a
    /// link. Both are rootfs-internal links and neither leaves anything.
    ///
    /// ⛔ It applies to the DIRECTORY components only. The final component is
    /// still opened `O_NOFOLLOW` by the caller and replaced by
    /// unlink-then-create, because following THAT is
    /// [`TODO/complete.md`](../../../TODO/complete.md) T-0405's escape.
    InRoot,
}

impl Resolve {
    fn use_openat2(self) -> bool {
        self != Resolve::Walk && have_openat2()
    }

    /// The `openat2` resolve flags this mode asks the kernel for.
    fn flags(self) -> u64 {
        match self {
            // ⚠ `RESOLVE_IN_ROOT` treats the starting descriptor as `/`: an
            // absolute symlink target is rebased onto it and `..` at the top
            // stays at the top, so nothing resolves outside the rootfs and the
            // kernel enforces it rather than this code.
            Resolve::InRoot => sys::RESOLVE_IN_ROOT,
            _ => sys::RESOLVE_BENEATH | sys::RESOLVE_NO_SYMLINKS,
        }
    }
}

/// Does this kernel have `openat2(2)`?
///
/// ⭐ Measured once, by calling it, and never inferred from a release string. A
/// kernel version is a claim; the syscall's own answer is the fact. The result
/// is cached because the answer cannot change while this process runs.
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
            // ⛔ Only these two mean "this kernel cannot answer". Anything else
            // is an answer about `.`, and the syscall exists.
            Err(e) if e == sys::ENOSYS || e.0 == 7 => false,
            Err(_) => true,
        }
    })
}

fn cbuf(s: &str) -> Result<CBuf> {
    CBuf::new(s)
        .ok_or_else(|| Error::Extract(format!("{s:?} contains a NUL and cannot reach the kernel")))
}

/// Open the **parent** of `parts` beneath `root`, creating directories as
/// needed, and return the descriptor plus the final component.
///
/// ⛔ This is the function T-0304 turns on. Every component is resolved with
/// symlinks refused, so `evil/passwd` after `evil -> /etc` cannot land: the
/// resolution of `evil` fails rather than succeeding somewhere else.
pub fn open_parent(
    root: &Dir,
    parts: &[&str],
    make_dirs: bool,
) -> Result<std::result::Result<(Dir, String), Refusal>> {
    open_parent_with(root, parts, make_dirs, Resolve::Auto)
}

/// As [`open_parent`], with the resolution mechanism named. ⛔ Production calls
/// [`open_parent`]; this exists so the tests can drive BOTH mechanisms on one
/// kernel.
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
