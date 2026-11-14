//! The apply of one layer, entry by entry.
//!
//! **Entry level, never `unpack()`.** A `tar` process brings its ownership
//! rules, its exit codes and its path rules: rurima runs `tar -xpf`, which
//! keeps permissions and ownership, and its `docker pull` stops there. udocker
//! also runs `tar` and works only because it gives `--no-same-owner` and
//! `--no-same-permissions`: the flags are the mechanism, the `tar` process is
//! not. The `unpack()` of a library is the same mistake in this process: it
//! makes the ownership, permission and path decisions that this crate makes.
//!
//! **The layer order.** The whiteouts of layer N apply to the tree of the
//! layers before it, **before** the entries of N (udocker's order). A layer
//! can then remove a path and make it again. The other order removes what
//! the same layer wrote.

use std::io::Read;

use pod_hostprobe::sys;

use crate::error::{self, Result, cbuf, refuse};
use crate::safety::{self, Dir};
use crate::sidecar::{Meta, Sidecar};
use crate::whiteout::{self, Kind};

/// What one layer did, for the transcript and for the record.
#[derive(Debug, Default, Clone)]
pub struct LayerStats {
    /// The members of the archive that the crate read, the root member
    /// (`./`) and the whiteouts included.
    pub entries: u64,
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub hardlinks: u64,
    pub whiteouts: u64,
    pub opaques: u64,
    /// The entries of a type that the crate does not make, counted and named:
    /// a device node in a layer is a thing that the crate could not make, and
    /// a caller that sees `0` then has a false count. The root member is not
    /// one: it names the destination, which exists.
    pub skipped: u64,
    pub skipped_kinds: Vec<String>,
    /// How many directories the permission pass of this layer widened.
    pub widened: u64,
    /// Each directory entry of this layer, with the mode of the image, in
    /// the order of the layer. The pass after the last layer sets these
    /// modes ([`crate::modes::DirModes`]).
    pub dir_modes: Vec<(Vec<String>, u32)>,
}

/// The ids that this process applies, read one time.
///
/// From the kernel, not a fixed 0: a caller can run as uid 0 in a namespace
/// or as a user, and a fixed 0 writes a sidecar that claims an id that the
/// kernel did not apply.
#[derive(Debug, Clone, Copy)]
pub struct Ids {
    pub uid: u64,
    pub gid: u64,
}

impl Ids {
    pub fn current() -> Ids {
        // `getuid(2)` and `getgid(2)` cannot fail. An id is 32 bits on each
        // target: the mask keeps an id above 2^31 that a 32-bit target gives
        // with its sign extended.
        // SAFETY: the two calls take no argument and touch no memory.
        let (uid, gid) = unsafe {
            (sys::syscall6(sys::SYS_GETUID, 0, 0, 0, 0, 0, 0), sys::syscall6(sys::SYS_GETGID, 0, 0, 0, 0, 0, 0))
        };
        Ids { uid: uid as u64 & u64::from(u32::MAX), gid: gid as u64 & u64::from(u32::MAX) }
    }
}

