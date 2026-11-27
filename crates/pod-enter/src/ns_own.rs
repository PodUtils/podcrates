//! The steps of the child on the namespace rung with a mount namespace of
//! its own ([`crate::entry::NsEntry::Own`]), from the user namespace of the
//! rung to the change of root (`ns_root`). The order: the user namespace
//! where the rung needs one (`ns_user`), the mount namespace with `/`
//! private, the root mount (the overlay, or a bind of the root onto
//! itself), the tmpfs on `/tmp`, the optional mounts, the tmpfs mounts and
//! the volumes of the plan, the host name, the read-only remount of the
//! root, and the pivot. The nested user namespace of the payload comes
//! after it (`ns_lock`).

use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::entry::NsMount;
use crate::ns_root::RootMount;
use crate::ns_volume::Volumes;

/// What the child needs for the steps, built before the fork.
pub(crate) struct OwnSteps<'a> {
    pub(crate) m: &'a NsMount,
    pub(crate) root: RootMount,
    pub(crate) volumes: &'a Volumes,
    pub(crate) hostname: Option<&'a CBuf>,
    pub(crate) hostname_len: usize,
    /// The clone of the child made the user namespace (`ns_pid`).
    pub(crate) cloned_user: bool,
    pub(crate) root_fd: i64,
    pub(crate) rootpath: &'a CBuf,
    pub(crate) empty: CBuf,
    pub(crate) slash: CBuf,
    pub(crate) tmpfs: CBuf,
}

impl<'a> OwnSteps<'a> {
    /// The steps of `m` for the root `root`.
    pub(crate) fn new(m: &'a NsMount, root: RootMount, volumes: &'a Volumes, rootpath: &'a CBuf) -> OwnSteps<'a> {
        let lit = |s: &str| CBuf::new(s).expect("a literal");
        OwnSteps {
            m,
            root,
            volumes,
            hostname: None,
            hostname_len: 0,
            cloned_user: false,
            root_fd: -1,
            rootpath,
            empty: sys::cempty(),
            slash: lit("/"),
            tmpfs: lit("tmpfs"),
        }
    }

    /// In the child: each step to the pivot. `Err` gives the step, the
    /// errno and the index of a volume or a tmpfs. Async-signal-safe.
    pub(crate) fn run(&self) -> std::result::Result<(), (u8, Errno, u8)> {
        let m = self.m;
        let one = |(step, e): (u8, Errno)| (step, e, 0);
        // A user namespace first, where the rung needs one (`ns_user`).
        if let Some(u) = &m.user {
            crate::ns_user::enter_with(u, self.cloned_user).map_err(one)?;
        }
        // The lower layer of an overlay is the working directory: the
        // unshare moves it into the new namespace, and a descriptor of the
        // old one does not mount there.
        if m.overlay.is_some() {
            sys::fchdir(self.root_fd).map_err(|e| (crate::ns_overlay::NS_OVERLAY_STEP, e, 0))?;
        }
        sys::unshare(sys::CLONE_NEWNS).map_err(|e| (7, e, 0))?;
        // `/` private and recursive, so that no mount reaches the host.
        sys::mount(&self.empty, &self.slash, &self.empty, sys::MS_REC | sys::MS_PRIVATE).map_err(|e| (8, e, 0))?;
        // The root mount, before each mount under it.
        if let Some(o) = &m.overlay {
            crate::ns_overlay::mount(o, self.rootpath, m.user.is_some()).map_err(one)?;
        }
        self.root.bind(m.overlay.is_some()).map_err(one)?;
        sys::mount(&self.tmpfs, &m.tmp_target, &self.tmpfs, 0).map_err(|e| (9, e, 0))?;
        // `/proc`, `/dev/pts` and the host's devices: a refused one writes
        // one line and the rung goes on.
        crate::ns_optional::mount_each(m);
        // The volumes of the plan, after each mount of the rung: a refused
        // one stops the payload (`ns_volume`).
        crate::ns_volume::bind_each(self.volumes)?;
        if let Some(name) = self.hostname {
            crate::uts::enter(name, self.hostname_len).map_err(one)?;
        }
        self.root.seal()?;
        self.root.pivot().map_err(one)
    }
}
