use super::*;
use core::sync::atomic::AtomicBool;
use linux_raw_sys::general::{AT_STATX_SYNC_AS_STAT, STATX_BASIC_STATS, statx as KernelStatx};

/// Set when the kernel answered `ENOSYS` to the stat call of this
/// architecture: later calls go straight to `statx(2)`.
pub(super) static NO_FSTATAT: AtomicBool = AtomicBool::new(false);

/// `statx(2)` into the same `Stat` that `newfstatat(2)` fills.
///
/// A loongarch64 kernel older than 6.11 has no `newfstatat`, and it has
/// `statx`. The flags are the `AT_*` flags of [`fstatat`]; they have the
/// same values in both calls. For a descriptor, give an empty path and
/// `AT_EMPTY_PATH`.
pub fn statx_at(dirfd: i64, path: &CBuf, flags: u64) -> Result<Stat, Errno> {
    let mut raw = unsafe { core::mem::zeroed::<KernelStatx>() };
    let flags = flags | AT_STATX_SYNC_AS_STAT as u64;
    let args = [dirfd as u64, path.ptr(), flags, STATX_BASIC_STATS as u64, &mut raw as *mut KernelStatx as u64, 0];
    unsafe { sys(SYS_STATX, args)? };
    Ok(stat_from_statx(&raw))
}

/// The encoding of `st_dev` and `st_rdev` that `newfstatat` writes (the
/// one of glibc `makedev`), so both paths give equal numbers.
fn makedev(major: u32, minor: u32) -> u64 {
    let (major, minor) = (major as u64, minor as u64);
    ((major & 0xfff) << 8) | (minor & 0xff) | ((minor & !0xff) << 12) | ((major & !0xfff) << 32)
}

fn stat_from_statx(raw: &KernelStatx) -> Stat {
    Stat {
        st_dev: makedev(raw.stx_dev_major, raw.stx_dev_minor),
        st_ino: raw.stx_ino,
        st_mode: raw.stx_mode as u32,
        st_uid: raw.stx_uid,
        st_gid: raw.stx_gid,
        st_rdev: makedev(raw.stx_rdev_major, raw.stx_rdev_minor),
        st_size: raw.stx_size as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(dirfd: i64, path: &str, flags: u64) {
        let c = CBuf::new(path).unwrap();
        let b = statx_at(dirfd, &c, flags).expect("statx");
        // No `newfstatat` here (loongarch64 before 6.11): `statx` is the only path.
        let Ok(a) = fstatat_direct(dirfd, &c, flags) else { return };
        let key = |s: &Stat| (s.st_dev, s.st_ino, s.st_mode, s.st_uid, s.st_gid, s.st_rdev, s.st_size);
        assert_eq!(key(&a), key(&b), "{path} flags {flags:#x}");
        assert_ne!(b.st_ino, 0);
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pod-hostprobe-statx-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_statx_buffer_holds_what_the_kernel_writes() {
        assert!(core::mem::size_of::<KernelStatx>() >= 256);
    }

    #[test]
    fn statx_gives_the_stat_of_newfstatat_for_a_file_a_directory_and_a_char_device() {
        let d = temp("dir");
        let f = d.join("file");
        std::fs::write(&f, b"0123456789").unwrap();
        same(AT_FDCWD as i64, f.to_str().unwrap(), 0);
        same(AT_FDCWD as i64, d.to_str().unwrap(), 0);
        same(AT_FDCWD as i64, "/dev/null", 0);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn statx_with_no_follow_stats_the_link_and_not_its_target() {
        let d = temp("link");
        let l = d.join("link");
        let _ = std::fs::remove_file(&l);
        std::os::unix::fs::symlink("/dev/null", &l).unwrap();
        same(AT_FDCWD as i64, l.to_str().unwrap(), AT_SYMLINK_NOFOLLOW);
        same(AT_FDCWD as i64, l.to_str().unwrap(), 0);
        let c = CBuf::new(l.to_str().unwrap()).unwrap();
        assert_eq!(statx_at(AT_FDCWD as i64, &c, AT_SYMLINK_NOFOLLOW).unwrap().kind(), "symlink");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn statx_with_an_empty_path_stats_the_descriptor() {
        use std::os::fd::AsRawFd;
        let d = temp("fd");
        let f = d.join("fd-file");
        std::fs::write(&f, b"abc").unwrap();
        let file = std::fs::File::open(&f).unwrap();
        same_fd(file.as_raw_fd() as i64);
        let dir = std::fs::File::open(&d).unwrap();
        same_fd(dir.as_raw_fd() as i64);
        std::fs::remove_dir_all(&d).unwrap();
    }

    fn same_fd(fd: i64) {
        same(fd, "", AT_EMPTY_PATH);
    }
}
