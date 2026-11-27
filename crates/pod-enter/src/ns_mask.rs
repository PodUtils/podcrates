//! The masks of the proc of the namespace rung for a payload of kuid 0
//! (T-1636, PodUtils/podvm#695).
//!
//! The lock (`ns_lock`) keeps a payload of host root at kuid 0 with no
//! capability of the initial user namespace. The kernel checks the files of
//! `/proc/sys` by `GLOBAL_ROOT_UID`, not by a capability: such a payload can
//! write `/proc/sys/kernel/core_pattern` (a helper that runs as real root)
//! and `/proc/sysrq-trigger`. So where the effective uid of the caller maps
//! to uid 0, the child binds the paths that docker keeps read-only
//! ([`PROC_READ_ONLY`]) onto themselves and remounts each read-only, and puts a
//! read-only tmpfs on each directory and a bind of `/dev/null` on each file
//! that docker masks ([`PROC_MASKED`]). This comes after the mounts of `/proc` and
//! before the pivot: the nested mount namespace of the lock then locks each
//! of them, so the payload cannot remount or unmount one.
//!
//! A path that the proc does not have is skipped; each other refusal stops
//! the payload. Where `/proc` of the image is not a proc (its mount was
//! refused), there is nothing to mask.

use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::ns_dev_bind::{mount_data_on_fd, mount_fd_on_fd, mount_on_fd};

/// The step byte of a refused mask: the payload does not run.
pub const NS_PROC_MASK_STEP: u8 = 70;

/// The names under `/proc` that docker keeps read-only.
pub const PROC_READ_ONLY: [&str; 5] = ["bus", "fs", "irq", "sys", "sysrq-trigger"];

/// The names under `/proc` that docker masks.
pub const PROC_MASKED: [&str; 10] = [
    "acpi",
    "asound",
    "interrupts",
    "kcore",
    "keys",
    "latency_stats",
    "sched_debug",
    "scsi",
    "timer_list",
    "timer_stats",
];

/// The masks of one fork, built before it.
pub(crate) struct ProcMask {
    /// `/proc` of the tree, where the payload is kuid 0.
    proc: Option<CBuf>,
    /// Each name, and true where it stays read-only (not masked).
    names: Vec<(CBuf, bool)>,
    null: CBuf,
    tmpfs: CBuf,
    empty: CBuf,
}

/// Does the effective uid `euid` map to uid 0 in `uid_map` (the parent user
/// namespace of the caller)? Host root, and a namespace that maps uid 0 to
/// uid 0.
pub(crate) fn maps_to_root(uid_map: &str, euid: u32) -> bool {
    let lines = crate::ns_lock::map_lines(uid_map).unwrap_or_default();
    lines.iter().any(|&(inside, outside, count)| euid == inside && outside == 0 && count > 0)
}

impl ProcMask {
    /// The masks for the tree at `root`: none where the effective uid of
    /// this process does not map to uid 0.
    pub(crate) fn of(root: &str) -> ProcMask {
        let lit = |s: &str| CBuf::new(s).expect("a literal");
        let euid = unsafe { sys::syscall6(sys::SYS_GETEUID, 0, 0, 0, 0, 0, 0) } as u32;
        let root_payload = std::fs::read_to_string("/proc/self/uid_map").is_ok_and(|m| maps_to_root(&m, euid));
        let proc = root_payload.then(|| CBuf::new(&format!("{}/proc", root.trim_end_matches('/')))).flatten();
        let names =
            PROC_READ_ONLY.iter().map(|n| (lit(n), true)).chain(PROC_MASKED.iter().map(|n| (lit(n), false))).collect();
        ProcMask { proc, names, null: lit("/dev/null"), tmpfs: lit("tmpfs"), empty: sys::cempty() }
    }

    /// In the child, after the mounts of `/proc`: each mask. `Err` gives
    /// the step, the errno and the index of the name. Async-signal-safe.
    pub(crate) fn apply(&self) -> std::result::Result<(), (u8, Errno, u8)> {
        let Some(proc) = &self.proc else { return Ok(()) };
        let dir = sys::O_PATH | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
        let Ok(pfd) = sys::open(proc, dir, 0) else { return Ok(()) };
        let out = match sys::fstatfs(pfd) {
            Ok(fs) if fs.f_type == sys::PROC_SUPER_MAGIC => self.each(pfd),
            _ => Ok(()),
        };
        let _ = sys::close(pfd);
        out
    }

    fn each(&self, pfd: i64) -> std::result::Result<(), (u8, Errno, u8)> {
        for (at, (name, read_only)) in self.names.iter().enumerate() {
            let refused = |e| (NS_PROC_MASK_STEP, e, at as u8);
            let fd = match sys::openat(pfd, name, sys::O_PATH | sys::O_NOFOLLOW | sys::O_CLOEXEC, 0) {
                Ok(fd) => fd,
                Err(e) if e == sys::ENOENT => continue,
                Err(e) => return Err(refused(e)),
            };
            let r = if *read_only { self.read_only(pfd, name, fd) } else { self.mask(fd) };
            let _ = sys::close(fd);
            r.map_err(refused)?;
        }
        Ok(())
    }

    /// A bind of the file of `fd` onto itself, then a read-only remount of
    /// the bind with the flags of its mount.
    fn read_only(&self, pfd: i64, name: &CBuf, fd: i64) -> sys::Sysres {
        mount_fd_on_fd(fd, fd, sys::MS_BIND | sys::MS_REC, &self.empty)?;
        // A new open crosses into the bind: the remount needs its root.
        let top = sys::openat(pfd, name, sys::O_PATH | sys::O_NOFOLLOW | sys::O_CLOEXEC, 0)?;
        let r = sys::fstatfs(top)
            .and_then(|fs| mount_on_fd(&self.empty, top, crate::ns_volume::remount_flags(fs.f_flags), &self.empty));
        let _ = sys::close(top);
        r
    }

    /// A read-only tmpfs on a directory, a bind of `/dev/null` on a file.
    fn mask(&self, fd: i64) -> sys::Sysres {
        let st = sys::fstatat(fd, &self.empty, sys::AT_EMPTY_PATH)?;
        if st.st_mode & sys::S_IFMT == sys::S_IFDIR {
            let flags = sys::MS_RDONLY | sys::MS_NOSUID | sys::MS_NODEV | sys::MS_NOEXEC;
            mount_data_on_fd(&self.tmpfs, fd, &self.tmpfs, flags, 0)
        } else {
            mount_on_fd(&self.null, fd, sys::MS_BIND, &self.empty)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Host root, a namespace that maps uid 0 to uid 0, and the others.
    #[test]
    fn the_masks_go_where_the_uid_maps_to_root() {
        assert!(maps_to_root("         0          0 4294967295\n", 0));
        assert!(maps_to_root("0 0 1\n", 0));
        assert!(!maps_to_root("0 100000 65536\n", 0));
        assert!(!maps_to_root("0 1000 1\n", 0));
        assert!(!maps_to_root("         0          0 4294967295\n", 1000));
        assert!(!maps_to_root("", 0));
    }

    // The lists of docker's default spec, with no repeat.
    #[test]
    fn the_lists_have_no_repeat() {
        let mut all: Vec<&str> = PROC_READ_ONLY.iter().chain(PROC_MASKED.iter()).copied().collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), PROC_READ_ONLY.len() + PROC_MASKED.len());
    }
}
