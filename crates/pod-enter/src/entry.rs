use super::*;

/// Enter `root` and exec the plan, returning as soon as the exec has succeeded.
///
/// ⛔ **The readiness signal is an `O_CLOEXEC` pipe, and it is the mechanism
/// T-0602 asks for.** The child holds the write end; a successful `execve`
/// closes it and the parent's read returns EOF, and any failure before the exec
/// is written to it as an errno. There is no interval anywhere in it, so there
/// is no scheduling assumption to be wrong about.
///
/// ⭐ **THE CHROOT-BY-PATH ENTRY.** The payload sees `PODVM_ACTIVE_MODE`
/// naming [`ENTERED_RUNG`], whatever the ladder chose: this is the rung the
/// sequence implements, and [`spawn_ladder`] is the one that reports a rung.
pub fn spawn(root: &RootDir, plan: &Plan, err: &mut dyn Write) -> Result<Child> {
    spawn_with(root, plan, ENTERED_RUNG.word(), None, None, true, None, err)
}

/// Enter `root` on a ladder rung and exec the plan, returning on the exec.
///
/// ⭐ T-1003's drive beside [`spawn`]: the same sequence, except the payload
/// sees `PODVM_ACTIVE_MODE` naming the ladder rung and the child execs the
/// memfd where one was handed in (`Some(fd)`), or the path candidates where
/// the payload routes past fd-exec (`None`: a `#!` script, which fails
/// through it).
///
/// ⛔ Only rung-complete rungs drive through here: memfd, the run
/// directory, the tmpfs mount and the FUSE mount the CLI stages before
/// calling, and the persistent cache. Reaching this call with anything
/// else is the caller skipping the choice, so it refuses rather than
/// exec'ing down a rung nobody drove.
pub fn spawn_ladder(
    root: &RootDir,
    plan: &Plan,
    mode: ladder::Mode,
    fd: Option<i64>,
    err: &mut dyn Write,
) -> Result<Child> {
    use ladder::Mode as M;
    match mode {
        M::Memfd | M::RunDir | M::Cache | M::Tmpfs | M::Fuse => {}
    }
    spawn_with(root, plan, mode.name(), None, fd, true, None, err)
}

/// Enter `root` without `chroot(2)` and exec, returning on the exec.
///
/// TODO/enter.md T-1317. The payload runs with the host's root and its
/// working directory inside the image: `exec_argv` is the exact argv the
/// child execs (the image's loader, the payload by host path, then the
/// payload's own arguments), or `plan.argv` where `fd_exec` carries a
/// staged memfd. `active` is the word the payload reads as
/// `PODVM_ACTIVE_MODE`. The readiness pipe, the exit codes and the
/// forwarding wait are the chroot entry's: one fork shape, not two.
pub fn spawn_userland(
    root: &RootDir,
    plan: &Plan,
    exec_argv: Vec<String>,
    active: &str,
    fd_exec: Option<i64>,
    err: &mut dyn Write,
) -> Result<Child> {
    if fd_exec.is_none() && exec_argv.is_empty() {
        return Err(Error::Runtime(
            "the userland entry was handed no argv and no memfd: there is nothing to exec".into(),
        ));
    }
    let override_argv: Vec<CBuf> = exec_argv
        .iter()
        .map(|a| {
            CBuf::new(a)
                .ok_or_else(|| Error::Runtime(format!("the argument {a:?} contains a NUL byte")))
        })
        .collect::<Result<_>>()?;
    spawn_with(
        root,
        plan,
        active,
        Some(override_argv),
        fd_exec,
        false,
        None,
        err,
    )
}

/// Run a payload inside `root` without `chroot(2)`, and return its exit status.
///
/// ⛔ Like [`run`]: the exit status is the payload's, not podvm's.
pub fn run_userland(
    root: &RootDir,
    plan: &Plan,
    exec_argv: Vec<String>,
    active: &str,
    fd_exec: Option<i64>,
    err: &mut dyn Write,
) -> Result<i32> {
    let child = spawn_userland(root, plan, exec_argv, active, fd_exec, err)?;
    // The parent's copy of the memfd serves nothing past the fork: the
    // child execs from its own. It closes here, on the only path that
    // holds one, so no caller leaks a descriptor.
    if let Some(f) = fd_exec {
        let _ = sys::close(f);
    }
    child.wait_forwarding(err)
}

/// Run a payload inside `root` on a ladder rung, and return its exit status.
///
/// ⛔ Like [`run`]: the exit status is the payload's, not podvm's.
pub fn run_ladder(
    root: &RootDir,
    plan: &Plan,
    mode: ladder::Mode,
    fd: Option<i64>,
    err: &mut dyn Write,
) -> Result<i32> {
    let r = spawn_ladder(root, plan, mode, fd, err);
    // The parent's copy of the memfd serves nothing past the fork: the child
    // execs from its own. It closes on every path, including the refusal
    // ones, so a refused launch leaks no descriptor.
    if let Some(f) = fd {
        let _ = sys::close(f);
    }
    let child = r?;
    child.wait_forwarding(err)
}

