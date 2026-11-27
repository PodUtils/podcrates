//! The entries: the chroot sequence, the ladder rungs, the entry with no
//! chroot and the namespace rung. Each one calls the same fork
//! ([`crate::launch::spawn_with`]).

use std::io::Write;

use pod_core::Class;
use pod_hostprobe::select::Rung;
use pod_hostprobe::sys::{self, CBuf};

use crate::abi::{GuestRoot, ResolveKind};
use crate::launch::spawn_with;
use crate::ns_optional::{NS_PROC_STEP, NS_PTS_STEP, Optional, optional_mounts};
use crate::ns_overlay::{NS_OVERLAY_DIRS_STEP, NS_OVERLAY_STEP, NS_OVERLAY_TMPFS_STEP, OverlayMount, Root};
use crate::userland::Maps;
use crate::{Child, ENTERED_RUNG, Error, Names, Plan, Result, RootDir, ladder};

/// Enter `root` and exec the plan. Return when the exec succeeded.
///
/// The readiness signal is an `O_CLOEXEC` pipe. The child holds the write
/// end: a successful `execve` closes it and the read of the parent gives
/// the end of the file. A failure before the exec goes into the pipe as an
/// errno. There is no interval, so there is no guess about the scheduler.
///
/// The payload sees the active mode variable with the word of
/// [`ENTERED_RUNG`], the rung that this sequence does. [`spawn_ladder`]
/// reports a rung of the ladder.
pub fn spawn(root: &RootDir, plan: &Plan, err: &mut dyn Write) -> Result<Child> {
    spawn_with(root, plan, ENTERED_RUNG.word(), None, None, None, None, true, err)
}

/// Enter `root` on a rung of the ladder and exec the plan. Return when the
/// exec succeeded.
///
/// The same sequence as [`spawn`], but the payload sees the word of the
/// ladder rung, and the child execs the memfd where the caller gives one
/// (`Some(fd)`), or the candidate paths where the payload does not take
/// the exec of a descriptor (`None`: a `#!` script fails through it).
///
/// The caller stages the memfd, the run directory, the tmpfs mount, the
/// FUSE mount or the cache before this call.
pub fn spawn_ladder(
    root: &RootDir,
    plan: &Plan,
    mode: ladder::Mode,
    fd: Option<i64>,
    err: &mut dyn Write,
) -> Result<Child> {
    spawn_with(root, plan, mode.name(), None, fd, None, None, true, err)
}

/// Enter `root` with no `chroot(2)` and exec. Return when the exec
/// succeeded.
///
/// The payload runs with the root of the host, and with its working
/// directory inside the image or a volume. `maps` is the table of the run
/// ([`crate::userland::Maps`], its root is `root.path`): the working
/// directory resolves through it before the fork. `exec_argv` is the
/// exact argv that the child execs (the loader of the image, the payload
/// by host path, then the arguments of the payload), or `plan.argv` where
/// `fd_exec` gives a staged memfd. `active` is the word of the active mode variable. The
/// readiness pipe, the exit codes and the forwarding wait are the ones of
/// the chroot entry: one fork, not two.
pub fn spawn_userland(
    root: &RootDir,
    plan: &Plan,
    maps: &Maps,
    exec_argv: Vec<String>,
    active: &str,
    fd_exec: Option<i64>,
    err: &mut dyn Write,
) -> Result<Child> {
    if fd_exec.is_none() && exec_argv.is_empty() {
        return Err(Error::runtime(
            Class::Usage,
            "the userland entry got no argv and no memfd: there is nothing to exec",
        ));
    }
    let override_argv: Vec<CBuf> = exec_argv
        .iter()
        .map(|a| {
            CBuf::new(a).ok_or_else(|| Error::runtime(Class::Usage, format!("the argument {a:?} contains a NUL byte")))
        })
        .collect::<Result<_>>()?;
    spawn_with(root, plan, active, Some(override_argv), fd_exec, Some(maps), None, true, err)
}

