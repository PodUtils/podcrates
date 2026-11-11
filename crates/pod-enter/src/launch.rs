//! The one fork of each entry.

use std::io::Write;

use pod_core::Class;
use pod_hostprobe::sys::{self, CBuf};

use crate::abi::{GuestRoot, resolve_dir};
use crate::entry::{NsMount, chroot_landed, ns_setup_step, refuse_workdir, workdir_kind};
use crate::names::var_name;
use crate::status::{NOT_FOUND, invocable_but_refused};
use crate::userland::Maps;
use crate::{Child, Error, Plan, Result, RootDir, errno_failure};

/// The exit code of a child that wrote a failed step to the readiness
/// pipe. The parent reaps that child and reads the step from the pipe, so
/// no caller sees this code. It is docker's code of a runtime error.
const SETUP_FAILED: i32 = 125;

/// The one entry sequence of [`crate::spawn`], [`crate::spawn_ladder`] and
/// [`crate::spawn_userland`].
///
/// `active` is the word of the active mode variable. `fd_exec` is the
/// written memfd that the child execs, where the caller gives one.
/// `argv_exec` is the exact argv that the child execs in place of the
/// candidates (the loader of the entry with no chroot). `userland` is
/// `None` for the chroot sequence, and the table of the run for the
/// sequence with `fchdir` only. `ns` carries the
/// mount of the namespace rung, or nothing. One function, so the fork, the
/// rules of the root and the readiness pipe are the same for each entry.
///
/// Eight parameters, each one used: they are the fixed set of the fork
/// (each thing that the child touches exists before it).
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_with(
    root: &RootDir,
    plan: &Plan,
    active: &str,
    argv_exec: Option<Vec<CBuf>>,
    fd_exec: Option<i64>,
    userland: Option<&Maps>,
    ns: Option<&NsMount>,
    err: &mut dyn Write,
) -> Result<Child> {
    // Before the fork. Each allocation of the child happens here: between
    // `clone` and `execve` only async-signal-safe work is permitted, so
    // nothing after the fork allocates, formats or takes a lock.
    let nul = |what: String| Error::runtime(Class::Usage, what);
    let argv_owned: Vec<CBuf> = plan
        .argv
        .iter()
        .map(|a| CBuf::new(a).ok_or_else(|| nul(format!("the argument {a:?} contains a NUL byte"))))
        .collect::<Result<_>>()?;
    // The request variable never crosses the exec, whatever built the
    // plan: [`Plan::env_for`] removes it from what it builds, and this
    // removes it from what it gets, so a plan made by hand cannot pass one
    // either. The entered rung goes in under the other name, so a nested
    // run starts with no request.
    let names = &plan.names;
    let active_var = format!("{}={active}", names.mode_active);
    let envp_owned: Vec<CBuf> = plan
        .env
        .iter()
        .filter(|e| {
            let name = var_name(e);
            name != names.mode_request && name != names.mode_active
        })
        .chain(std::iter::once(&active_var))
        .map(|e| CBuf::new(e).ok_or_else(|| nul(format!("the environment {e:?} contains a NUL"))))
        .collect::<Result<_>>()?;
    let chroot = userland.is_none();
    let workdir = CBuf::new(if plan.working_dir.is_empty() { "/" } else { &plan.working_dir })
        .ok_or_else(|| nul("the working directory contains a NUL byte".to_string()))?;
    let slash = CBuf::new("/").expect("a literal");
    let dot = CBuf::new(".").expect("a literal");
    let tmpfs = CBuf::new("tmpfs").expect("a literal");
    // The empty path through which `execveat` execs a descriptor.
    let empty = sys::cempty();
    // The path by which the namespace rung changes its root. The entry
    // descriptor was opened before the fork, in the mount namespace of the
    // caller, and `fchdir` with `chroot(".")` anchors the new root in that
    // mount: the tmpfs that the child mounts in its own namespace stays
    // hidden under it. The path resolves in the new namespace, under the
    // private mount, and the device and inode check after the chroot
    // refuses a path that changed.
    let rootpath = CBuf::new(&root.path).ok_or_else(|| nul("the root path contains a NUL byte".to_string()))?;

    let mut argv: Vec<*const u8> = argv_owned.iter().map(|c| c.ptr() as *const u8).collect();
    argv.push(std::ptr::null());
    let mut envp: Vec<*const u8> = envp_owned.iter().map(|c| c.ptr() as *const u8).collect();
    envp.push(std::ptr::null());
    let argv_exec_ptrs: Option<Vec<*const u8>> = argv_exec.as_ref().map(|ov| {
        let mut v: Vec<*const u8> = ov.iter().map(|c| c.ptr() as *const u8).collect();
        v.push(std::ptr::null());
        v
    });

    // The child resolves the program inside the new root, so the candidate
    // paths are built here and tried there. A path that the parent
    // resolves names the outer tree, which is gone after the chroot.
    let candidates: Vec<CBuf> = plan.program_candidates().into_iter().filter_map(|p| CBuf::new(&p)).collect();
    if candidates.is_empty() {
        return Err(Error::NotFound(format!(
            "{:?}: no candidate path could be built for it",
            plan.argv.first().map(String::as_str).unwrap_or("")
        )));
    }

    // Check the working directory before the fork, on each rung: a missing
    // one is refused here with its name, and the payload never runs from
    // the wrong directory with exit 0. A NUL byte failed at the `CBuf`
    // above. With no chroot the check is the resolution through the table.
    let workdir_host = match userland {
        Some(maps) => userland_workdir(root, maps, &plan.working_dir)?,
        None => {
            if let Err(kind) = workdir_kind(root.path.as_str(), &plan.working_dir) {
                return Err(refuse_workdir(
                    &plan.working_dir,
                    &format!("in the image's rootfs {:?}", root.path),
                    kind,
                    None,
                ));
            }
            None
        }
    };

    // The confinement of the payload, compiled before the banner and the
    // fork: the child opens the paths of its rules and applies it
    // ([`crate::confine`]).
    let mut confined = crate::confine::compile(plan, chroot.then_some(root))?;
    // The child puts each passed descriptor onto its number first: the
    // root descriptor and the memfd must hold no such number.
    for &(child_fd, _) in &plan.fds.pass {
        if child_fd == root.fd() || Some(child_fd) == fd_exec {
            return Err(Error::runtime(
                Class::Usage,
                format!("a passed descriptor goes onto {child_fd}, the number of the root file system or of the memfd"),
            ));
        }
    }

    // The banner before the fork, so it cannot mix with the first output
    // of the payload. It goes to the error writer, never to the standard
    // output.
    let _ = write!(err, "{}", plan.banner);
    let _ = err.flush();

    // The readiness pipe. `O_CLOEXEC` on both ends: the `execve` closes the
    // write end, and the read of the parent then gives the end of the file.
    let mut pipe = [0i32; 2];
    sys::pipe2(&mut pipe, sys::O_CLOEXEC).map_err(|e| errno_failure("pipe2 for the readiness channel failed", e))?;
    let (read_end, write_end) = (i64::from(pipe[0]), i64::from(pipe[1]));
    // A report on a number of a passed descriptor would go into that file,
    // and the parent would read the end of the pipe as a success.
    let read_end = clear_of_passes(plan, read_end).inspect_err(|_| {
        let _ = sys::close(write_end);
    })?;
    let write_end = clear_of_passes(plan, write_end).inspect_err(|_| {
        let _ = sys::close(read_end);
    })?;

    // The limits as numbers, made before the fork: the child only reads
    // them. The child names a refused limit by its index in one byte.
    if plan.limits.len() > crate::limits::MOST {
        let _ = (sys::close(read_end), sys::close(write_end));
        return Err(Error::runtime(
            Class::Usage,
            format!("{} resource limits; a payload takes at most {}", plan.limits.len(), crate::limits::MOST),
        ));
    }
    let limits: Vec<(u64, u64, u64)> = plan.limits.iter().map(|l| (l.resource.number(), l.soft, l.hard)).collect();

    // The parent of the child, for its check after `PR_SET_PDEATHSIG`.
    let parent = sys::getpid();
    let pid = match unsafe { sys::clone_fork(sys::SIGCHLD) } {
        Ok(0) => {
            // The child: async-signal-safe work only, then execve.
            let _ = sys::close(read_end);
            // Four bytes to the readiness pipe: the step and the errno.
            let report_at = |step: u8, e: sys::Errno, at: u8| -> ! {
                let msg = [step, (e.0 & 0xff) as u8, ((e.0 >> 8) & 0xff) as u8, at];
                let _ = sys::write(write_end, &msg);
                sys::exit_group(SETUP_FAILED)
            };
            let report = |step: u8, e: sys::Errno| -> ! { report_at(step, e, 0) };
            // First, the end with the parent (T-205). A parent that ended
            // before the call sends no signal: the child compares its parent.
            if plan.die_with_parent {
                if let Err(e) = sys::set_pdeathsig(9) {
                    report(14, e);
                }
                if sys::getppid() != parent {
                    sys::exit_group(SETUP_FAILED);
                }
            }
            for (child_fd, host_fd) in &plan.fds.pass {
                if host_fd != child_fd {
                    let _ = sys::dup2(*host_fd, *child_fd);
                    let _ = sys::close(*host_fd);
                } else {
                    // On its own number there is no `dup2` to clear the
                    // close-on-exec flag: clear it, so the payload holds it.
                    let _ = sys::fcntl(*child_fd, sys::F_SETFD, 0);
                }
            }
            // The namespace rung: a mount namespace of its own, `/` private
            // and recursive so that its mounts cannot reach the host, then
            // its private tmpfs on the `/tmp` of the image. A failure
            // reports its step, and the parent enters chroot.
            if let Some(m) = ns {
                if let Err(e) = sys::unshare(sys::CLONE_NEWNS) {
                    report(7, e);
                }
                if let Err(e) = sys::mount(&empty, &slash, &empty, sys::MS_REC | sys::MS_PRIVATE) {
                    report(8, e);
                }
                if let Err(e) = sys::mount(&tmpfs, &m.tmp_target, &tmpfs, 0) {
                    report(9, e);
                }
            }
            // `fchdir` onto the checked descriptor: a path can change after
            // its check, and a descriptor cannot. The working directory is
            // the root file system from here until the `chdir` below.
            if let Err(e) = sys::fchdir(root.fd()) {
                report(1, e);
            }
            if chroot {
                let changed = if ns.is_some() { sys::chroot(&rootpath) } else { sys::chroot(&dot) };
                if let Err(e) = changed {
                    report(2, e);
                }
                // `chdir("/")` after the chroot. With no `chdir` the working
                // directory is still the inode of the old root, which is a
                // known way out of a chroot.
                if let Err(e) = sys::chdir(&slash) {
                    report(3, e);
                }
                // The `WorkingDir` of the image. The check before the fork
                // is the usual refusal; this reports a directory that went
                // missing after it, and the payload does not run from `/`.
                if let Err(e) = sys::chdir(&workdir) {
                    report(11, e);
                }
                // The device and inode check: the chroot by path of the
                // namespace rung must land on the opened descriptor. A
                // mismatch is a path that changed or a mount that the new
                // root cannot see. `fstatat` on the descriptor needs no
                // path, so it works after the chroot; `EXDEV` names a
                // landing on another file. `slash`, not `dot`: the working
                // directory can be the `WorkingDir` of the image.
                if ns.is_some() {
                    let anchored = sys::fstatat(root.fd(), &empty, sys::AT_EMPTY_PATH).ok();
                    let landed = sys::fstatat(sys::AT_FDCWD as i64, &slash, 0).ok();
                    if !matches!((anchored, landed), (Some(a), Some(b)) if chroot_landed(&a, &b)) {
                        report(10, sys::EXDEV);
                    }
                }
            } else if let Some(w) = &workdir_host
                && let Err(e) = sys::chdir(w)
            {
                // No chroot: the working directory that the table resolved
                // before the fork, inside the image relative to the
                // descriptor above, or inside a volume by its host path.
                report(11, e);
            }

            // The paths of the rules open here, in the view of the payload:
            // after the passed descriptors, the mounts and the root change,
            // before the limits. Landlock does not read a rule on the
            // directory under a mount point, so a rule must hold the tmpfs
            // of the namespace rung, not the `/tmp` of the image under it.
            if let Some(c) = confined.as_mut()
                && let Err(r) = c.open_paths()
            {
                let _ = sys::write(write_end, &crate::confine::message(r));
                sys::exit_group(SETUP_FAILED)
            }

            // The limits and `no_new_privs` of the payload, after the root
            // change and the `chdir`, before the exec: they reach the
            // payload and no other process.
            for (at, &(resource, soft, hard)) in limits.iter().enumerate() {
                if let Err(e) = sys::set_rlimit(resource, soft, hard) {
                    report_at(12, e, at as u8);
                }
            }
            if plan.no_new_privs
                && let Err(e) = sys::set_no_new_privs()
            {
                report(13, e);
            }
            // The confinement, the last step before the exec: Landlock and
            // the seccomp filter (its deny list holds the mount calls of
            // the namespace rung above). A refusal names its call, and the
            // payload does not run.
            if let Some(c) = &confined
                && let Err(r) = pod_confine::apply(c)
            {
                let _ = sys::write(write_end, &crate::confine::message(r));
                sys::exit_group(SETUP_FAILED)
            }

            // The program resolves here, in the process that changed the
            // root. A memfd from the caller execs with no path, under the
            // same chroot.
            if let Some(mfd) = fd_exec {
                if let Err(e) = unsafe { crate::memfd::exec_fd(mfd, &empty, argv.as_ptr(), envp.as_ptr()) } {
                    let msg = [5u8, (e.0 & 0xff) as u8, ((e.0 >> 8) & 0xff) as u8, 0];
                    let _ = sys::write(write_end, &msg);
                }
                sys::exit_group(NOT_FOUND)
            }
            // The exact argv, exec'd once: the loader names its own
            // failures, so there is no list of candidates.
            if let Some(ov) = &argv_exec {
                let ptrs = argv_exec_ptrs.as_ref().map(|v| v.as_ptr()).unwrap_or(std::ptr::null());
                if let Err(e) = unsafe { sys::execve(&ov[0], ptrs, envp.as_ptr()) } {
                    let msg = [6u8, (e.0 & 0xff) as u8, ((e.0 >> 8) & 0xff) as u8, 0];
                    let _ = sys::write(write_end, &msg);
                }
                sys::exit_group(NOT_FOUND)
            }
            let mut last = sys::Errno(2i32);
            for c in &candidates {
                if let Err(e) = unsafe { sys::execve(c, argv.as_ptr(), envp.as_ptr()) } {
                    last = e;
                }
            }
            // Each candidate failed. 127 is the code of the shell and of
            // docker for "not found".
            let msg = [4u8, (last.0 & 0xff) as u8, ((last.0 >> 8) & 0xff) as u8, 0];
            let _ = sys::write(write_end, &msg);
            sys::exit_group(NOT_FOUND)
        }
        Ok(pid) => pid,
        Err(e) => {
            let _ = sys::close(read_end);
            let _ = sys::close(write_end);
            return Err(errno_failure("clone(2) for the payload failed", e));
        }
    };

    // The parent closes its write end first. Otherwise the read below
    // never sees the end of the file: the copy of this process holds the
    // pipe open.
    let _ = sys::close(write_end);
    // Four bytes for a step, six for a refusal of the confinement.
    let mut buf = [0u8; 6];
    let got = loop {
        match sys::read(read_end, &mut buf) {
            Ok(n) => break n,
            Err(e) if e == sys::EINTR => continue,
            Err(_) => break 0,
        }
    };
    let _ = sys::close(read_end);
    if got == 0 {
        return Ok(Child { pid });
    }
    // The child failed before its exec and said where. It has exited, so
    // the parent reaps it here and leaves no zombie.
    let mut st = 0i32;
    let _ = sys::wait4(pid, &mut st);
    if buf[0] == crate::confine::STEP {
        return Err(crate::confine::failure(confined.as_ref(), &buf));
    }
    let errno = sys::Errno(i32::from(buf[1]) | (i32::from(buf[2]) << 8));
    // A failure of the namespace setup does not fail the run: the caller
    // enters chroot, so these steps come back as `NsSetup`.
    if let Some(step) = ns_setup_step(buf[0]) {
        return Err(Error::NsSetup { step, errno });
    }
    Err(step_failure(plan, buf[0], errno, buf[3]))
}