/// What the namespace rung mounts, built before the fork.
///
/// One private tmpfs on the image's `/tmp`: the mount a run isolates
/// from the host (TODO/enter.md T-1339's Prove). No user, pid or
/// network namespace and no `/proc`: out of scope beside this rung,
/// and the banner says so.
pub struct NsMount {
    pub(super) tmp_target: CBuf,
}

/// The image's `/tmp` as a mount target, or `None` where it cannot be
/// one: missing, a file, or a symlink. Mounting over a link would land
/// on its target, which is outside the image, so a link refuses like a
/// missing directory. `None` enters chroot with the fallback named,
/// never a namespace rung without its mount: the banner's mount
/// topology is fixed text, and a rung without the mount would print it
/// falsely.
pub fn ns_tmp_target(root: &str) -> Option<String> {
    let target = format!("{}/tmp", root.trim_end_matches('/'));
    let not_link = std::fs::symlink_metadata(&target)
        .map(|m| !m.is_symlink())
        .unwrap_or(false);
    let is_dir = std::fs::metadata(&target)
        .map(|m| m.is_dir())
        .unwrap_or(false);
    (not_link && is_dir).then_some(target)
}

/// The fallback line: the cause, then the rung actually entered.
/// Pure, so the drive and the unit test read the same words.
pub fn ns_fallback_line(cause: &str) -> String {
    format!("podvm: {cause}; entered chroot instead (TODO/enter.md T-1339)")
}

/// Whether the chroot landed on the opened descriptor: same device and
/// same file. Pure, so the child and the unit test read the same words.
///
/// TODO/enter.md T-1339: the (d,i) guard past the namespace rung's path
/// chroot. A path checked and then passed can be swapped in between
/// (T-0504's class); a mismatch here refuses before the exec rather
/// than entering wrong.
pub fn chroot_landed(anchored: &sys::Stat, landed: &sys::Stat) -> bool {
    anchored.st_dev == landed.st_dev && anchored.st_ino == landed.st_ino
}

/// Whether the working directory resolves to a directory inside the rootfs.
///
/// TODO/enter.md T-1412. Empty and `/` are the root itself and always
/// exist. Anything else must resolve through [`crate::abi::resolve_dir_in`]:
/// absolute links stay inside the root, `..` past the root escapes, and a
/// loop refuses. A file where a directory belongs answers `Absent` like a
/// missing one: entering it would fail the chdir past the point of no
/// return, so both refuse before the fork.
pub(super) fn workdir_kind(
    rootfs: &str,
    workdir: &str,
) -> std::result::Result<(), crate::abi::ResolveKind> {
    if workdir.contains('\0') {
        // A NUL byte never reaches the kernel: `CBuf::new` refuses it below
        // with its own sentence, so this answers absence and lets that fire.
        return Err(crate::abi::ResolveKind::Absent);
    }
    let guest = if workdir.is_empty() { "/" } else { workdir };
    crate::abi::resolve_dir_in(std::path::Path::new(rootfs), guest).map(|_| ())
}

/// The refusal for a working directory that does not resolve.
///
/// `context` names what was asked for beside the path: the image where the
/// caller knows it (`for image "alpine"`), the rootfs where only it is
/// known. One sentence builder, so the pre-fork check in [`spawn_with`]
/// and the CLI-side [`check_workdir`] cannot disagree about what docker
/// does and what podvm does instead.
pub(super) fn refuse_workdir(workdir: &str, context: &str, kind: crate::abi::ResolveKind) -> Error {
    use crate::abi::ResolveKind as K;
    let why = match kind {
        K::Absent => "is not a directory in the image",
        K::Escapes => "escapes the image",
        K::Loop => "has too many levels of symlinks",
    };
    Error::Runtime(format!(
        "the working directory {workdir:?} {context} {why}: docker creates it, \
         and podvm refuses it instead, naming it (TODO/enter.md T-1412)"
    ))
}

