//! The one fork of each entry.

use std::io::Write;

use pod_core::Class;
use pod_hostprobe::sys::{self, CBuf};

use crate::entry::{NsEntry, ns_setup_step};
use crate::names::var_name;
use crate::status::NOT_FOUND;
use crate::userland::Maps;
use crate::{Child, Error, Plan, Result, RootDir, errno_failure};

/// The exit code of a child that wrote a failed step to the readiness
/// pipe. The parent reaps that child and reads the step from the pipe, so
/// no caller sees this code. It is docker's code of a runtime error.
const SETUP_FAILED: i32 = 125;

/// The flags of a descriptor of a directory that the child only opens
/// under.
const DIR: u64 = sys::O_PATH | sys::O_DIRECTORY | sys::O_CLOEXEC;

/// The one entry sequence of [`crate::spawn`], [`crate::spawn_ladder`] and
/// [`crate::spawn_userland`].
///
/// `active` is the word of the active mode variable. `fd_exec` is the
/// written memfd that the child execs, where the caller gives one.
/// `argv_exec` is the exact argv that the child execs in place of the
/// candidates (the loader of the entry with no chroot). `userland` is
/// `None` for the chroot sequence, and the table of the run for the
/// sequence with `fchdir` only. `ns` carries the
/// mounts of the namespace rung (its own, or a join), or nothing. `banner`
/// is false on a second fork of one entry (a fallback or a retry): the
/// banner goes out one time. One function, so the fork, the
/// rules of the root and the readiness pipe are the same for each entry.
///
/// Nine parameters, each one used: they are the fixed set of the fork
/// (each thing that the child touches exists before it).
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_with(
    root: &RootDir,
    plan: &Plan,
    active: &str,
    argv_exec: Option<Vec<CBuf>>,
    fd_exec: Option<i64>,
    userland: Option<&Maps>,
    ns: Option<NsEntry<'_>>,
    banner: bool,
    err: &mut dyn Write,
) -> Result<Child> {
    // Before the fork. Each allocation of the child happens here: between
    // `clone` and `execve` only async-signal-safe work is permitted, so
    // nothing after the fork allocates, formats or takes a lock.
    // The child names a refused limit by its index in one byte.
    if plan.limits.len() > crate::limits::MOST {
        return Err(Error::runtime(
            Class::Usage,
            format!("{} resource limits; a payload takes at most {}", plan.limits.len(), crate::limits::MOST),
        ));
    }
    // A limit given two times: the second could fail in the child with
    // `EPERM` after the first lowered the hard value.
    if let Some(r) = crate::limits::repeated(&plan.limits) {
        return Err(Error::LimitTwice(r));
    }
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
    // The exec and the write table of a confinement with no chroot, for
    // the interposer; a value of the caller never reaches the payload.
    let table_vars = crate::confine::table_vars(plan, userland.is_some())?;
    let envp_owned: Vec<CBuf> = plan
        .env
        .iter()
        .filter(|e| {
            let name = var_name(e);
            name != names.mode_request
                && name != names.mode_active
                && name != names.exec_table
                && name != names.write_table
        })
        .chain(std::iter::once(&active_var))
        .chain(table_vars.iter())
        .map(|e| CBuf::new(e).ok_or_else(|| nul(format!("the environment {e:?} contains a NUL"))))
        .collect::<Result<_>>()?;
    let chroot = userland.is_none();
    let workdir = CBuf::new(if plan.working_dir.is_empty() { "/" } else { &plan.working_dir })
        .ok_or_else(|| nul("the working directory contains a NUL byte".to_string()))?;
    let slash = CBuf::new("/").expect("a literal");
    let dot = CBuf::new(".").expect("a literal");
    let tmp_workdir = if ns.is_some() { crate::ns_volume::tmp_workdir_parts(plan) } else { Vec::new() };
    // The empty path through which `execveat` execs a descriptor.
    let empty = sys::cempty();
    // The path by which the namespace rung changes its root (`ns_root`).
    // The entry descriptor was opened before the fork, in the mount
    // namespace of the caller: the mounts of the child are not under it.
    // The path resolves in the new namespace, and the device and inode
    // check after the change refuses a path that changed.
    let rootpath = CBuf::new(&root.path).ok_or_else(|| nul("the root path contains a NUL byte".to_string()))?;
    let hostname = crate::uts::checked(plan)?;
    let hostname_len = plan.hostname.as_ref().map_or(0, String::len);
    let entry = match (&ns, userland) {
        (Some(NsEntry::Own(_)), _) => None,
        (Some(NsEntry::Join(..)), _) => Some("a join of the mount namespace of the payload"),
        (_, Some(_)) => Some("the entry with no chroot"),
        _ => Some("the chroot rung"),
    };
    if let Some(entry) = entry {
        crate::uts::refuse_without_namespace(plan, entry)?;
    }
    let pid_ns = crate::ns_pid::Pid::of(plan, ns.as_ref(), entry)?;
    let shown = crate::title::Shown::of(plan, &pid_ns)?;
    // The volumes: a join binds none, as the mount namespace of the run
    // holds them; an entry with no mount namespace refuses them.
    let volumes = crate::ns_volume::prepare(plan, root, entry.filter(|_| ns.is_none()))?;
    // The nested user namespace of the payload (`ns_lock`).
    let nested = crate::ns_lock::nested_for(plan, ns.as_ref(), userland.is_some())?;
    let proc_self = CBuf::new("/proc/self").expect("a literal");
    let own = match ns {
        Some(NsEntry::Own(m)) => Some(crate::ns_own::OwnSteps {
            hostname: hostname.as_ref(),
            hostname_len,
            cloned_user: pid_ns.own,
            ..crate::ns_own::OwnSteps::new(m, crate::ns_root::RootMount::of(plan, root)?, &volumes, &rootpath)
        }),
        _ => None,
    };

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
    // A directory in a volume resolves in the host side of the volume.
    // On an overlay root with an upper directory, the upper layer shows
    // over the image (`ns_upper`).
    let upper = ns.as_ref().and_then(NsEntry::upper);
    let workdir_host = match userland {
        Some(maps) => crate::userland::userland_workdir(root, maps, &plan.working_dir)?,
        None => crate::ns_volume::check_workdir(root, plan, upper).map(|()| None)?,
    };

    // The confinement of the payload, compiled before the banner and the
    // fork: the child opens the paths of its rules and applies it
    // ([`crate::confine`]).
    let mut confined = crate::confine::compile(plan, chroot.then_some(root), upper)?;
    // The child puts each passed descriptor onto its number first: the
    // root descriptor and the memfd move clear of each such number.
    let mut moved = crate::pass_clear::Moved::default();
    let root_fd = crate::pass_clear::keep(plan, root.fd(), "the root file system", &mut moved)?;
    let fd_exec = fd_exec.map(|fd| crate::pass_clear::keep(plan, fd, "the memfd", &mut moved)).transpose()?;

    // The banner before the fork, so it cannot mix with the first output
    // of the payload. It goes to the error writer, never to the standard
    // output.
    if banner {
        let _ = write!(err, "{}", plan.banner);
        let _ = err.flush();
    }

    // The readiness pipe. `O_CLOEXEC` on both ends: the `execve` closes the
    // write end, and the read of the parent then gives the end of the file.
    let mut pipe = [0i32; 2];
    sys::pipe2(&mut pipe, sys::O_CLOEXEC).map_err(|e| errno_failure("pipe2 for the readiness channel failed", e))?;
    let (read_end, write_end) = (i64::from(pipe[0]), i64::from(pipe[1]));
    // A report on a number of a passed descriptor would go into that file,
    // and the parent would read the end of the pipe as a success.
    let read_end = crate::pass_clear::clear_of_passes(plan, read_end).inspect_err(|_| {
        let _ = sys::close(write_end);
    })?;
    let write_end = crate::pass_clear::clear_of_passes(plan, write_end).inspect_err(|_| {
        let _ = sys::close(read_end);
    })?;
    let pid_ns = pid_ns.with_parent(plan.die_with_parent).inspect_err(|_| {
        let _ = (sys::close(read_end), sys::close(write_end));
    })?;
    // The two pipes of the maps that the parent writes (`ns_lock`).
    let maps_pipes = crate::ns_lock::Pipes::of(plan, nested.as_ref()).inspect_err(|_| {
        let _ = (sys::close(read_end), sys::close(write_end));
        pid_ns.close();
    })?;
    let own = own.map(|o| crate::ns_own::OwnSteps { root_fd, ..o });

    // The limits as numbers, made before the fork: the child only reads
    // them.
    let limits: Vec<(u64, u64, u64)> = plan.limits.iter().map(|l| (l.resource.number(), l.soft, l.hard)).collect();

    // The parent of the child, for its check after `PR_SET_PDEATHSIG`.
    let parent = sys::getpid();
    let pid = match unsafe { sys::clone_fork(sys::SIGCHLD | pid_ns.flags) } {
        Ok(0) => {
            // The child: async-signal-safe work only, then execve.
            let _ = sys::close(read_end);
            maps_pipes.close_parent_ends();
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
                if pid_ns.parent_ended(parent) {
                    sys::exit_group(SETUP_FAILED);
                }
            }
            // The raise of a hard limit before a join and the lock, which
            // take away the capability that it needs (T-1638).
            if let Err((e, at)) = crate::limits::raise_hard(&limits) {
                report_at(12, e, at);
            }
            // The join comes before the passed descriptors, so a pass onto
            // the number of the pidfd cannot replace it.
            if let Some(NsEntry::Join(j, _)) = ns
                && let Err((step, e)) = j.enter()
            {
                report(step, e);
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
            // The namespace rung (`ns_own`): a mount namespace of its own,
            // `/` private and recursive so that its mounts cannot reach the
            // host, the mounts, then `pivot_root` into the root. A failure
            // reports its step, and the parent enters chroot. A descriptor of
            // `/proc/self` first, for the maps of the nested namespace.
            let self_fd = match nested.as_ref().map(|_| sys::open(&proc_self, DIR, 0)) {
                Some(Err(e)) => report(crate::ns_user::NS_NESTED_USER_STEP, e),
                Some(Ok(fd)) => fd,
                None => -1,
            };
            if let Some(Err((step, e, at))) = own.as_ref().map(crate::ns_own::OwnSteps::run) {
                report_at(step, e, at);
            }
            // The nested user namespace (`ns_lock`): after each mount, and
            // after the pivot (the kernel refuses a chrooted process), or
            // before the chroot of another entry.
            if let Some(n) = &nested
                && let Err((step, e)) = n.enter(self_fd, &maps_pipes)
            {
                report_at(step, e, n.at());
            }
            let _ = sys::close(self_fd);
            maps_pipes.close_child_ends();
            // No ambient capability crosses the exec on the namespace rung:
            // a payload that is not uid 0 keeps it with no lock (T-1641).
            if ns.is_some() {
                let _ = sys::ambient_clear();
            }
            // `fchdir` onto the checked descriptor: a path can change after
            // its check, and a descriptor cannot. The working directory is
            // the root file system from here until the `chdir` below. The
            // namespace rung is in its root already, and a join of a run of
            // it too: the join sets the root of the mount namespace. A join
            // never takes a directory of the caller as its working
            // directory: it changes its root by path.
            let in_root = match ns {
                Some(NsEntry::Own(_)) => true,
                Some(n @ NsEntry::Join(..)) => crate::ns_root::landed(&n, root_fd, &empty, &slash),
                None => false,
            };
            if ns.is_none()
                && let Err(e) = sys::fchdir(root_fd)
            {
                report(1, e);
            }
            if chroot {
                // A join of a run with no pivot changes its root by path.
                let changed = match ns {
                    _ if in_root => Ok(0),
                    Some(_) => sys::chroot(&rootpath),
                    None => sys::chroot(&dot),
                };
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
                // Under the tmpfs of the namespace rung, a directory of the
                // image is hidden: the child makes it there.
                if let Err(e) = sys::chdir(&workdir)
                    && !(e == sys::ENOENT
                        && !tmp_workdir.is_empty()
                        && crate::ns_volume::make_workdir(&slash, &tmp_workdir).is_ok())
                {
                    report(11, e);
                }
                // The device and inode check: the root of the namespace rung
                // must be the opened descriptor (`ns_root::landed`).
                if let Some(n) = ns
                    && !crate::ns_root::landed(&n, root_fd, &empty, &slash)
                {
                    report(10, sys::EXDEV);
                }
            } else if let Some(w) = &workdir_host
                && let Err(e) = sys::chdir(w)
            {
                // No chroot: the working directory that the table resolved
                // before the fork, inside the image relative to the
                // descriptor above, or inside a volume by its host path.
                report(11, e);
            }
            // The title of the init or the relay, before the limits and the
            // rules: the payload reads no argument or variable of the caller.
            if let Some(s) = &shown {
                s.apply();
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
            if let Err((e, at)) = crate::limits::apply(&limits) {
                report_at(12, e, at);
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

            // The payload of a PID namespace forks here, under its init or
            // its relay (`ns_pid`).
            if let Err((step, e)) = pid_ns.split() {
                report(step, e);
            }
            // The payload starts with an empty mask. A mask passes through
            // `execve`, so a signal that the caller blocks in this thread
            // would never reach the payload. Here, after the split: the init
            // and the relay keep the mask of the caller until they block
            // their own. The call fails only on a bad size or address,
            // which `sys` fixes.
            let _ = sys::sigrestore(0);
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
            let last = unsafe { crate::status::exec_each(&candidates, argv.as_ptr(), envp.as_ptr()) };
            // Each candidate failed. The parent reads the errno and gives
            // 126 or 127, the codes of the shell and of docker.
            let msg = [4u8, (last.0 & 0xff) as u8, ((last.0 >> 8) & 0xff) as u8, 0];
            let _ = sys::write(write_end, &msg);
            sys::exit_group(NOT_FOUND)
        }
        Ok(pid) => pid,
        Err(e) => {
            let _ = (sys::close(read_end), sys::close(write_end));
            pid_ns.close();
            maps_pipes.close_parent_ends();
            maps_pipes.close_child_ends();
            // A refused PID namespace is a refusal of the rung.
            if let Some(step) = ns_setup_step(pid_ns.clone_step()).filter(|_| pid_ns.flags != 0) {
                return Err(Error::NsSetup { step, errno: e });
            }
            return Err(errno_failure("clone(2) for the payload failed", e));
        }
    };
    pid_ns.close();
    // The maps of the nested namespace that the parent writes (`ns_lock`).
    maps_pipes.close_child_ends();
    maps_pipes.serve(nested.as_ref(), pid);
    maps_pipes.close_parent_ends();

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
        return Ok(Child { pid, entered: crate::rung_of(active) });
    }
    // The child failed before its exec and said where. It has exited, so
    // the parent reaps it here and leaves no zombie.
    let mut st = 0i32;
    let _ = sys::wait4(pid, &mut st);
    if buf[0] == crate::confine::STEP {
        return Err(crate::confine::failure(confined.as_ref(), &buf));
    }
    let errno = sys::Errno(i32::from(buf[1]) | (i32::from(buf[2]) << 8));
    // A refused volume names the volume, and never falls back; nor does a
    // refused read-only root.
    if let Some(e) = crate::ns_volume::failure(plan, buf[0], errno, buf[3])
        .or_else(|| crate::ns_root::failure(buf[0], errno, buf[3]))
    {
        return Err(e);
    }
    // A failure of the namespace setup does not fail the run: the caller
    // enters chroot, so these steps come back as `NsSetup`.
    if let Some(step) = ns_setup_step(buf[0]) {
        return Err(Error::NsSetup { step, errno });
    }
    Err(crate::status::step_failure(plan, buf[0], errno, buf[3]))
}
