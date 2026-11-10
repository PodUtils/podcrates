//! The extraction itself: `TODO/extract.md` T-0301, T-0306 and T-0307.
//!
//! ⛔ **Entry level, never `unpack()`.** T-0301: shelling out to `tar` inherits
//! its ownership semantics, its exit codes and its path behaviour, and that is
//! how the ownership wall reaches five separate tools rather than one.
//! `references/RuriOSS__rurima/tree/src/archive.c:95` builds a `tar` argument
//! vector containing `-xpf`, which preserves permissions and ownership, and
//! that is where its `docker pull` dies.
//! `references/indigo-dc__udocker/tree/udocker/container/structure.py:285-287`
//! also shells out and survives only because it passes `--no-same-owner` and
//! `--no-same-permissions`. **The flags are the mechanism; the shell-out is
//! not.** A library's `unpack()` convenience is the same mistake in-process: it
//! makes the ownership, permission and path decisions this crate exists to
//! make.
//!
//! ⭐ **The layer order is more precise than "whiteouts after each layer".**
//! `references/indigo-dc__udocker/tree/udocker/container/structure.py:279`
//! applies layer N's whiteouts to the accumulated tree **before** extracting N,
//! which is the order that lets a layer both delete a path and recreate it. The
//! opposite order deletes what the same layer just wrote.

use std::io::Read;

use podvm_probe::sys;

use crate::error::{Error, Result};
use crate::safety::{self, Dir, Refusal};
use crate::sidecar::{Meta, Sidecar};
use crate::whiteout::{self, Kind};

/// What one layer did, for the transcript and for the record.
#[derive(Debug, Default, Clone)]
pub struct LayerStats {
    pub entries: u64,
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub hardlinks: u64,
    pub whiteouts: u64,
    pub opaques: u64,
    /// Entries whose type podvm does not materialise. ⚠ Counted and named
    /// rather than silently dropped: a device node in a layer is a thing the
    /// runtime could not create, and a caller that sees `0` when one was
    /// skipped has been told the wrong thing.
    pub skipped: u64,
    pub skipped_kinds: Vec<String>,
    /// ⭐ T-0306: how many entries this layer's re-permission pass widened.
    pub widened: u64,
}

/// The ids this process can actually apply, read once.
///
/// ⛔ Read from the kernel, never assumed to be 0. podvm runs as uid 0 on the
/// target, and as whoever invoked it everywhere else; an extractor that
/// hard-coded 0 would write a sidecar claiming it applied an id it did not.
#[derive(Debug, Clone, Copy)]
pub struct Ids {
    pub uid: u64,
    pub gid: u64,
}

impl Ids {
    pub fn current() -> Ids {
        Ids {
            uid: unsafe { sys::syscall6(sys::SYS_GETUID, 0, 0, 0, 0, 0, 0) }.max(0) as u64,
            gid: unsafe { sys::syscall6(sys::SYS_GETGID, 0, 0, 0, 0, 0, 0) }.max(0) as u64,
        }
    }
}

