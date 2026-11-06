//! `TODO/image.md` T-0203: check blocks **and inodes**, before any download and
//! again before any extraction, and name the destination in the message.
//!
//! ⭐ The defect this exists to avoid is in the corpus at file and line.
//! `references/mhx__dwarfs/tree/src/utility/filesystem_extractor.cpp:544-552`
//! takes `rv` from `write_range_data`, which returns a libarchive
//! `la_ssize_t`, then formats it into `"short write: {} != {}"` as though it
//! were a byte count. `-20` is `ARCHIVE_WARN`, a status code; the underlying
//! `archive_errno` is `ENOSPC`. The message names neither the errno nor the
//! path, so the reader learns nothing they can act on.
//!
//! ⚠ The same trap is in the corpus as a shipped default:
//! `references/VHSgunzo__memfd-exec/tree/src/executable.rs:580-584` falls back
//! to `env::temp_dir()`, then `/dev/shm`, then `$HOME/.cache`, with no space
//! check on any of them.
//!
//! ⛔ Refuse up front rather than streaming until `ENOSPC`. A partial
//! extraction has to be cleaned up on a filesystem that is already full, which
//! is the state in which cleanup is least likely to work.

use podvm_probe::sys::{self, CBuf};

use crate::error::{Error, Result};

/// This process's file-size ceiling (`RLIMIT_FSIZE`), or `None` where
/// there is none (infinity) or it could not be read.
///
/// ⚠ Unlike free space, which the whole pull shares, the ceiling binds
/// every blob on its own: one file may never pass it, however much room
/// the filesystem has. An unreadable ceiling means no bound rather than
/// a refusal: the pull worked without one before this check existed,
/// and a kernel that will not answer must not break it.
pub fn file_ceiling() -> Option<u64> {
    match sys::prlimit(sys::RLIMIT_FSIZE) {
        Ok((cur, _)) if cur != u64::MAX => Some(cur),
        _ => None,
    }
}

/// What one filesystem has left. `statfs(2)` is the syscall behind `statvfs(3)`
/// and is what `crates/podvm-probe/src/writable.rs` already calls, so there is
/// one read path for free space in this tree and not two.
#[derive(Debug, Clone, Copy)]
pub struct Free {
    pub bytes: u64,
    pub inodes: u64,
    pub block_size: u64,
}

/// ⚠ Some filesystems report `f_files == 0`, meaning inodes are allocated
/// dynamically and there is no fixed count. `f_ffree` is then meaningless
/// rather than zero, and a check that read it as zero would refuse every write
/// on tmpfs.
#[derive(Debug, Clone, Copy)]
pub struct Have {
    pub free: Free,
    pub inodes_are_counted: bool,
}

pub fn read(path: &str) -> Result<Have> {
    let Some(c) = CBuf::new(path) else {
        return Err(Error::Store(format!(
            "{path:?} contains a NUL and cannot reach the kernel"
        )));
    };
    let s = sys::statfs(&c).map_err(|e| {
        Error::NoSpace(format!(
            "statfs({path}) failed with {} ({}), so the free space at the \
             destination is unknown and podvm will not start a download it \
             cannot size",
            e.name(),
            e.0
        ))
    })?;
    let block_size = if s.f_bsize > 0 { s.f_bsize as u64 } else { 0 };
    Ok(Have {
        free: Free {
            bytes: s.f_bavail.saturating_mul(block_size),
            inodes: s.f_ffree,
            block_size,
        },
        inodes_are_counted: s.f_files > 0,
    })
}

/// The bytes under a directory, counted by walking it. Symlinks are
/// never followed: a rootfs holds absolute links into itself (which
/// would double-count) and relative links out of it (which would escape
/// the store). An unreadable entry counts nothing rather than failing
/// the report: TODO/cli.md T-1337's `system df` never exits non-zero
/// for accounting alone.
pub fn dir_bytes(path: &std::path::Path) -> u64 {
    use std::collections::HashSet;
    use std::os::unix::fs::MetadataExt;
    let mut total = 0u64;
    // Hardlinked files share one inode: counting each name would bill
    // the same bytes twice, so an inode already seen adds nothing.
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(meta) = std::fs::symlink_metadata(e.path()) else {
                continue;
            };
            if meta.is_dir() && !meta.is_symlink() {
                stack.push(e.path());
            } else if meta.is_file() && seen.insert((meta.dev(), meta.ino())) {
                total = total.saturating_add(meta.len());
            }
        }
    }
    // The top directory itself is not counted: only what it holds is
    // content, and a df that billed the directory entry would disagree
    // with the store's own accounting by exactly that entry.
    total
}

/// Bytes rendered in the unit they are in. ⚠ `docs/conventions/code.md`: binary
/// units where they are binary, and never a value printed in one unit and
/// labelled the other.
pub fn mib(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kib = bytes as f64 / 1024.0;
    if kib < 1024.0 {
        return format!("{kib:.1} KiB");
    }
    let m = kib / 1024.0;
    if m < 1024.0 {
        format!("{m:.1} MiB")
    } else {
        format!("{:.2} GiB", m / 1024.0)
    }
}

