//! Check blocks **and inodes**, before a download and again before an
//! extraction, and name the destination in the message.
//!
//! The defect to avoid: an extractor (dwarfs) formats a libarchive status
//! code (`-20`, `ARCHIVE_WARN`) as a byte count in `"short write: {} != {}"`.
//! The errno under it is `ENOSPC`. The message names neither the errno nor
//! the path, so the reader learns nothing to act on.
//!
//! The same trap as a shipped default: a loader (memfd-exec) falls back to
//! `env::temp_dir()`, then `/dev/shm`, then `$HOME/.cache`, with no space
//! check on any of them.
//!
//! Refuse at the start; do not stream until `ENOSPC`. A partial extraction
//! must be cleaned up on a file system that is full already, which is the
//! state in which a cleanup is least likely to work.
//!
//! A failure is a [`Failure`] at [`Stage::Host`]: [`Class::Usage`] for a path
//! that cannot reach the kernel, [`Class::Unavailable`] for no space or an
//! unknown free space.

use pod_core::{Class, Failure, Stage};

use crate::Settings;
use crate::sys::{self, CBuf};

type Result<T> = std::result::Result<T, Failure>;

/// This process's file-size ceiling (`RLIMIT_FSIZE`), or `None` where
/// there is none (infinity) or it could not be read.
///
/// Unlike free space, which the whole pull shares, the ceiling binds
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
/// and is what [`crate::writable`] calls too, so there is one read path for
/// free space in the crate and not two.
#[derive(Debug, Clone, Copy)]
pub struct Free {
    pub bytes: u64,
    pub inodes: u64,
    pub block_size: u64,
}

/// Some filesystems report `f_files == 0`, meaning inodes are allocated
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
        return Err(Failure::new(
            Stage::Host,
            Class::Usage,
            format!("{path:?} contains a NUL and cannot reach the kernel"),
        ));
    };
    let s = sys::statfs(&c).map_err(|e| {
        Failure::new(
            Stage::Host,
            Class::Unavailable,
            format!(
                "statfs({path}) failed with {} ({}), so the free space at the \
                 destination is unknown, and a download that cannot be sized \
                 does not start",
                e.name(),
                e.0
            ),
        )
    })?;
    let block_size = if s.f_bsize > 0 { s.f_bsize as u64 } else { 0 };
    Ok(Have {
        free: Free { bytes: s.f_bavail.saturating_mul(block_size), inodes: s.f_ffree, block_size },
        inodes_are_counted: s.f_files > 0,
    })
}

/// The bytes under a directory, counted by walking it. Symlinks are
/// never followed: a rootfs holds absolute links into itself (which
/// would double-count) and relative links out of it (which would escape
/// the store). An unreadable entry counts nothing and does not fail
/// the report: a `df` command does not exit non-zero for accounting
/// alone.
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

/// Bytes rendered in the unit they are in: binary units where they are
/// binary, and never a value printed in one unit and labelled the other.
pub fn mib(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kib = bytes as f64 / 1024.0;
    if kib < 1024.0 {
        return format!("{kib:.1} KiB");
    }
    let m = kib / 1024.0;
    if m < 1024.0 { format!("{m:.1} MiB") } else { format!("{:.2} GiB", m / 1024.0) }
}

/// The precheck. Both halves, and the message names the destination, the
/// free amount, the required amount and the unit.
///
/// `headroom_bytes` is what is asked for beyond the payload, so a store is not
/// filled to its last block by a pull that then leaves nothing for the
/// extraction after it. The message names `settings.directory_setting`.
pub fn require(
    settings: &Settings,
    destination: &str,
    what: &str,
    need_bytes: u64,
    need_inodes: u64,
    headroom_bytes: u64,
) -> Result<Have> {
    let have = read(destination)?;
    let want = need_bytes.saturating_add(headroom_bytes);
    if have.free.bytes < want {
        let remedy = match &settings.directory_setting {
            Some(name) => format!("Set {name} to a directory with room"),
            None => "Choose a directory with room".to_string(),
        };
        return Err(Failure::new(
            Stage::Host,
            Class::Unavailable,
            format!(
                "not enough space at {destination} for {what}: {} free, {} needed \
                 ({} of payload plus {} of headroom). {remedy}, or free space at \
                 {destination}",
                mib(have.free.bytes),
                mib(want),
                mib(need_bytes),
                mib(headroom_bytes),
            ),
        ));
    }
    if have.inodes_are_counted && have.free.inodes < need_inodes {
        return Err(Failure::new(
            Stage::Host,
            Class::Unavailable,
            format!(
                "not enough inodes at {destination} for {what}: {} free, {need_inodes} \
                 needed. This filesystem has {} of free space and cannot create the \
                 files anyway, which is the failure that otherwise arrives as ENOSPC \
                 with plenty of bytes left",
                have.free.inodes,
                mib(have.free.bytes),
            ),
        ));
    }
    Ok(have)
}