/// Apply one layer to the tree of the layers before it.
///
/// The whiteouts of the layer were collected by [`crate::remove::collect`]
/// and applied by [`crate::remove::apply`] before this pass: this pass
/// counts the markers and does not make them.
pub fn apply_layer(
    root: &Dir,
    digest: &str,
    stream: &mut dyn Read,
    ids: Ids,
    sidecar: &mut Sidecar,
) -> Result<LayerStats> {
    let mut st = LayerStats::default();
    // Only the directories that THIS layer wrote get the permission pass, so
    // the mode of a directory that the layer did not touch stays.
    let mut wrote_dirs: Vec<(Vec<String>, u32)> = Vec::new();

    let mut ar = tar::Archive::new(stream);
    // `entries()`: each decision below is made here, not by the library.
    for entry in ar.entries().map_err(error::io)? {
        let mut entry = entry.map_err(error::io)?;
        let header = entry.header().clone();
        let raw = entry.path_bytes().to_vec();
        let path = String::from_utf8_lossy(&raw).into_owned();
        st.entries += 1;

        // The whiteouts applied before the entries of this layer, and the
        // marker itself is never made.
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

        // **The root of the archive is an entry, not a refusal.** A tar made
        // with a `./` prefix has `./` as its first member, and it names the
        // destination: there is nothing to make and nothing to refuse. The
        // Debian images of `public.ecr.aws` have it; a refusal there refuses
        // each layer of each Debian image. A directory with an empty name is
        // the root too (PodUtils/podvm#261). Another entry with an empty name
        // is still refused, as the crate cannot put it anywhere. It is not
        // in `skipped`: the image lost nothing (PodUtils/podvm#352).
        if safety::is_root_entry(&path, header.entry_type().is_dir()) {
            continue;
        }
        let parts = match safety::components(&path) {
            Ok(p) => p,
            Err(r) => return Err(refuse(digest, &path, r)),
        };

        let kind = header.entry_type();
        let mode = header.mode().unwrap_or(0o644);
        let uid = header.uid().unwrap_or(0);
        let gid = header.gid().unwrap_or(0);

        // The parent is opened component by component with symlinks refused:
        // this stops `evil/passwd` after `evil -> /etc`.
        let (parent, name) = match safety::open_parent(root, &parts, true)? {
            Ok(x) => x,
            Err(r) => return Err(refuse(digest, &path, r)),
        };

        let mut applied_mode = None;
        if kind.is_dir() {
            make_dir(&parent, &name, mode)?;
            st.dirs += 1;
            let full: Vec<String> = parts.iter().map(|s| (*s).to_string()).collect();
            wrote_dirs.push((full, mode));
        } else if kind.is_symlink() {
            let target = entry.link_name_bytes().map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
            // An absolute target is relative to the root file system, not a
            // refusal. The rebased result gets the containment check, and a
            // target that still goes out after the rebase IS refused.
            let dir_parts: Vec<&str> = parts[..parts.len() - 1].to_vec();
            if let Err(r) = safety::rebase_symlink_target(&dir_parts, &target) {
                return Err(refuse(digest, &path, r));
            }
            replace(&parent, &name)?;
            let t = cbuf(&target)?;
            let n = cbuf(&name)?;
            // The link gets the target that the image wrote, with no change.
            // The rebase decides if the link is ALLOWED, not what is stored: a
            // stored rebased form puts the root of this extraction into the
            // image.
            sys::symlinkat(&t, parent.fd(), &n)
                .map_err(|e| error::sys(format!("cannot create the symlink {path:?}"), e))?;
            st.symlinks += 1;
        } else if kind.is_hard_link() {
            let target = entry.link_name_bytes().map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
            // The target of a hard link resolves in the destination, and a
            // target outside it is refused: the payload can write through a
            // hard link out of the root file system.
            let tparts = match safety::components(&target) {
                Ok(p) => p,
                Err(r) => return Err(refuse(digest, &path, r)),
            };
            let (tparent, tname) = match safety::open_parent(root, &tparts, false)? {
                Ok(x) => x,
                Err(r) => return Err(refuse(digest, &path, r)),
            };
            replace(&parent, &name)?;
            let tn = cbuf(&tname)?;
            let n = cbuf(&name)?;
            sys::linkat(tparent.fd(), &tn, parent.fd(), &n, 0).map_err(|e| {
                error::sys(format!("cannot link {path:?} to {target:?}, which the layer says is already extracted"), e)
            })?;
            st.hardlinks += 1;
        } else if kind.is_file() {
            let m = write_file(&parent, &name, mode, &mut entry)?;
            applied_mode = m;
            st.files += 1;
        } else {
            // A device node, a fifo or a socket. A rootless process cannot
            // `mknod` them, so the crate counts and names them and does not
            // try and fail.
            st.skipped += 1;
            let k = format!("{:?}", kind);
            if !st.skipped_kinds.contains(&k) {
                st.skipped_kinds.push(k);
            }
            continue;
        }

        // The sidecar row, for each entry that was made.
        let (applied_uid, applied_gid, reason) = resolve_ids(uid, gid, ids);
        let row = Meta { path: parts.join("/"), uid, gid, mode, applied_uid, applied_gid, reason, applied_mode };
        if kind.is_dir() { sidecar.push_dir(&row) } else { sidecar.push(&row) }.map_err(error::io)?;
    }

    // The permission pass, after the layer and on what this layer wrote.
    st.widened = repermission(root, &wrote_dirs)?;
    st.dir_modes = wrote_dirs;
    Ok(st)
}