/// `fd`, or, where the child puts a passed descriptor onto its number, a
/// close-on-exec copy above each such number (the original closes).
fn clear_of_passes(plan: &Plan, fd: i64) -> Result<i64> {
    let targets = || plan.fds.pass.iter().map(|&(child_fd, _)| child_fd);
    if !targets().any(|t| t == fd) {
        return Ok(fd);
    }
    let above = targets().max().unwrap_or(0) + 1;
    let moved = sys::fcntl(fd, sys::F_DUPFD_CLOEXEC, above as u64);
    let _ = sys::close(fd);
    moved.map_err(|e| errno_failure("moving the readiness pipe clear of the passed descriptors failed", e))
}

/// The working directory of an entry with no chroot, resolved through the
/// table before the fork: `None` for the root itself, a path relative to
/// the descriptor of the root file system where the root pair serves it,
/// or the host path of a volume where a volume serves it.
///
/// The walk follows each link as the guest kernel does and keeps the
/// guest path, so the child never follows a link of the image on the
/// host. A table whose root is not `root.path` is refused: the relative
/// path would name a directory of another tree.
fn userland_workdir(root: &RootDir, maps: &Maps, working_dir: &str) -> Result<Option<CBuf>> {
    if maps.rootfs().trim_end_matches('/') != root.path.trim_end_matches('/') {
        return Err(Error::runtime(
            Class::Usage,
            format!(
                "the table of path maps has the root {:?}, and the entry holds the root {:?}",
                maps.rootfs(),
                root.path
            ),
        ));
    }
    let guest = if working_dir.is_empty() { "/" } else { working_dir };
    let (host, parts) = resolve_dir(maps, guest).map_err(|kind| {
        let volume = maps.volume_label(guest);
        refuse_workdir(
            working_dir,
            &format!("in the image's rootfs {:?} or its volumes", root.path),
            kind,
            volume.as_deref(),
        )
    })?;
    let text = if maps.in_volume(&parts) {
        host.display().to_string()
    } else if parts.is_empty() {
        return Ok(None);
    } else {
        parts.join("/")
    };
    CBuf::new(&text).map(Some).ok_or_else(|| Error::runtime(Class::Usage, "the working directory contains a NUL byte"))
}