/// Run a payload inside `root` with no `chroot(2)`, and return its exit
/// status. As [`crate::run`], the exit status is the status of the
/// payload.
pub fn run_userland(
    root: &RootDir,
    plan: &Plan,
    maps: &Maps,
    exec_argv: Vec<String>,
    active: &str,
    fd_exec: Option<i64>,
    err: &mut dyn Write,
) -> Result<i32> {
    let child = spawn_userland(root, plan, maps, exec_argv, active, fd_exec, err)?;
    // The copy of the memfd in the parent serves nothing after the fork:
    // the child execs from its own copy. It closes here, on the only path
    // that holds one, so no caller leaks a descriptor.
    if let Some(f) = fd_exec {
        let _ = sys::close(f);
    }
    child.wait_forwarding(err)
}

/// Run a payload inside `root` on a rung of the ladder, and return its
/// exit status. As [`crate::run`], the exit status is the status of the
/// payload.
pub fn run_ladder(
    root: &RootDir,
    plan: &Plan,
    mode: ladder::Mode,
    fd: Option<i64>,
    err: &mut dyn Write,
) -> Result<i32> {
    let r = spawn_ladder(root, plan, mode, fd, err);
    // The copy of the memfd in the parent closes on each path, also on a
    // refusal, so a refused launch leaks no descriptor.
    if let Some(f) = fd {
        let _ = sys::close(f);
    }
    let child = r?;
    child.wait_forwarding(err)
}

/// What the namespace rung mounts, built before the fork.
///
/// One private tmpfs on the `/tmp` of the image: the mount that a run
/// keeps apart from the host. Then the mounts that do not stop the rung
/// (`ns_optional`): a fresh proc and a bind of the host's `/dev/pts` (in a
/// user namespace, a new devpts instance: `ns_pts`). A PID
/// namespace only where the plan asks for one (`ns_pid`), and no network
/// namespace: the banner says so. A user namespace only for
/// a caller that cannot make the mount namespace in its own (`ns_user`).
pub struct NsMount {
    /// The maps of the user namespace that the child makes first, where
    /// the rung needs one.
    pub(crate) user: Option<pod_hostprobe::userns::Maps>,
    pub(crate) tmp_target: CBuf,
    pub(crate) optional: Vec<Optional>,
    /// The overlay root, first after the private remount, where the root
    /// is an overlay ([`crate::Root::Overlay`]).
    pub(crate) overlay: Option<OverlayMount>,
    /// The binds of the host's devices, built with the user namespace
    /// (`ns_dev_bind`).
    pub(crate) devices: Option<crate::ns_dev_bind::Devices>,
}

/// The mounts of the namespace rung: its own, or the mount namespace of
/// a running process, by its pidfd (`ns_join`).
#[derive(Clone, Copy)]
pub(crate) enum NsEntry<'a> {
    Own(&'a NsMount),
    /// The join, and the upper directory of the overlay root of the run.
    Join(crate::ns_join::Join, Option<&'a str>),
}

impl<'a> NsEntry<'a> {
    /// Is the root an overlay: the landing check then asks for one.
    pub(crate) fn overlay(&self) -> bool {
        match self {
            NsEntry::Own(m) => m.overlay.is_some(),
            NsEntry::Join(j, _) => j.overlay,
        }
    }

    /// The upper directory of the caller of an overlay root
    /// (`Upper::Dir`), with the attribute of an opaque directory that its
    /// mount reads: the checks before the fork read it over the image.
    pub(crate) fn upper(&self) -> Option<crate::ns_upper::UpperDir<'a>> {
        match self {
            NsEntry::Own(m) => m.overlay.as_ref().and_then(|o| {
                let userxattr = o.userxattr(m.user.is_some());
                o.upper_dir().map(|path| crate::ns_upper::UpperDir { path, userxattr })
            }),
            NsEntry::Join(j, upper) => {
                let userxattr = j.user || crate::ns_overlay::caller_in_user_namespace();
                upper.map(|path| crate::ns_upper::UpperDir { path, userxattr })
            }
        }
    }
}

/// The step byte of the join of a mount namespace.
pub const NS_JOIN_STEP: u8 = 16;

/// The `/tmp` of the image as a mount target, or `None` where it cannot be
/// one: missing, a file, or a symbolic link. A mount over a link lands on
/// its target, which is outside the image, so a link is refused as a
/// missing directory is. `None` enters chroot and names the fallback; the
/// entry never enters the namespace rung without its mount, because the
/// banner names the mount as a fixed fact.
pub fn ns_tmp_target(root: &str) -> Option<String> {
    ns_dir_target(root, "tmp")
}