/// Apply one layer to the accumulated tree.
///
/// `whiteouts` is the layer's own whiteout set, already collected by
/// [`collect_whiteouts`] and already applied by [`apply_whiteouts`]. Passing it
/// in rather than recomputing it is what keeps the two passes over one stream
/// honest about being two passes.
pub fn apply_layer(
    root: &Dir,
    digest: &str,
    stream: &mut dyn Read,
    ids: Ids,
    sidecar: &mut Sidecar,
) -> Result<LayerStats> {
    let mut st = LayerStats::default();
    // ⭐ T-0306's subject: only what THIS layer wrote is re-permissioned, so a
    // mode the image set on an untouched directory is left alone.
    let mut wrote_dirs: Vec<(Vec<String>, u32)> = Vec::new();

    let mut ar = tar::Archive::new(stream);
    // ⛔ `entries()`, and every decision below is made here rather than by the
    // library. This is the line T-0301 is about.
    for entry in ar.entries()? {
        let mut entry = entry?;
        let header = entry.header().clone();
        let raw = entry.path_bytes().to_vec();
        let path = String::from_utf8_lossy(&raw).into_owned();
        st.entries += 1;

        // A whiteout was handled before this layer's entries were extracted, so
        // the marker itself is never materialised.
        match whiteout::classify(&path) {
            Kind::Remove(_) => {
                st.whiteouts += 1;
                continue;
            }
            Kind::Opaque => {
                st.opaques += 1;
                continue;
            }
            Kind::Normal => {}
        }

        // ⭐ **THE ARCHIVE ROOT IS AN ENTRY, AND IT IS NOT A REFUSAL.** A tar
        // built with a `./` prefix carries `./` itself as its first member, and
        // it names the destination rather than something inside it: there is
        // nothing to create and nothing to refuse.
        //
        // ⛔ Measured, not anticipated: `experiments/results/whiteout-contract.txt`
        // read "no `./` on any layer" out of the two images pinned at M2, and
        // that reading is about those two images. `public.ecr.aws/debian/debian`
        // carries one, and on 2026-09-09 podvm refused the whole layer of every
        // Debian image with "the entry \"./\" is not extractable because it is
        // empty". ⚠ Only the root: an entry with a genuinely empty name is still
        // refused, because a member with no name is a member podvm cannot place.
        if safety::is_archive_root(&path) {
            st.skipped += 1;
            let k = "ArchiveRoot".to_string();
            if !st.skipped_kinds.contains(&k) {
                st.skipped_kinds.push(k);
            }
            continue;
        }
        let parts = match safety::components(&path) {
            Ok(p) => p,
            Err(r) => return Err(refuse(digest, &path, &r)),
        };

        let kind = header.entry_type();
        let mode = header.mode().unwrap_or(0o644);
        let uid = header.uid().unwrap_or(0);
        let gid = header.gid().unwrap_or(0);

        // ⛔ The parent is opened by descent with symlinks refused. This is
        // T-0304, and it is what stops `evil/passwd` after `evil -> /etc`.
        let (parent, name) = match safety::open_parent(root, &parts, true)? {
            Ok(x) => x,
            Err(r) => return Err(refuse(digest, &path, &r)),
        };

        let mut applied_mode = None;
        if kind.is_dir() {
            make_dir(&parent, &name, mode)?;
            st.dirs += 1;
            let full: Vec<String> = parts.iter().map(|s| (*s).to_string()).collect();
            wrote_dirs.push((full, mode));
        } else if kind.is_symlink() {
            let target = entry
                .link_name_bytes()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            // ⭐ T-0305: an absolute target is rootfs-relative, not a refusal.
            // The rebased result is containment checked like anything else, and
            // a target that still escapes after rebasing IS refused.
            let dir_parts: Vec<&str> = parts[..parts.len() - 1].to_vec();
            match safety::rebase_symlink_target(&dir_parts, &target) {
                Ok(_) => {}
                Err(r) => return Err(refuse(digest, &path, &r)),
            }
            replace(&parent, &name)?;
            let t = cbuf(&target)?;
            let n = cbuf(&name)?;
            // ⛔ The link is created with the target the image wrote, verbatim.
            // Rebasing decides whether it is ALLOWED, never what is stored:
            // storing the rebased form would bake this extraction's idea of the
            // rootfs into the image, and T-0305 rebases rather than
            // dereferences for exactly that reason.
            sys::symlinkat(&t, parent.fd(), &n).map_err(|e| {
                Error::Extract(format!("cannot create the symlink {path:?}: {}", e.name()))
            })?;
            st.symlinks += 1;
        } else if kind.is_hard_link() {
            let target = entry
                .link_name_bytes()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            // ⛔ T-0307: a hard link's target is resolved inside the
            // destination, and one that resolves outside it is refused. A hard
            // link out of the rootfs is a file the payload can write through.
            let tparts = match safety::components(&target) {
                Ok(p) => p,
                Err(r) => return Err(refuse(digest, &path, &r)),
            };
            let (tparent, tname) = match safety::open_parent(root, &tparts, false)? {
                Ok(x) => x,
                Err(r) => return Err(refuse(digest, &path, &r)),
            };
            replace(&parent, &name)?;
            let tn = cbuf(&tname)?;
            let n = cbuf(&name)?;
            sys::linkat(tparent.fd(), &tn, parent.fd(), &n, 0).map_err(|e| {
                Error::Extract(format!(
                    "cannot link {path:?} to {target:?}, which the layer says is \
                     already extracted: {}",
                    e.name()
                ))
            })?;
            st.hardlinks += 1;
        } else if kind.is_file() {
            let m = write_file(&parent, &name, mode, &mut entry)?;
            applied_mode = m;
            st.files += 1;
        } else {
            // ⚠ A device node, fifo or socket. podvm cannot `mknod` on the
            // target at all (that is the whole premise of this runtime), so
            // these are counted and named rather than attempted and reported as
            // a failure.
            st.skipped += 1;
            let k = format!("{:?}", kind);
            if !st.skipped_kinds.contains(&k) {
                st.skipped_kinds.push(k);
            }
            continue;
        }

        // ⛔ THE SIDECAR, for every entry that landed. T-0302.
        let (applied_uid, applied_gid, reason) = resolve_ids(uid, gid, ids);
        sidecar.push(&Meta {
            path: parts.join("/"),
            uid,
            gid,
            mode,
            applied_uid,
            applied_gid,
            reason,
            applied_mode,
        })?;
    }

    // ⭐ T-0306, after the layer and over what this layer wrote.
    st.widened = repermission(root, &wrote_dirs)?;
    Ok(st)
}