/// The ownership decision for one entry.
///
/// The crate does not restore ownership: the applied ids are the ids of the
/// extracting process. The reason names each difference, as a sidecar row
/// that differs from the image with no word is what this crate must not
/// write.
fn resolve_ids(uid: u64, gid: u64, ids: Ids) -> (u64, u64, Option<String>) {
    let mut why = Vec::new();
    if uid != ids.uid {
        why.push(format!("uid {uid} unmapped"));
    }
    if gid != ids.gid {
        why.push(format!("gid {gid} unmapped"));
    }
    (ids.uid, ids.gid, if why.is_empty() { None } else { Some(why.join(", ")) })
}

fn make_dir(parent: &Dir, name: &str, mode: u32) -> Result<()> {
    let n = cbuf(name)?;
    // Made with the owner bits on. A directory made 0555 cannot take the next
    // entry of THIS layer. The pass after the last layer sets the mode of
    // the image.
    let open = u64::from(mode | 0o700) & 0o7777;
    match sys::mkdirat(parent.fd(), &n, open) {
        Ok(_) => return Ok(()),
        Err(e) if e == sys::EEXIST => {}
        Err(e) => return Err(error::sys(format!("cannot create the directory {name:?}"), e)),
    }
    // A directory of an earlier layer, or of an earlier extraction whose
    // last pass closed it (0555): it gets the owner bits, or the entries
    // below it cannot be written. Through a descriptor with `O_NOFOLLOW`:
    // a mode change by the name follows a symlink out of the tree.
    let flags = sys::O_RDONLY | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
    match sys::openat(parent.fd(), &n, flags, 0) {
        Ok(fd) => {
            // A refusal shows at the first write below this directory.
            let _ = sys::fchmod(fd, open);
            let _ = sys::close(fd);
            return Ok(());
        }
        Err(e) if e == sys::ENOTDIR || e == sys::ELOOP => {}
        Err(e) => return Err(error::sys(format!("cannot open the directory {name:?}"), e)),
    }
    // A symlink or a file of an earlier layer. The image says directory
    // here: remove the name first, as for a file, then make the directory.
    replace(parent, name)?;
    sys::mkdirat(parent.fd(), &n, open)
        .map(|_| ())
        .map_err(|e| error::sys(format!("cannot create the directory {name:?}"), e))
}

/// Remove the entry at this name before the write.
///
/// A later layer that puts a symlink in place of a file, or a directory in
/// place of a symlink, is ordinary OCI. An `O_TRUNC` open of a SYMLINK
/// follows it and writes where it points: the escape that the containment
/// refuses on the other path.
fn replace(parent: &Dir, name: &str) -> Result<()> {
    let n = cbuf(name)?;
    match sys::unlinkat(parent.fd(), &n, 0) {
        Ok(_) => Ok(()),
        Err(e) if e == sys::ENOENT => Ok(()),
        // A file in place of a directory: remove the directory when it is
        // empty, and keep it when it is not, as a union file system does.
        Err(e) if e == sys::EISDIR => {
            let _ = sys::unlinkat(parent.fd(), &n, sys::AT_REMOVEDIR);
            Ok(())
        }
        Err(e) => Err(error::sys(format!("cannot replace {name:?}"), e)),
    }
}

/// Write one regular file. Returns the mode that stayed on the file when it is
/// not the mode of the image: the kernel refused the mode of the image after
/// the write, or dropped its `S_ISUID` or `S_ISGID`.
fn write_file(parent: &Dir, name: &str, mode: u32, r: &mut dyn Read) -> Result<Option<u32>> {
    replace(parent, name)?;
    let n = cbuf(name)?;
    // O_EXCL: `replace` removed the old entry, so a file at this name now is
    // a file that another process made between the two calls; the crate does
    // not write over it. O_NOFOLLOW: a second guard against a symlink at the
    // name.
    let want = (mode | 0o600) & 0o7777;
    let fd = sys::openat(
        parent.fd(),
        &n,
        sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL | sys::O_CLOEXEC | sys::O_NOFOLLOW,
        u64::from(want),
    )
    .map_err(|e| error::sys(format!("cannot create {name:?}"), e))?;

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
                            err = Some(error::sys(format!("a write of {name:?} returned 0 with bytes left"), sys::EIO));
                            break;
                        }
                        Err(e) if e == sys::EINTR => continue,
                        Err(e) => {
                            err = Some(error::sys(format!("cannot write {name:?}"), e));
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
                err = Some(error::io(e));
                break;
            }
        }
    }

    // The mode is set on the DESCRIPTOR, after the content and before the
    // close. An `fchmodat` of the name resolves it again, in a tree that this
    // process still writes.
    let want_final = mode & 0o7777;
    // Each time, not only when the mode differs from the open: the umask of
    // the process cut the mode of the open, a mode of the image with no
    // owner write is set only now, and a write by a process with no
    // CAP_FSETID cleared S_ISUID and S_ISGID.
    let refused = sys::fchmod(fd, u64::from(want_final)).is_err();
    let kept = kept_mode(fd, want_final, refused.then_some(want));
    let _ = sys::close(fd);
    if let Some(e) = err {
        return Err(e);
    }
    Ok(kept)
}