/// The error for a step that the child reported.
fn step_failure(plan: &Plan, step_byte: u8, errno: sys::Errno, at: u8) -> Error {
    // A refused limit, `no_new_privs` or the death signal: the call, by
    // name, and the payload did not run.
    let call = match step_byte {
        12 => plan.limits.get(usize::from(at)).map(|l| format!("setrlimit({})", l.resource.name())),
        13 => Some("prctl(PR_SET_NO_NEW_PRIVS)".to_string()),
        14 => Some("prctl(PR_SET_PDEATHSIG)".to_string()),
        _ => None,
    };
    if let Some(call) = call {
        return Error::runtime(
            crate::errno_class(errno),
            format!("the payload could not be started: {call}: {} ({})", errno.name(), errno.0),
        );
    }
    let step = match step_byte {
        1 => "fchdir onto the rootfs descriptor",
        2 => "chroot into the rootfs",
        3 => "chdir(\"/\") after the chroot",
        5 => "execveat of the memfd",
        6 => "execve of the userland entry argv",
        11 => "chdir to the working directory",
        _ => "execve of every candidate path",
    };
    // A working directory that went missing names the directory, so the
    // refusal says which `-w` failed.
    if step_byte == 11 {
        return Error::runtime(
            crate::errno_class(errno),
            format!(
                "the working directory {:?} could not be entered: {step} failed with {} ({})",
                plan.working_dir,
                errno.name(),
                errno.0
            ),
        );
    }
    let text = format!("the payload could not be started: {step} failed with {} ({})", errno.name(), errno.0);
    if !matches!(step_byte, 4..=6) {
        return Error::runtime(crate::errno_class(errno), text);
    }
    let named = format!("{:?}: {text}", plan.argv.first().map(String::as_str).unwrap_or(""));
    // 126 and not 127, as docker: `docker run alpine /etc/passwd` exits
    // 126. A caller that reads 127 tries another path; one that reads 126
    // does not.
    if invocable_but_refused(errno) { Error::CannotInvoke(named) } else { Error::NotFound(named) }
}
