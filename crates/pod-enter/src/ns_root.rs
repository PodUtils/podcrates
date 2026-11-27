//! The root of the namespace rung (PodUtils/podvm#695): a mount of its own,
//! the change of root with `pivot_root(2)`, and the read-only root of
//! [`Plan::read_only_root`].
//!
//! Right after `/` is private, the child binds the root path onto itself
//! (an overlay root is a mount already), and each mount of the rung goes
//! on top of it. After the mounts it enters it with `pivot_root(".", ".")`
//! and detaches the old root: the mount namespace of the payload then holds
//! no path of the host, so a way out of a `chroot(2)` of the payload ends at
//! the root of the image. A `chroot(2)` of the tree would keep the host's
//! `/` under it. A refused bind or pivot is a setup step of the rung.
//!
//! With [`Plan::read_only_root`], `/dev` of the tree gets a bind onto
//! itself first, so it stays writable (docker's `/dev` is a tmpfs), and
//! after the mounts the root mount is remounted read-only with each flag
//! that a user namespace locks ([`crate::volume_remount_flags`]). The
//! mounts on top keep their own flags. A refusal stops the payload: it
//! never runs on a writable root.

use pod_core::Class;
use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::{Error, Plan, Result, RootDir};

/// The bind of the root path onto itself.
pub const NS_ROOT_BIND_STEP: u8 = 53;
/// `chdir` to the root path and `pivot_root(".", ".")`.
pub const NS_PIVOT_STEP: u8 = 54;
/// The detach of the old root.
pub const NS_OLD_ROOT_STEP: u8 = 55;
/// The read-only remount of the root, and its check.
pub const NS_READ_ONLY_STEP: u8 = 56;
/// The bind of `/dev` of the tree onto itself, for a read-only root.
pub const NS_DEV_KEEP_STEP: u8 = 57;

/// The root mount of one fork, built before it.
pub(crate) struct RootMount {
    path: CBuf,
    /// `/dev` of the tree, where the root is read-only and the tree holds
    /// a directory there.
    dev: Option<CBuf>,
    read_only: bool,
    empty: CBuf,
    dot: CBuf,
}

/// The name of a setup step of this module, for [`crate::ns_setup_step`].
pub(crate) fn step_name(byte: u8) -> Option<&'static str> {
    match byte {
        NS_ROOT_BIND_STEP => Some("binding the root onto itself"),
        NS_PIVOT_STEP => Some("pivot_root into the root"),
        NS_OLD_ROOT_STEP => Some("detaching the old root"),
        _ => None,
    }
}

/// Is the root of this process the root of the entry `n`: the same device
/// and file as the descriptor `root_fd`, or an overlay over it? `slash`
/// is `/`, `empty` the empty path. Async-signal-safe.
pub(crate) fn landed(n: &crate::entry::NsEntry<'_>, root_fd: i64, empty: &CBuf, slash: &CBuf) -> bool {
    let anchored = sys::fstatat(root_fd, empty, sys::AT_EMPTY_PATH).ok();
    match anchored {
        Some(a) if n.overlay() => crate::ns_overlay::landed(&a, slash),
        Some(a) => sys::fstatat(sys::AT_FDCWD as i64, slash, 0).is_ok_and(|b| crate::chroot_landed(&a, &b)),
        None => false,
    }
}

impl RootMount {
    /// The root mount of `root` for `plan`.
    pub(crate) fn of(plan: &Plan, root: &RootDir) -> Result<RootMount> {
        let path =
            CBuf::new(&root.path).ok_or_else(|| Error::runtime(Class::Usage, "the root path contains a NUL byte"))?;
        let dev = plan.read_only_root.then(|| crate::entry::ns_dir_target(&root.path, "dev")).flatten();
        let dev = dev.and_then(|d| CBuf::new(&d));
        let lit = |s: &str| CBuf::new(s).expect("a literal");
        Ok(RootMount { path, dev, read_only: plan.read_only_root, empty: sys::cempty(), dot: lit(".") })
    }

    /// In the child, right after `/` is private (and the overlay, where
    /// `overlay`): the bind of the root onto itself, then the bind of
    /// `/dev` for a read-only root. Async-signal-safe.
    pub(crate) fn bind(&self, overlay: bool) -> std::result::Result<(), (u8, Errno)> {
        if !overlay {
            sys::mount(&self.path, &self.path, &self.empty, sys::MS_BIND | sys::MS_REC)
                .map_err(|e| (NS_ROOT_BIND_STEP, e))?;
        }
        if let Some(dev) = &self.dev {
            sys::mount(dev, dev, &self.empty, sys::MS_BIND | sys::MS_REC).map_err(|e| (NS_DEV_KEEP_STEP, e))?;
        }
        Ok(())
    }

    /// In the child, after each mount of the rung: the read-only remount
    /// of the root where the plan asks for it, and its check. `Err` gives
    /// the step, the errno, and 1 where the remount passed and the root is
    /// still writable. Async-signal-safe.
    pub(crate) fn seal(&self) -> std::result::Result<(), (u8, Errno, u8)> {
        if !self.read_only {
            return Ok(());
        }
        let refused = |e| (NS_READ_ONLY_STEP, e, 0);
        let st = sys::statfs(&self.path).map_err(refused)?;
        sys::mount(&self.empty, &self.path, &self.empty, crate::ns_volume::remount_flags(st.f_flags))
            .map_err(refused)?;
        // Fail safe: a root that does not read as read-only stops the run.
        match sys::statfs(&self.path).map(|s| s.flag(sys::ST_RDONLY)) {
            Ok(Some(true)) => Ok(()),
            Ok(_) => Err((NS_READ_ONLY_STEP, sys::EROFS, 1)),
            Err(e) => Err(refused(e)),
        }
    }

    /// In the child, after the mounts: `pivot_root` into the root and the
    /// detach of the old one. The working directory and the root are then
    /// the root of the tree. Async-signal-safe.
    pub(crate) fn pivot(&self) -> std::result::Result<(), (u8, Errno)> {
        sys::chdir(&self.path).map_err(|e| (NS_PIVOT_STEP, e))?;
        // `pivot_root(".", ".")` stacks the old root on the new one; the
        // detach of "." then removes it, with each mount under it.
        unsafe { sys::sys(sys::SYS_PIVOT_ROOT, [self.dot.ptr(), self.dot.ptr(), 0, 0, 0, 0]) }
            .map_err(|e| (NS_PIVOT_STEP, e))?;
        sys::umount_detach(&self.dot).map(drop).map_err(|e| (NS_OLD_ROOT_STEP, e))
    }
}

/// The refusal of the read-only root, or `None` where `byte` is another
/// step. These steps never fall back: the payload does not run on a
/// writable root.
pub(crate) fn failure(byte: u8, errno: Errno, at: u8) -> Option<Error> {
    let step = match byte {
        NS_READ_ONLY_STEP => "remounting the root read-only",
        NS_DEV_KEEP_STEP => "binding /dev of the root onto itself",
        _ => return None,
    };
    let text = if at == 1 && byte == NS_READ_ONLY_STEP {
        "the root does not read as read-only after its read-only remount".to_string()
    } else {
        format!("{step} failed with {} ({})", errno.name(), errno.0)
    };
    Some(Error::runtime(
        crate::errno_class(errno),
        format!("the read-only root of the plan: {text}; the payload did not run"),
    ))
}

#[cfg(test)]
mod tests;