/// ⛔ The precheck. Both halves, and the message names the destination, the
/// free amount, the required amount and the unit.
///
/// `headroom_bytes` is what is asked for beyond the payload, so a store is not
/// filled to its last block by a pull that then leaves nothing for the
/// extraction M2 performs.
pub fn require(
    destination: &str,
    what: &str,
    need_bytes: u64,
    need_inodes: u64,
    headroom_bytes: u64,
) -> Result<Have> {
    let have = read(destination)?;
    let want = need_bytes.saturating_add(headroom_bytes);
    if have.free.bytes < want {
        return Err(Error::NoSpace(format!(
            "not enough space at {destination} for {what}: {} free, {} needed \
             ({} of payload plus {} of headroom). Set $PODVM_STORE to a \
             directory with room, or free space at {destination}",
            mib(have.free.bytes),
            mib(want),
            mib(need_bytes),
            mib(headroom_bytes),
        )));
    }
    if have.inodes_are_counted && have.free.inodes < need_inodes {
        return Err(Error::NoSpace(format!(
            "not enough inodes at {destination} for {what}: {} free, {need_inodes} \
             needed. This filesystem has {} of free space and cannot create the \
             files anyway, which is the failure that otherwise arrives as ENOSPC \
             with plenty of bytes left",
            have.free.inodes,
            mib(have.free.bytes),
        )));
    }
    Ok(have)
}

/// A candidate destination and what it has, for choosing between them.
pub struct Candidate {
    pub path: String,
    pub source: String,
    pub have: Have,
}

/// ⛔ Choose by free space among paths a probe **obtained by writing**, rather
/// than taking `$TMPDIR`. On the runtime podvm targets `/tmp` is 64 MiB, which
/// almost no image fits in, and it is exactly what `$TMPDIR` names.
///
/// `writable` is `(path, source)` from the probe's write allowlist; a path that
/// cannot be `statfs`'d is dropped with its reason rather than ranked last,
/// because an unknown free space is not a small one.
pub fn choose<'a>(
    writable: impl IntoIterator<Item = (&'a str, &'a str)>,
    dropped: &mut Vec<String>,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for (path, source) in writable {
        match read(path) {
            Ok(have) => out.push(Candidate {
                path: path.to_string(),
                source: source.to_string(),
                have,
            }),
            Err(e) => dropped.push(format!("{path}: {e}")),
        }
    }
    out.sort_by(|a, b| {
        b.have
            .free
            .bytes
            .cmp(&a.have.free.bytes)
            .then(a.path.cmp(&b.path))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmp_reports_blocks_and_inodes_on_this_machine() {
        // The shipping path against a real filesystem, not a double.
        let have = read("/tmp").unwrap();
        assert!(have.free.block_size > 0);
    }

    #[test]
    fn a_file_ceiling_is_absent_or_positive_never_zero() {
        // Environment-dependent (this lane reads infinity), so the test
        // pins the invariant, not the value: a zero ceiling would refuse
        // every file including the empty one.
        assert!(file_ceiling().is_none_or(|c| c > 0));
    }

    #[test]
    fn a_destination_that_does_not_exist_is_an_error_naming_it() {
        let e = read("/proc/self/no-such-directory").unwrap_err();
        let text = format!("{e}");
        assert!(text.contains("/proc/self/no-such-directory"), "{text}");
        assert!(text.contains("ENOENT"), "{text}");
    }

    #[test]
    fn a_requirement_over_the_free_space_names_all_four_things() {
        // ⭐ The whole point of the entry: destination, free, required, unit.
        let e = require("/tmp", "alpine:latest", u64::MAX / 4, 1, 0).unwrap_err();
        let text = format!("{e}");
        assert!(text.contains("/tmp"), "{text}");
        assert!(text.contains("free"), "{text}");
        assert!(text.contains("needed"), "{text}");
        assert!(text.contains("iB"), "{text}");
    }

    #[test]
    fn a_requirement_the_machine_can_meet_returns_what_it_has() {
        let have = require("/tmp", "one byte", 1, 1, 0).unwrap();
        assert!(have.free.bytes >= 1);
    }

    #[test]
    fn an_inode_requirement_is_checked_only_where_inodes_are_counted() {
        // ⚠ tmpfs reports f_files == 0. Reading f_ffree as zero there would
        // refuse every write on a filesystem with room.
        let have = read("/dev/shm").or_else(|_| read("/tmp")).unwrap();
        if !have.inodes_are_counted {
            assert!(require("/dev/shm", "x", 0, u64::MAX, 0).is_ok());
        }
    }

    #[test]
    fn candidates_come_back_largest_first_and_an_unreadable_one_is_dropped_with_its_reason() {
        let mut dropped = Vec::new();
        let got = choose(
            [
                ("/tmp", "the candidate list"),
                ("/proc/self/no-such-directory", "the candidate list"),
            ],
            &mut dropped,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, "/tmp");
        assert_eq!(dropped.len(), 1);
        assert!(dropped[0].contains("no-such-directory"), "{:?}", dropped);
    }

    #[test]
    fn a_byte_count_is_printed_in_the_unit_it_is_in() {
        assert_eq!(mib(512), "512 B");
        assert_eq!(mib(1024), "1.0 KiB");
        assert_eq!(mib(3 * 1024 * 1024), "3.0 MiB");
        assert_eq!(mib(2 * 1024 * 1024 * 1024), "2.00 GiB");
    }

    /// T-1337: the walk counts file bytes once each, never follows a
    /// link, and never fails the report for one bad entry.
    #[test]
    fn dir_bytes_counts_once_and_never_follows() {
        let d = std::env::temp_dir().join(format!("podvm-du-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a"), vec![7u8; 10]).unwrap();
        std::fs::write(d.join("sub/b"), vec![7u8; 6]).unwrap();
        // A symlink out and a symlink in: neither is followed.
        std::os::unix::fs::symlink("/etc/hostname", d.join("out")).unwrap();
        std::os::unix::fs::symlink("a", d.join("in")).unwrap();
        // A hard link to `a`: the same inode billed once.
        std::fs::hard_link(d.join("a"), d.join("hard")).unwrap();
        assert_eq!(dir_bytes(&d), 16);
        assert_eq!(dir_bytes(&d.join("absent")), 0);
        let _ = std::fs::remove_dir_all(&d);
    }
}
