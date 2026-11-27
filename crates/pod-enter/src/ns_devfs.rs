//! The `/dev` of a read-only root (T-1639, PodUtils/podvm#695): a tmpfs, as
//! docker gives it, not the `/dev` of the image.
//!
//! A bind of `/dev` of the tree onto itself would keep it writable on the
//! disk: a write would stay in the image for the next run, and a link put
//! at `/dev/null` would stop the bind of the device there. So right after
//! the bind of the root the child mounts a tmpfs (`nosuid`, `noexec`, mode
//! 755) on `/dev` of the tree, opened with no link followed (`point_dir`),
//! and makes in it: the directory `pts` (its mount comes from
//! `ns_optional` where the image has `/dev/pts`), `shm` with a tmpfs of
//! mode 1777, and the links `fd`, `stdin`, `stdout` and `stderr`. The
//! devices of the host (`ns_dev_bind::DEVICES`, and `/dev/ptmx` where the
//! rung mounts a devpts) take a bind as for an image with no `/dev`: in a
//! user namespace of the rung through `ns_optional`, else here after the
//! optional mounts. A refused step of the tmpfs stops the payload.

use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::ns_dev_bind::{Devices, mount_data_on_fd};
use crate::ns_root::NS_DEV_KEEP_STEP;
use crate::{Names, RootDir};

/// The links of docker's `/dev`: the target, then the name.
const LINKS: [(&str, &str); 4] = [
    ("/proc/self/fd", "fd"),
    ("/proc/self/fd/0", "stdin"),
    ("/proc/self/fd/1", "stdout"),
    ("/proc/self/fd/2", "stderr"),
];

/// The tmpfs of `/dev` of one fork, built before it.
pub(crate) struct DevFs {
    dev: CBuf,
    devices: Devices,
    links: Vec<(CBuf, CBuf)>,
    pts: CBuf,
    shm: CBuf,
    tmpfs: CBuf,
    mode: CBuf,
    shm_mode: CBuf,
}

/// The directory `path` with no link followed at its last component: made
/// where it is missing, then opened `O_PATH`. A link gives `ELOOP` and a
/// file `ENOTDIR`. The components before it are the root path of the
/// caller. Async-signal-safe.
pub(crate) fn point_dir(path: &CBuf) -> std::result::Result<i64, Errno> {
    let flags = sys::O_PATH | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
    match sys::open(path, flags, 0) {
        Err(e) if e == sys::ENOENT => {}
        r => return r,
    }
    // `mkdir` does not follow a link at the last component: `EEXIST`, and
    // the open below refuses it.
    match sys::mkdir(path, 0o755) {
        Err(e) if e != sys::EEXIST => return Err(e),
        _ => {}
    }
    sys::open(path, flags, 0)
}

impl DevFs {
    /// The tmpfs of `/dev` for the tree `root`.
    pub(crate) fn of(root: &RootDir, names: &Names) -> DevFs {
        let lit = |s: &str| CBuf::new(s).expect("a literal");
        let dev = CBuf::new(&format!("{}/dev", root.path.trim_end_matches('/'))).unwrap_or_else(sys::cempty);
        DevFs {
            dev,
            devices: crate::ns_dev_bind::devices(root, names, true),
            links: LINKS.iter().map(|(t, n)| (lit(t), lit(n))).collect(),
            pts: lit("pts"),
            shm: lit("shm"),
            tmpfs: lit("tmpfs"),
            mode: lit("mode=755,size=65536k"),
            shm_mode: lit("mode=1777,size=65536k"),
        }
    }

    /// In the child, right after the bind of the root: the tmpfs and what
    /// it holds. `Err` gives the step, the errno, and the part (1 the mount
    /// point, 2 the tmpfs, 3 `pts`, 4 `shm`, 5 a link). Async-signal-safe.
    pub(crate) fn mount(&self) -> std::result::Result<(), (u8, Errno, u8)> {
        let at = |n: u8| move |e| (NS_DEV_KEEP_STEP, e, n);
        let fd = point_dir(&self.dev).map_err(at(1))?;
        let mounted = mount_data_on_fd(&self.tmpfs, fd, &self.tmpfs, sys::MS_NOSUID | sys::MS_NOEXEC, self.mode.ptr());
        let _ = sys::close(fd);
        mounted.map_err(at(2))?;
        // A new open crosses into the tmpfs.
        let top = point_dir(&self.dev).map_err(at(2))?;
        let out = self.fill(top);
        let _ = sys::close(top);
        out
    }

    fn fill(&self, top: i64) -> std::result::Result<(), (u8, Errno, u8)> {
        let at = |n: u8| move |e| (NS_DEV_KEEP_STEP, e, n);
        sys::mkdirat(top, &self.pts, 0o755).map_err(at(3))?;
        sys::mkdirat(top, &self.shm, 0o755).map_err(at(4))?;
        let flags = sys::O_PATH | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
        let shm = sys::openat(top, &self.shm, flags, 0).map_err(at(4))?;
        let shm_flags = sys::MS_NOSUID | sys::MS_NODEV | sys::MS_NOEXEC;
        let mounted = mount_data_on_fd(&self.tmpfs, shm, &self.tmpfs, shm_flags, self.shm_mode.ptr());
        let _ = sys::close(shm);
        mounted.map_err(at(4))?;
        for (target, name) in &self.links {
            sys::symlinkat(target, top, name).map_err(at(5))?;
        }
        Ok(())
    }

    /// In the child, after the optional mounts, where no user namespace of
    /// the rung binds the devices: each bind of a device. A refused one
    /// writes its line. Async-signal-safe.
    pub(crate) fn bind_devices(&self) {
        crate::ns_dev_bind::bind_each(&self.devices);
    }
}