/// A candidate destination and what it has, for choosing between them.
pub struct Candidate {
    pub path: String,
    pub source: String,
    pub have: Have,
}

/// Choose by free space among paths a probe **obtained by writing**, rather
/// than taking `$TMPDIR`. On a confined target `/tmp` is 64 MiB, which almost
/// no image fits in, and it is exactly what `$TMPDIR` names.
///
/// `writable` is `(path, source)` from the probe's write allowlist; a path that
/// cannot be `statfs`'d is dropped with its reason rather than ranked last,
/// because an unknown free space is not a small one.
pub fn choose<'a>(writable: impl IntoIterator<Item = (&'a str, &'a str)>, dropped: &mut Vec<String>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for (path, source) in writable {
        match read(path) {
            Ok(have) => out.push(Candidate { path: path.to_string(), source: source.to_string(), have }),
            Err(e) => dropped.push(format!("{path}: {e}")),
        }
    }
    out.sort_by(|a, b| b.have.free.bytes.cmp(&a.have.free.bytes).then(a.path.cmp(&b.path)));
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
        // Destination, free, required, unit.
        let e = require(&Settings::default(), "/tmp", "alpine:latest", u64::MAX / 4, 1, 0).unwrap_err();
        let text = format!("{e}");
        assert!(text.contains("/tmp"), "{text}");
        assert!(text.contains("free"), "{text}");
        assert!(text.contains("needed"), "{text}");
        assert!(text.contains("iB"), "{text}");
    }

    #[test]
    fn a_requirement_over_the_free_space_names_the_setting_of_the_caller() {
        let named = Settings { directory_setting: Some("$STORE".to_string()), ..Settings::default() };
        let text = format!("{}", require(&named, "/tmp", "x", u64::MAX / 4, 1, 0).unwrap_err());
        assert!(text.contains("Set $STORE to a directory with room"), "{text}");
        let text = format!("{}", require(&Settings::default(), "/tmp", "x", u64::MAX / 4, 1, 0).unwrap_err());
        assert!(text.contains("Choose a directory with room"), "{text}");
        assert!(!text.contains('$'), "{text}");
    }

    #[test]
    fn a_requirement_the_machine_can_meet_returns_what_it_has() {
        let have = require(&Settings::default(), "/tmp", "one byte", 1, 1, 0).unwrap();
        assert!(have.free.bytes >= 1);
    }

    #[test]
    fn an_inode_requirement_is_checked_only_where_inodes_are_counted() {
        // tmpfs reports f_files == 0. Reading f_ffree as zero there would
        // refuse every write on a filesystem with room.
        let have = read("/dev/shm").or_else(|_| read("/tmp")).unwrap();
        if !have.inodes_are_counted {
            assert!(require(&Settings::default(), "/dev/shm", "x", 0, u64::MAX, 0).is_ok());
        }
    }

    #[test]
    fn candidates_come_back_largest_first_and_an_unreadable_one_is_dropped_with_its_reason() {
        let mut dropped = Vec::new();
        let got = choose(
            [("/tmp", "the candidate list"), ("/proc/self/no-such-directory", "the candidate list")],
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

    /// The walk counts file bytes once each, never follows a link, and
    /// never fails the report for one bad entry.
    #[test]
    fn dir_bytes_counts_once_and_never_follows() {
        let d = std::env::temp_dir().join(format!("pod-hostprobe-du-{}", std::process::id()));
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
