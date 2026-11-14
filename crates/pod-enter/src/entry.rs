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
use crate::ns_overlay::{
    NS_OVERLAY_DIRS_STEP, NS_OVERLAY_STEP, NS_OVERLAY_TMPFS_STEP, OverlayMount, Root, spawn_namespace_root,
};
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
    spawn_with(root, plan, ENTERED_RUNG.word(), None, None, None, None, err)
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
    spawn_with(root, plan, mode.name(), None, fd, None, None, err)
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
    spawn_with(root, plan, active, Some(override_argv), fd_exec, Some(maps), None, err)
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
/// (`ns_optional`): a fresh proc and a bind of the host's `/dev/pts`. No
/// user, PID or network namespace: the banner says so.
pub struct NsMount {
    pub(crate) tmp_target: CBuf,
    pub(crate) optional: Vec<Optional>,
    /// The overlay root, first after the private remount, where the root
    /// is an overlay ([`crate::Root::Overlay`]).
    pub(crate) overlay: Option<OverlayMount>,
}

/// The mounts of the namespace rung: its own, or the mount namespace of
/// a running process, by its pidfd (and true where its root is an
/// overlay).
#[derive(Clone, Copy)]
pub(crate) enum NsEntry<'a> {
    Own(&'a NsMount),
    Join(i64, bool),
}

impl NsEntry<'_> {
    /// Is the root an overlay: the landing check then asks for one.
    pub(crate) fn overlay(&self) -> bool {
        match self {
            NsEntry::Own(m) => m.overlay.is_some(),
            NsEntry::Join(_, overlay) => *overlay,
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

/// The refusal of a working directory that does not resolve.
///
/// `context` names what the caller asked for beside the path: the image
/// where the caller knows it (`for image "alpine"`), the root file system
/// where only it is known. One builder, so the check before the fork and
/// [`check_workdir`] say the same.
///
/// `volume` is the volume (`HOST:GUEST`) that holds the path, where the
/// caller knows it: a missing directory then names the volume, not the
/// image.
pub(crate) fn refuse_workdir(workdir: &str, context: &str, kind: ResolveKind, volume: Option<&str>) -> Error {
    let (context, why) = match (kind, volume) {
        (ResolveKind::Absent, Some(v)) => (format!("in the volume {v}"), "is not a directory in the volume"),
        (ResolveKind::Absent, None) => (context.to_string(), "is not a directory in the image"),
        (ResolveKind::Escapes, _) => (context.to_string(), "escapes the image"),
        (ResolveKind::Loop, _) => (context.to_string(), "has too many levels of symlinks"),
    };
    Error::runtime(
        Class::NotFound,
        format!(
            "the working directory {workdir:?} {context} {why}: docker creates it, and the entry refuses it instead"
        ),
    )
}

/// Check the working directory before the fork.
///
/// Resolve `workdir` under `root`: the root file system, or the table of a
/// run with no chroot ([`crate::userland::Maps`]), where a working
/// directory in a volume is the directory of the volume. Refuse, with the
/// path and the image (or, for a directory inside a volume, the volume as
/// `HOST:GUEST`), a directory that is missing, escapes, loops or is a file: docker
/// creates the directory, and this entry refuses and says so. Empty and
/// `/` always pass; `/tmp` passes where the image holds it.
///
/// Call it before a change of the root file system. The entry checks again
/// with no image name, and the child reports a directory that went
/// missing after the check as its own step.
pub fn check_workdir<G: GuestRoot + ?Sized>(root: &G, workdir: &str, image: &str) -> Result<()> {
    if workdir.contains('\0') {
        return Err(Error::runtime(Class::Usage, "the working directory contains a NUL byte"));
    }
    workdir_kind(root, workdir).map_err(|kind| {
        refuse_workdir(workdir, &format!("for image {image:?}"), kind, root.volume_label(workdir).as_deref())
    })
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
        NS_JOIN_STEP => Some("setns(pidfd, CLONE_NEWNS)"),
        NS_OVERLAY_TMPFS_STEP => Some("mounting the tmpfs of the overlay layers"),
        NS_OVERLAY_DIRS_STEP => Some("making the upper and work directories"),
        NS_OVERLAY_STEP => Some("mounting overlay on the root"),
        _ => None,
    }
}

/// Enter `root` in a mount namespace and exec, or fall back to chroot.
///
/// The child makes a mount namespace, mounts `/` again as recursively
/// private so that its mounts cannot reach the host, mounts its private
/// tmpfs on the `/tmp` of the image, and then does the chroot sequence. A
/// failure of the setup stops the namespace rung, and the entry enters
/// chroot and names the fallback: a host that lost a leg between the probe
/// and the entry still runs what the chroot rung can carry.
pub fn spawn_namespace(root: &RootDir, plan: &Plan, err: &mut dyn Write) -> Result<Child> {
    let names = &plan.names;
    let chroot = |err: &mut dyn Write, cause: &str| {
        let _ = writeln!(err, "{}", ns_fallback_line(names, cause));
        spawn_with(root, plan, ENTERED_RUNG.word(), None, None, None, None, err)
    };
    let Some(target) = ns_tmp_target(&root.path) else {
        return chroot(err, &format!("the image holds no /tmp mount point under {}", root.path));
    };
    let Some(target) = CBuf::new(&target) else {
        return chroot(err, "the /tmp mount point contains a NUL byte");
    };
    let mount = NsMount { tmp_target: target, optional: optional_mounts(&root.path, names), overlay: None };
    match spawn_with(root, plan, Rung::Namespace.word(), None, None, None, Some(NsEntry::Own(&mount)), err) {
        Err(Error::NsSetup { step, errno }) => {
            chroot(err, &format!("the namespace rung refused at {step} ({} ({}))", errno.name(), errno.0))
        }
        other => other,
    }
}

/// Enter `root` in the mount namespace of the running process of `pidfd`
/// and exec: the payload of an exec sees the `/tmp` and the `/proc` of
/// the payload of the run. The entry mounts nothing of its own.
///
/// A refused join (a kernel before 5.8, `EPERM`, a process that ended)
/// names the step and enters a mount namespace of its own with
/// [`spawn_namespace`], which can fall back to chroot in turn.
pub fn spawn_joined(root: &RootDir, plan: &Plan, pidfd: i64, err: &mut dyn Write) -> Result<Child> {
    spawn_joined_root(Root::Tree(root), plan, pidfd, err)
}

/// [`spawn_joined`] on a [`Root`]. For an overlay root, the chroot must
/// land on an overlay that is not the lower directory, and a refused join
/// enters an overlay root of its own ([`spawn_namespace_root`]), which
/// never falls back to chroot.
pub fn spawn_joined_root(root: Root<'_>, plan: &Plan, pidfd: i64, err: &mut dyn Write) -> Result<Child> {
    let join = NsEntry::Join(pidfd, matches!(root, Root::Overlay { .. }));
    match spawn_with(root.dir(), plan, Rung::Namespace.word(), None, None, None, Some(join), err) {
        Err(Error::NsSetup { step, errno }) => {
            let cause = format!("{step} ({} ({}))", errno.name(), errno.0);
            let _ = writeln!(err, "{}", ns_join_fallback_line(&plan.names, &cause));
            spawn_namespace_root(root, plan, err)
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