/// ⭐ T-0302's decision, per entry.
///
/// ⛔ Ownership is never restored by default, so `applied` is what the
/// extracting process actually is unless the image's id is one this process can
/// hold. The reason names the gap, because a sidecar row that silently differs
/// from the image is the thing this crate exists not to produce.
fn resolve_ids(uid: u64, gid: u64, ids: Ids) -> (u64, u64, Option<String>) {
    let mut why = Vec::new();
    if uid != ids.uid {
        why.push(format!("uid {uid} unmapped"));
    }
    if gid != ids.gid {
        why.push(format!("gid {gid} unmapped"));
    }
    (
        ids.uid,
        ids.gid,
        if why.is_empty() {
            None
        } else {
            Some(why.join(", "))
        },
    )
}

fn refuse(digest: &str, entry: &str, r: &Refusal) -> Error {
    Error::Refused {
        layer: digest.to_string(),
        entry: entry.to_string(),
        why: r.why(),
    }
}

fn cbuf(s: &str) -> Result<sys::CBuf> {
    sys::CBuf::new(s)
        .ok_or_else(|| Error::Extract(format!("{s:?} contains a NUL and cannot reach the kernel")))
}

fn make_dir(parent: &Dir, name: &str, mode: u32) -> Result<()> {
    let n = cbuf(name)?;
    // ⛔ Created with the owner bits already on. A mode-0555 directory that is
    // created 0555 cannot be written into by the next entry of THIS layer, let
    // alone the next layer, and T-0306's pass runs afterwards.
    match sys::mkdirat(parent.fd(), &n, (mode | 0o700) as u64 & 0o7777) {
        Ok(_) => Ok(()),
        Err(e) if e == sys::EEXIST => Ok(()),
        Err(e) => Err(Error::Extract(format!(
            "cannot create the directory {name:?}: {}",
            e.name()
        ))),
    }
}

/// ⛔ Remove whatever is already at this name before writing.
///
/// A later layer replacing a file with a symlink, or a symlink with a
/// directory, is ordinary OCI. Opening with `O_TRUNC` over an existing SYMLINK
/// would follow it and write through to wherever it points, which is the escape
/// T-0304 refuses by another route.
fn replace(parent: &Dir, name: &str) -> Result<()> {
    let n = cbuf(name)?;
    match sys::unlinkat(parent.fd(), &n, 0) {
        Ok(_) => Ok(()),
        Err(e) if e == sys::ENOENT => Ok(()),
        // A directory being replaced by a file: remove it if it is empty, and
        // otherwise leave it, which is what a union filesystem would do.
        Err(e) if e.0 == 21 => {
            let _ = sys::unlinkat(parent.fd(), &n, sys::AT_REMOVEDIR);
            Ok(())
        }
        Err(e) => Err(Error::Extract(format!(
            "cannot replace {name:?}: {}",
            e.name()
        ))),
    }
}