pub(crate) fn ns_dir_target(root: &str, name: &str) -> Option<String> {
    let target = format!("{}/{name}", root.trim_end_matches('/'));
    let not_link = std::fs::symlink_metadata(&target).map(|m| !m.is_symlink()).unwrap_or(false);
    let is_dir = std::fs::metadata(&target).map(|m| m.is_dir()).unwrap_or(false);
    (not_link && is_dir).then_some(target)
}

/// `n` in decimal in `buf`, with no allocation: the child of the fork
/// writes it.
pub(crate) fn decimal(mut n: u32, buf: &mut [u8; 10]) -> &[u8] {
    let mut at = buf.len();
    loop {
        at -= 1;
        buf[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            return &buf[at..];
        }
    }
}

/// The fallback line: the cause, then the rung that the entry entered.
pub fn ns_fallback_line(names: &Names, cause: &str) -> String {
    format!("{}: {cause}; entered chroot instead", names.product)
}

/// Did the chroot land on the opened descriptor: the same device and the
/// same file?
///
/// The namespace rung changes its root by path. A path that was checked
/// and then given can change between the two, so a mismatch here refuses
/// before the exec.
pub fn chroot_landed(anchored: &sys::Stat, landed: &sys::Stat) -> bool {
    anchored.st_dev == landed.st_dev && anchored.st_ino == landed.st_ino
}

/// Does the working directory resolve to a directory inside the root file
/// system, or inside a volume of the table?
///
/// Empty and `/` are the root, and always exist. Each other path must
/// resolve through [`crate::abi::resolve_dir_in`]: an absolute link stays
/// inside the table, a `..` past the root escapes, and a loop is refused.
/// A file where a directory must be is `Absent`, as a missing one is: the
/// `chdir` would fail after the point of no return.
pub(crate) fn workdir_kind<G: GuestRoot + ?Sized>(root: &G, workdir: &str) -> std::result::Result<(), ResolveKind> {
    if workdir.contains('\0') {
        // A NUL byte never reaches the kernel: `CBuf::new` refuses it
        // with its own sentence.
        return Err(ResolveKind::Absent);
    }
    let guest = if workdir.is_empty() { "/" } else { workdir };
    crate::abi::resolve_dir_in(root, guest).map(|_| ())
}

/// The setup step of the namespace rung that a byte of the readiness pipe
/// names, or `None` where the byte is not a setup step. The parent and
/// the tests read this one mapping, so each fallback has a name.
pub fn ns_setup_step(step: u8) -> Option<&'static str> {
    match step {
        7 => Some("unshare(CLONE_NEWNS)"),
        8 => Some("remounting / recursively private"),
        9 => Some("mounting tmpfs on /tmp"),
        10 => Some("verifying the chroot landed on the opened rootfs"),
        NS_PROC_STEP => Some("mounting proc on /proc"),
        NS_PTS_STEP => Some("binding /dev/pts on /dev/pts"),
        crate::ns_proc_bind::NS_PROC_BIND_STEP => Some("binding the host's /proc on /proc"),
        crate::ns_proc_bind::NS_PROC_REMOUNT_STEP => Some("remounting the bind of /proc nosuid, nodev and noexec"),
        crate::ns_dev_bind::NS_DEV_POINT_STEP => Some("opening the mount point of a device of the host"),
        crate::ns_dev_bind::NS_DEV_BIND_STEP => Some("binding the host's device"),
        crate::ns_dev_bind::NS_DEV_REMOUNT_STEP => Some("remounting the bind of a device nosuid and noexec"),
        crate::ns_pts::NS_DEVPTS_STEP => Some("mounting a new devpts instance on /dev/pts"),
        crate::ns_pts::NS_PTMX_BIND_STEP => Some("binding the ptmx of the devpts on /dev/pts"),
        NS_JOIN_STEP => Some("setns(pidfd, CLONE_NEWNS)"),
        crate::ns_join::NS_JOIN_USER_STEP => Some("setns(pidfd, CLONE_NEWUSER | CLONE_NEWNS)"),
        NS_OVERLAY_TMPFS_STEP => Some("mounting the tmpfs of the overlay layers"),
        NS_OVERLAY_DIRS_STEP => Some("making the upper and work directories"),
        NS_OVERLAY_STEP => Some("mounting overlay on the root"),
        crate::uts::NS_UTS_STEP => Some("unshare(CLONE_NEWUTS)"),
        crate::uts::NS_HOSTNAME_STEP => Some("sethostname"),
        // The steps of a volume come back as a refusal that names the
        // volume, never as `NsSetup` (`ns_volume::failure`).
        crate::ns_volume::NS_VOLUME_POINT_STEP => Some("opening the mount point of a volume in the image"),
        crate::ns_volume::NS_VOLUME_LINK_STEP => {
            Some("the guest path of a volume crosses a symbolic link of the image")
        }
        crate::ns_volume::NS_VOLUME_BIND_STEP => Some("binding the host path of a volume"),
        crate::ns_volume::NS_VOLUME_REMOUNT_STEP => Some("remounting the bind of a read-only volume read-only"),
        crate::ns_volume::NS_VOLUME_SOURCE_STEP => {
            Some("opening the host path of a volume under its base with no symbolic link")
        }
        // The steps of a tmpfs come back as a refusal that names the tmpfs
        // (`ns_tmpfs::failure`).
        _ => crate::ns_user::step_name(step)
            .or_else(|| crate::ns_pid::step_name(step))
            .or_else(|| crate::ns_tmpfs::step_name(step))
            .or_else(|| crate::ns_root::step_name(step))
            .or_else(|| crate::ns_lock::step_name(step)),
    }
}