/// The mode that stayed on the descriptor when it is not `want`, for the
/// sidecar. Read back, not assumed: the kernel clears `S_ISGID` with no
/// error when the group of the file is not a group of this process.
/// `fallback` is the answer when the read fails.
fn kept_mode(fd: i64, want: u32, fallback: Option<u32>) -> Option<u32> {
    match sys::fstatat(fd, &sys::cempty(), sys::AT_EMPTY_PATH) {
        Ok(st) if st.st_mode & 0o7777 == want => None,
        Ok(st) => Some(st.st_mode & 0o7777),
        Err(_) => fallback,
    }
}

/// The permission pass between layers, or the next layer fails.
///
/// An extraction with no ownership change leaves each directory with the
/// mode of the image and the owner of the extracting process. A directory of
/// mode 0555 in layer 1 cannot take the entries of layer 2, and the failure
/// looks like a corrupt layer. udocker runs a `find` after each layer that
/// adds `u+x` to directories, `u+w` and `u+r` to each entry, and a `chgrp` to
/// the gid of the caller, "to avoid file permission issues when extracting
/// the next layer".
///
/// **Owner bits only, and only on the directories that this extraction
/// wrote.** Wider group or other bits change what the image means for a
/// payload that reads modes. **No `chgrp`**: the crate extracts as the only
/// mapped id already.
///
/// **Each directory, not only the widened ones**: the umask of the process
/// cut the mode of `mkdirat`, and a directory that an earlier layer made
/// keeps its mode until this pass sets the mode of this layer.
///
/// The widened owner bits stay until the pass after the last layer
/// ([`crate::modes::DirModes::set`]).
fn repermission(root: &Dir, dirs: &[(Vec<String>, u32)]) -> Result<u64> {
    let mut widened = 0;
    for (parts, mode) in dirs {
        let want = mode | 0o700;
        let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
        let Ok(Ok((parent, name))) = safety::open_parent(root, &refs, false) else {
            continue;
        };
        let Some(n) = sys::CBuf::new(&name) else {
            continue;
        };
        // Through a descriptor of the directory, never `fchmodat` of the
        // name: a later entry of the layer can put a symlink at the name, and
        // `fchmodat` follows it to a file of the host. A name that is not a
        // directory now is not changed.
        let Ok(fd) =
            sys::openat(parent.fd(), &n, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC, 0)
        else {
            continue;
        };
        if sys::fchmod(fd, u64::from(want & 0o7777)).is_ok() && want != *mode {
            widened += 1;
        }
        let _ = sys::close(fd);
    }
    Ok(widened)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured case: `etc/shadow` of alpine is uid 0, gid 42. An
    /// extraction as uid 0 gid 0 keeps the uid and drops the gid, and the
    /// reason says which.
    #[test]
    fn shadow_keeps_its_uid_and_reports_the_dropped_gid() {
        let ids = Ids { uid: 0, gid: 0 };
        let (u, g, why) = resolve_ids(0, 42, ids);
        assert_eq!((u, g), (0, 0));
        assert_eq!(why.as_deref(), Some("gid 42 unmapped"));
    }

    /// A file that the image gives to the extracting id has nothing dropped and
    /// no reason: a sidecar in which each row claims a gap is not read.
    #[test]
    fn an_already_matching_owner_records_no_reason() {
        let ids = Ids { uid: 0, gid: 0 };
        assert_eq!(resolve_ids(0, 0, ids).2, None);
    }

    /// The ids are the ids of the PROCESS, not a fixed 0. An extraction as a
    /// user records that user, and the reason names both gaps.
    #[test]
    fn extracting_as_a_non_root_id_records_that_id() {
        let ids = Ids { uid: 1000, gid: 1000 };
        let (u, g, why) = resolve_ids(0, 42, ids);
        assert_eq!((u, g), (1000, 1000));
        let why = why.unwrap();
        assert!(why.contains("uid 0 unmapped"), "{why}");
        assert!(why.contains("gid 42 unmapped"), "{why}");
    }
}