/// Write one regular file. Returns the widened mode where the image's own mode
/// would not have let this process write the file it is creating.
fn write_file(parent: &Dir, name: &str, mode: u32, r: &mut dyn Read) -> Result<Option<u32>> {
    replace(parent, name)?;
    let n = cbuf(name)?;
    // ⛔ O_EXCL: `replace` removed what was there, so a file at this name now is
    // one another process created between the two calls, and overwriting it
    // would be this extractor losing a race it did not know it was in.
    // ⛔ O_NOFOLLOW: belt and braces against a symlink appearing at the name.
    let want = (mode | 0o600) & 0o7777;
    let fd = sys::openat(
        parent.fd(),
        &n,
        sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL | sys::O_CLOEXEC | sys::O_NOFOLLOW,
        want as u64,
    )
    .map_err(|e| Error::Extract(format!("cannot create {name:?}: {}", e.name())))?;

    let mut buf = vec![0u8; 128 * 1024];
    let mut err = None;
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let mut off = 0;
                while off < n {
                    match sys::write(fd, &buf[off..n]) {
                        Ok(w) if w > 0 => off += w as usize,
                        Ok(_) => {
                            err = Some(Error::Extract(format!(
                                "a write of {name:?} returned 0 with bytes left"
                            )));
                            break;
                        }
                        Err(e) if e == sys::EINTR => continue,
                        Err(e) => {
                            err = Some(Error::Extract(format!(
                                "cannot write {name:?}: {}",
                                e.name()
                            )));
                            break;
                        }
                    }
                }
                if err.is_some() {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                err = Some(Error::Io(e));
                break;
            }
        }
    }

    // ⛔ The mode is set on the DESCRIPTOR, after the content and before the
    // close. `fchmodat` on the name would re-resolve it, and the name is in a
    // tree this process is still writing to.
    let want_final = mode & 0o7777;
    let widened = if want_final != want {
        // The image's mode does not include owner write. It is applied now that
        // the content is written, and the sidecar records both.
        let _ = sys::fchmod(fd, want_final as u64);
        None
    } else {
        None
    };
    let _ = sys::close(fd);
    if let Some(e) = err {
        return Err(e);
    }
    Ok(widened)
}

/// ⭐ T-0306: re-permission between layers, or the second layer fails.
///
/// Ownership-neutral extraction leaves directories with the image's modes and
/// the extractor's ownership. A mode-0555 directory in layer 1 cannot be
/// written into when layer 2 extracts over it, and the failure looks like a
/// corrupt layer.
/// `references/indigo-dc__udocker/tree/udocker/container/structure.py:296-303`
/// runs a `find` after every layer that adds `u+x` to directories missing it,
/// `u+w` and `u+r` to anything missing them, and `chgrp` to the caller's own
/// gid. Its docstring at
/// `references/indigo-dc__udocker/tree/udocker/container/structure.py:265-267`
/// states the reason: "permissions are changed to avoid file permission issues
/// when extracting the next layer".
///
/// ⛔ **Owner bits only, and only on entries this extraction wrote.** Widening
/// group or other bits changes what the image means for a payload that reads
/// modes, and widening everything makes the sidecar the only record of the real
/// image. ⛔ **No `chgrp`**: podvm extracts as the only mapped id already, so
/// udocker's third action is one podvm has no reason to take.
fn repermission(root: &Dir, dirs: &[(Vec<String>, u32)]) -> Result<u64> {
    let mut widened = 0;
    for (parts, mode) in dirs {
        let want = mode | 0o700;
        if want == *mode {
            continue;
        }
        let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
        let Ok(Ok((parent, name))) = safety::open_parent(root, &refs, false) else {
            continue;
        };
        let Ok(n) = sys::CBuf::new(&name).ok_or(()) else {
            continue;
        };
        if sys::fchmodat(parent.fd(), &n, (want & 0o7777) as u64, 0).is_ok() {
            widened += 1;
        }
    }
    Ok(widened)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ THE MEASURED CASE, and the row T-0302's `Prove` reads back.
    /// `experiments/results/whiteout-contract.txt` check B: alpine's
    /// `etc/shadow` is uid 0, gid 42. Extracting as uid 0 gid 0 leaves the uid
    /// alone and drops the gid, and the reason says which.
    #[test]
    fn shadow_keeps_its_uid_and_reports_the_dropped_gid() {
        let ids = Ids { uid: 0, gid: 0 };
        let (u, g, why) = resolve_ids(0, 42, ids);
        assert_eq!((u, g), (0, 0));
        assert_eq!(why.as_deref(), Some("gid 42 unmapped"));
    }

    /// ⚠ A file the image already owns as the extracting id has nothing
    /// dropped, and must not carry a reason: a sidecar where every row claims a
    /// gap is one nobody reads.
    #[test]
    fn an_already_matching_owner_records_no_reason() {
        let ids = Ids { uid: 0, gid: 0 };
        assert_eq!(resolve_ids(0, 0, ids).2, None);
    }

    /// ⛔ The ids are the PROCESS's, not a hard-coded 0. Extracting as an
    /// ordinary user records that user, and the reason names both gaps.
    #[test]
    fn extracting_as_a_non_root_id_records_that_id() {
        let ids = Ids {
            uid: 1000,
            gid: 1000,
        };
        let (u, g, why) = resolve_ids(0, 42, ids);
        assert_eq!((u, g), (1000, 1000));
        let why = why.unwrap();
        assert!(why.contains("uid 0 unmapped"), "{why}");
        assert!(why.contains("gid 42 unmapped"), "{why}");
    }
}