/// Enter `root` in a mount namespace and exec, or fall back to chroot.
///
/// The child makes a mount namespace (in a user namespace of its own where
/// the caller has no `CAP_SYS_ADMIN`: `ns_user`), mounts `/` again as recursively
/// private so that its mounts cannot reach the host, mounts its private
/// tmpfs on the `/tmp` of the image, and then does the chroot sequence. A
/// failure of the setup stops the namespace rung, and the entry enters
/// chroot and names the fallback: a host that lost a leg between the probe
/// and the entry still runs what the chroot rung can carry.
pub fn spawn_namespace(root: &RootDir, plan: &Plan, err: &mut dyn Write) -> Result<Child> {
    namespace_entry(root, plan, true, err)
}

/// [`spawn_namespace`]; `banner` is false where an entry before it wrote
/// the banner (a refused join).
pub(crate) fn namespace_entry(root: &RootDir, plan: &Plan, banner: bool, err: &mut dyn Write) -> Result<Child> {
    let names = &plan.names;
    // The banner of the fallback: none where a fork before it wrote one.
    let chroot = |err: &mut dyn Write, cause: &str, banner: bool| {
        // A host name has no place on the chroot rung: refuse, with the cause.
        if let Some(name) = &plan.hostname {
            return Err(Error::runtime(
                Class::Unsupported,
                format!("the host name {name:?} needs the namespace rung, and {cause}"),
            ));
        }
        // Nor a PID namespace: the payload never sees the host's processes.
        if plan.pid_namespace {
            return Err(Error::runtime(
                Class::Unsupported,
                format!("the PID namespace needs the namespace rung, and {cause}"),
            ));
        }
        // A volume or a tmpfs has no place on the chroot rung either.
        if let Some(mount) = crate::ns_volume::first_mount(plan) {
            return Err(Error::runtime(Class::Unsupported, format!("{mount} needs the namespace rung, and {cause}")));
        }
        let _ = writeln!(err, "{}", ns_fallback_line(names, cause));
        spawn_with(root, plan, ENTERED_RUNG.word(), None, None, None, None, banner, err)
    };
    let Some(target) = ns_tmp_target(&root.path) else {
        return chroot(err, &format!("the image holds no /tmp mount point under {}", root.path), banner);
    };
    let Some(target) = CBuf::new(&target) else {
        return chroot(err, "the /tmp mount point contains a NUL byte", banner);
    };
    let optional = optional_mounts(&root.path, names);
    let mut mount = NsMount { user: None, tmp_target: target, optional, overlay: None, devices: None };
    let own = crate::ns_user::spawn_own(root, plan, &mut mount, banner, err);
    match (own.result, own.first) {
        (Err(Error::NsSetup { step, errno }), Some((first, e))) => {
            let cause = crate::both_refused(first, e, step, errno);
            chroot(err, &format!("the namespace rung refused at {cause}"), false)
        }
        (Err(Error::NsSetup { step, errno }), None) => {
            chroot(err, &format!("the namespace rung refused at {step} ({} ({}))", errno.name(), errno.0), false)
        }
        (other, _) => other,
    }
}