/// Check the working directory before the fork, on both rungs.
///
/// TODO/enter.md T-1412. Resolves `workdir` under `rootfs` and refuses
/// naming the path and the image where it is missing, escapes, loops, or
/// is a file: docker creates the directory, podvm refuses and says so.
/// Empty and `/` always pass; `-w /tmp` passes where the image holds it.
///
/// SEAM for the CLI agent (run/exec/lifecycle wiring, `--workdir`/`-w`
/// flag-parse validation): call
/// `podvm_enter::check_workdir(&rootfs, &workdir, &image)` during prepare,
/// before any fixup mutates the rootfs, and refuse at 125 with the returned
/// sentence. The entry itself re-checks without the image (naming the
/// rootfs) and the child reports a raced chdir as step 11 rather than
/// running from the wrong directory.
pub fn check_workdir(rootfs: &str, workdir: &str, image: &str) -> Result<()> {
    if workdir.contains('\0') {
        return Err(Error::Runtime(
            "the working directory contains a NUL byte".into(),
        ));
    }
    match workdir_kind(rootfs, workdir) {
        Ok(()) => Ok(()),
        Err(kind) => Err(refuse_workdir(
            workdir,
            &format!("for image {image:?}"),
            kind,
        )),
    }
}

/// The namespace setup step a readiness-pipe byte names, or `None`
/// where the byte is not a setup step. Pure, so the parent and the
/// unit test read the same words: a step the parent cannot name is a
/// fallback the banner cannot explain (TODO/enter.md T-1339).
pub fn ns_setup_step(step: u8) -> Option<&'static str> {
    match step {
        7 => Some("unshare(CLONE_NEWNS)"),
        8 => Some("remounting / recursively private"),
        9 => Some("mounting tmpfs on /tmp"),
        10 => Some("verifying the chroot landed on the opened rootfs"),
        _ => None,
    }
}

/// Enter `root` in a mount namespace and exec, falling back to chroot.
///
/// The child unshares a mount namespace, remounts `/` recursively
/// private so its mounts cannot propagate to the host, mounts its
/// private tmpfs on the image's `/tmp`, and then runs the chroot
/// sequence inside. Any setup failure abandons the namespace and
/// enters chroot instead with the fallback named: a host that loses
/// a leg between probe and entry still runs what the chroot rung
/// could carry (TODO/enter.md T-1339's Decision).
pub fn spawn_namespace(root: &RootDir, plan: &Plan, err: &mut dyn Write) -> Result<Child> {
    let target = match ns_tmp_target(&root.path) {
        Some(t) => t,
        None => {
            let _ = writeln!(
                err,
                "{}",
                ns_fallback_line(&format!(
                    "the image holds no /tmp mount point under {}",
                    root.path
                ))
            );
            return spawn_with(root, plan, ENTERED_RUNG.word(), None, None, true, None, err);
        }
    };
    let target = match CBuf::new(&target) {
        Some(t) => t,
        None => {
            let _ = writeln!(
                err,
                "{}",
                ns_fallback_line("the /tmp mount point contains a NUL byte")
            );
            return spawn_with(root, plan, ENTERED_RUNG.word(), None, None, true, None, err);
        }
    };
    let mount = NsMount { tmp_target: target };
    match spawn_with(
        root,
        plan,
        podvm_probe::select::Rung::Namespace.word(),
        None,
        None,
        true,
        Some(&mount),
        err,
    ) {
        Err(Error::NsSetup { step, errno }) => {
            let _ = writeln!(
                err,
                "{}",
                ns_fallback_line(&format!(
                    "the namespace rung refused at {step} ({} ({}))",
                    errno.name(),
                    errno.0
                ))
            );
            spawn_with(root, plan, ENTERED_RUNG.word(), None, None, true, None, err)
        }
        other => other,
    }
}

/// Run a payload inside `root` in a mount namespace, and return its exit status.
///
/// ⛔ Like [`run`]: the exit status is the payload's, not podvm's.
pub fn run_namespace(root: &RootDir, plan: &Plan, err: &mut dyn Write) -> Result<i32> {
    let child = spawn_namespace(root, plan, err)?;
    child.wait_forwarding(err)
}

/// Enter `root` on the rung the probe selected: the namespace rung
/// where selected, the chroot sequence everywhere else. One gate for
/// every entry path (foreground `run`, detached `start`): a rung
/// decided in one place and entered in another is the hole
/// `docs/conventions/code.md` refuses.
///
/// ⚠ Userland has its own entries and never passes through here: it is
/// the no-chroot family, not a rung of this sequence.
pub fn spawn_selected(
    root: &RootDir,
    plan: &Plan,
    rung: podvm_probe::select::Rung,
    err: &mut dyn Write,
) -> Result<Child> {
    use podvm_probe::select::Rung as R;
    match rung {
        R::Namespace => spawn_namespace(root, plan, err),
        _ => spawn(root, plan, err),
    }
}

/// Run a payload inside `root` on the rung the probe selected.
///
/// ⛔ Like [`run`]: the exit status is the payload's, not podvm's.
pub fn run_selected(
    root: &RootDir,
    plan: &Plan,
    rung: podvm_probe::select::Rung,
    err: &mut dyn Write,
) -> Result<i32> {
    let child = spawn_selected(root, plan, rung, err)?;
    child.wait_forwarding(err)
}