/// Enter `root` in the mount namespace of the running process of `pidfd`
/// and exec: the payload of an exec sees the `/tmp` and the `/proc` of
/// the payload of the run. The entry mounts nothing of its own, and binds
/// no volume of the plan: the mount namespace of the run holds its
/// volumes (a fallback below binds them). Where the
/// payload is in a user namespace that is not the one of the caller (a run
/// of a caller with no capabilities), the entry joins it too (`ns_join`).
/// Where `pidfd` is the init of a PID namespace whose namespaces this
/// process cannot open, the entry joins its payload ([`crate::joinable`]).
///
/// A refused join (a kernel before 5.8, `EPERM`, a process that ended)
/// names the step and enters a mount namespace of its own with
/// [`spawn_namespace`], which can fall back to chroot in turn.
pub fn spawn_joined(root: &RootDir, plan: &Plan, pidfd: i64, err: &mut dyn Write) -> Result<Child> {
    spawn_joined_root(Root::Tree(root), plan, pidfd, err)
}

/// [`spawn_joined`] on a [`Root`]. For an overlay root, the chroot must
/// land on an overlay that is not the lower directory, and a refused join
/// enters an overlay root of its own ([`crate::spawn_namespace_root`]), which
/// never falls back to chroot.
pub fn spawn_joined_root(root: Root<'_>, plan: &Plan, pidfd: i64, err: &mut dyn Write) -> Result<Child> {
    // The pidfd of the payload stays open until the child joined.
    let member = crate::ns_join::joinable(pidfd);
    let target = member.as_ref().map_or(pidfd, |m| i64::from(std::os::fd::AsRawFd::as_raw_fd(&m.pidfd)));
    // The owner of the mounts of the run stays open until the child joined.
    let owner = crate::ns_join::mount_owner(target);
    let mut j = crate::ns_join::Join::of(target, matches!(root, Root::Overlay { .. }));
    j.owner = owner.as_ref().map(|o| i64::from(std::os::fd::AsRawFd::as_raw_fd(o)));
    let join = NsEntry::Join(j, root.upper_dir());
    match spawn_with(root.dir(), plan, Rung::Namespace.word(), None, None, None, Some(join), true, err) {
        Err(Error::NsSetup { step, errno }) => {
            let cause = format!("{step} ({} ({}))", errno.name(), errno.0);
            let _ = writeln!(err, "{}", ns_join_fallback_line(&plan.names, &cause));
            crate::ns_overlay::namespace_root_entry(root, plan, false, err)
        }
        other => other,
    }
}

/// The line of a refused join: the cause, then what the entry did.
pub fn ns_join_fallback_line(names: &Names, cause: &str) -> String {
    format!(
        "{}: the entry does not share the mounts of the payload: {cause}; entered a mount namespace of its own",
        names.product
    )
}

/// Run a payload inside `root` in a mount namespace, and return its exit
/// status. As [`crate::run`], the exit status is the status of the
/// payload.
pub fn run_namespace(root: &RootDir, plan: &Plan, err: &mut dyn Write) -> Result<i32> {
    let child = spawn_namespace(root, plan, err)?;
    child.wait_forwarding(err)
}

/// Enter `root` on the rung that the probe selected: the namespace rung
/// where selected, the chroot sequence everywhere else. Each entry path
/// (a run in the foreground, a detached start) uses this one gate, so the
/// rung is decided and entered in one place.
///
/// The entry with no chroot has its own functions and does not pass
/// through here: it is not a rung of this sequence.
pub fn spawn_selected(root: &RootDir, plan: &Plan, rung: Rung, err: &mut dyn Write) -> Result<Child> {
    match rung {
        Rung::Namespace => spawn_namespace(root, plan, err),
        _ => spawn(root, plan, err),
    }
}

/// Run a payload inside `root` on the rung that the probe selected. As
/// [`crate::run`], the exit status is the status of the payload.
pub fn run_selected(root: &RootDir, plan: &Plan, rung: Rung, err: &mut dyn Write) -> Result<i32> {
    let child = spawn_selected(root, plan, rung, err)?;
    child.wait_forwarding(err)
}
