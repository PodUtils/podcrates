use super::*;

/// The one entry sequence [`spawn`], [`spawn_ladder`] and [`spawn_userland`] share.
///
/// `active` is the word the payload reads as `PODVM_ACTIVE_MODE`, and
/// `fd_exec` is the written memfd the child execs where one was handed in.
/// `argv_exec` is the exact argv the child execs instead of resolving
/// candidates (the userland loader invocation); `chroot` selects the
/// chroot sequence or the fchdir-only one; `ns` carries the namespace
/// rung's mount or nothing. One function, so the fork, the
/// root discipline and the readiness pipe cannot drift between the three
/// entries (`docs/conventions/code.md`).
///
/// ⚠ Eight parameters, each one used. They are the fork shape's fixed
/// set (everything the child touches is built before it), not a bundle
/// looking for a struct.
#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_with(
    root: &RootDir,
    plan: &Plan,
    active: &str,
    argv_exec: Option<Vec<CBuf>>,
    fd_exec: Option<i64>,
    chroot: bool,
    ns: Option<&NsMount>,
    err: &mut dyn Write,
) -> Result<Child> {
    // ---------------------------------------------------------- before the fork
    //
    // ⛔ Every allocation the child needs happens HERE. Between `clone` and
    // `execve` only async-signal-safe work is permitted, so nothing below the
    // fork may allocate, format a string, or take a lock.
    let argv_owned: Vec<CBuf> = plan
        .argv
        .iter()
        .map(|a| {
            CBuf::new(a)
                .ok_or_else(|| Error::Runtime(format!("the argument {a:?} contains a NUL byte")))
        })
        .collect::<Result<_>>()?;
    let envp_owned: Vec<CBuf> = {
        // ⭐ T-1003. The request variable never crosses the exec, whatever built
        // the plan: [`Plan::env_for`] scrubs what it builds, and this filters
        // what it is handed, so a hand-built plan cannot leak one either. The
        // rung actually entered is reported under a different name beside it,
        // so a nested `podvm run` starts unforced. Which name that is depends
        // on the entry: [`spawn`] reports [`ENTERED_RUNG`], [`spawn_ladder`]
        // reports the ladder rung that drove.
        let active_var = format!("{}={active}", Plan::MODE_ACTIVE_VAR);
        plan.env
            .iter()
            .filter(|e| {
                let name = e.split('=').next().unwrap_or("");
                name != Plan::MODE_REQUEST_VAR && name != Plan::MODE_ACTIVE_VAR
            })
            .chain(std::iter::once(&active_var))
            .map(|e| {
                CBuf::new(e)
                    .ok_or_else(|| Error::Runtime(format!("the environment {e:?} contains a NUL")))
            })
            .collect::<Result<_>>()?
    };
    let workdir = CBuf::new(if plan.working_dir.is_empty() {
        "/"
    } else {
        &plan.working_dir
    })
    .ok_or_else(|| Error::Runtime("the working directory contains a NUL byte".into()))?;
    // ⭐ T-1317. Without a chroot the working directory is addressed
    // relatively from the rootfs descriptor the child fchdirs onto: the
    // guest-absolute path minus its leading slash. A NUL byte falls back
    // to the rootfs itself rather than refusing past allocation time.
    let workdir_rel: Option<CBuf> = if chroot {
        None
    } else {
        let rel = plan.working_dir.trim_start_matches('/');
        if rel.is_empty() {
            None
        } else {
            CBuf::new(rel)
        }
    };
    let slash = CBuf::new("/").expect("a literal");
    let dot = CBuf::new(".").expect("a literal");
    // ⭐ T-1339. The fstype and source the namespace rung mounts, built
    // before the fork like everything else the child touches.
    let tmpfs = CBuf::new("tmpfs").expect("a literal");
    // ⭐ T-1003. The empty path `execveat` execs a descriptor through, built
    // before the fork like everything else the child touches: between `clone`
    // and `execve` only async-signal-safe work is permitted.
    let empty = sys::cempty();
    // ⭐ T-1339. The rootfs path the namespace rung chroots by, built
    // before the fork like everything else the child touches. The
    // entry fd was opened before the fork in the base mount namespace,
    // and fchdir+chroot(".") anchors the new root in that mount: the
    // tmpfs this child mounts in its own namespace stays invisible
    // under it (measured on the wsl-toolkit base, kernel
    // 7.2.0-WSL2-STABLE). The path resolves in this namespace, under
    // the private mount. `rootpath` is that path; the (d,i) guard
    // past the chroot refuses a swapped one.
    let rootpath = CBuf::new(&root.path).expect("root path has no NUL");

    let mut argv: Vec<*const u8> = argv_owned.iter().map(|c| c.ptr() as *const u8).collect();
    argv.push(std::ptr::null());
    let mut envp: Vec<*const u8> = envp_owned.iter().map(|c| c.ptr() as *const u8).collect();
    envp.push(std::ptr::null());
    // ⭐ T-1317. The exact argv's pointer table, built here for the same
    // reason: nothing below the fork may allocate.
    let argv_exec_ptrs: Option<Vec<*const u8>> = argv_exec.as_ref().map(|ov| {
        let mut v: Vec<*const u8> = ov.iter().map(|c| c.ptr() as *const u8).collect();
        v.push(std::ptr::null());
        v
    });

    // ⭐ T-0502. The program is resolved INSIDE the new root, so the candidate
    // paths are built here and tried there. A parent-resolved absolute path
    // names the outer tree, and the outer tree is gone after the chroot.
    let candidates: Vec<CBuf> = plan
        .program_candidates()
        .into_iter()
        .filter_map(|p| CBuf::new(&p))
        .collect();
    if candidates.is_empty() {
        return Err(Error::NotFound(format!(
            "{:?}: no candidate path could be built for it",
            plan.argv.first().map(String::as_str).unwrap_or("")
        )));
    }

    // ⭐ T-1412. The working directory is checked before the fork, on both
    // rungs: a missing one refuses here naming it, rather than running from
    // the wrong directory with exit 0. The CLI names the image where it
    // checks ([`check_workdir`]); here only the rootfs is known. A NUL byte
    // skips this check and fails at the `CBuf` above with its own sentence.
    if !plan.working_dir.contains('\0')
        && let Err(kind) = workdir_kind(&root.path, &plan.working_dir)
    {
        return Err(refuse_workdir(
            &plan.working_dir,
            &format!("in the image's rootfs {:?}", root.path),
            kind,
        ));
    }

    // ⛔ The banner before the fork, so it cannot interleave with the payload's
    // own first output. T-1104: stderr, never stdout.
    let _ = write!(err, "{}", plan.banner);
    let _ = err.flush();

    // ⭐ T-0602's readiness channel, created before the fork like everything
    // else the child touches. `O_CLOEXEC` on both ends is the whole trick: the
    // `execve` closes the write end, and the parent's read then returns EOF.
    let mut pipe = [0i32; 2];
    sys::pipe2(&mut pipe, sys::O_CLOEXEC).map_err(|e| {
        Error::Runtime(format!(
            "pipe2 for the readiness channel failed: {} ({})",
            e.name(),
            e.0
        ))
    })?;
    let (read_end, write_end) = (pipe[0] as i64, pipe[1] as i64);

    // --------------------------------------------------------------- the fork
    let pid = match unsafe { sys::clone_fork(sys::SIGCHLD) } {
        Ok(0) => {
            // ------ child. Async-signal-safe work only, then execve. ------
            let _ = sys::close(read_end);
            // ⚠ Four bytes to the readiness pipe, which is async-signal-safe and
            // is all the parent needs to tell a failure from a successful exec.
            let report = |step: u8, e: sys::Errno| -> ! {
                let msg = [step, (e.0 & 0xff) as u8, ((e.0 >> 8) & 0xff) as u8, 0];
                let _ = sys::write(write_end, &msg);
                sys::exit_group(EXIT_RUNTIME_ERROR)
            };
            for (child_fd, host_fd) in &plan.fds.pass {
                if host_fd != child_fd {
                    let _ = sys::dup2(*host_fd, *child_fd);
                    let _ = sys::close(*host_fd);
                }
            }
            // ⭐ T-1339. The namespace rung, inside the child: a mount
            // namespace of its own, `/` recursively private so its
            // mounts cannot propagate to the host, then its private
            // tmpfs on the image's `/tmp`. Any failure reports its step
            // and the parent enters chroot instead; nothing here may
            // allocate, format, or take a lock.
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
            // ⛔ `fchdir` onto the checked descriptor: a path checked and
            // then passed can be swapped in between, and a descriptor
            // cannot (TODO/enter.md T-0504). The working directory is the
            // rootfs from here until `chdir` moves it below.
            if let Err(e) = sys::fchdir(root.fd) {
                report(1, e);
            }
            if chroot {
                if ns.is_some() {
                    // ⭐ T-1339. The path chroot on the namespace rung. The
                    // entry descriptor was opened before the fork, in the
                    // base mount namespace, and `fchdir` plus `chroot(".")`
                    // anchors the new root in that mount: the tmpfs this
                    // child mounted in its own namespace stays invisible
                    // under it. Measured on the wsl-toolkit base, kernel
                    // 7.2.0-WSL2-STABLE: the child's own stat of the mount
                    // target read tmpfs while `/tmp` past a dot chroot read
                    // the host directory, with the (d,i) below agreeing on
                    // the same directory throughout. The path resolves in
                    // this namespace, under the private mount. The (d,i)
                    // guard below refuses a swapped path rather than
                    // entering it, so T-0504's discipline holds here too.
                    if let Err(e) = sys::chroot(&rootpath) {
                        report(2, e);
                    }
                } else if let Err(e) = sys::chroot(&dot) {
                    report(2, e);
                }
                // ⛔ `chdir("/")` after the chroot. Without it the working directory
                // is still the old root's inode, which is a documented way out of a
                // chroot and is not a containment podvm may claim.
                if let Err(e) = sys::chdir(&slash) {
                    report(3, e);
                }
                // The image's WorkingDir, if it exists. ⛔ T-1412: a missing
                // one refuses rather than running from `/` with exit 0. The
                // pre-fork check above is the ordinary refusal; this reports
                // the race where the directory went missing between the check
                // and here, instead of discarding the error and running wrong.
                if let Err(e) = sys::chdir(&workdir) {
                    report(11, e);
                }
                // ⭐ T-1339. The (d,i) guard: the path chroot above must
                // land on the opened descriptor. A mismatch is a swapped
                // path (T-0504's class) or a mount the new root cannot
                // see, and it refuses here, before the exec, rather than
                // entering wrong: the parent falls back to chroot with
                // the step named. `fstatat` on the descriptor needs no
                // path, so it answers past the chroot; `EXDEV` names a
                // landing off its file. `slash`, not `dot`: the working
                // directory below may be the image's WorkingDir.
                if ns.is_some() {
                    let anchored = sys::fstatat(root.fd, &empty, sys::AT_EMPTY_PATH).ok();
                    let landed = sys::fstatat(sys::AT_FDCWD as i64, &slash, 0).ok();
                    if !matches!((anchored, landed), (Some(a), Some(b)) if chroot_landed(&a, &b)) {
                        report(10, sys::EXDEV);
                    }
                }
            } else {
                // ⭐ T-1317. No chroot: the working directory stays inside
                // the image, addressed relatively from the descriptor above.
                // ⛔ T-1412, as in the chroot arm: a missing one refuses as
                // step 11 rather than running from the rootfs with exit 0.
                if let Some(w) = &workdir_rel
                    && let Err(e) = sys::chdir(w)
                {
                    report(11, e);
                }
            }

            // ⭐ Resolved HERE, in the process that changed the root. Where the
            // ladder handed a written memfd in, it is exec'd without resolving
            // a path at all; the chroot above still holds beneath it.
            if let Some(mfd) = fd_exec {
                if let Err(e) =
                    unsafe { crate::memfd::exec_fd(mfd, &empty, argv.as_ptr(), envp.as_ptr()) }
                {
                    let msg = [5u8, (e.0 & 0xff) as u8, ((e.0 >> 8) & 0xff) as u8, 0];
                    let _ = sys::write(write_end, &msg);
                }
                sys::exit_group(EXIT_NOT_FOUND)
            }
            // ⭐ T-1317. The exact argv, exec'd once: the loader names its own
            // failures, so there is no candidate list to walk. A memfd above
            // takes precedence: it is the staged bytes, not a path.
            if let Some(ov) = &argv_exec {
                let ptrs = argv_exec_ptrs
                    .as_ref()
                    .map(|v| v.as_ptr())
                    .unwrap_or(std::ptr::null());
                if let Err(e) = unsafe { sys::execve(&ov[0], ptrs, envp.as_ptr()) } {
                    let msg = [6u8, (e.0 & 0xff) as u8, ((e.0 >> 8) & 0xff) as u8, 0];
                    let _ = sys::write(write_end, &msg);
                }
                sys::exit_group(EXIT_NOT_FOUND)
            }
            let mut last = sys::Errno(2i32);
            for c in &candidates {
                if let Err(e) = unsafe { sys::execve(c, argv.as_ptr(), envp.as_ptr()) } {
                    last = e;
                }
            }
            // Every candidate failed. 127 is the shell's convention and
            // docker's for "not found", and the parent reports it unchanged.
            let msg = [4u8, (last.0 & 0xff) as u8, ((last.0 >> 8) & 0xff) as u8, 0];
            let _ = sys::write(write_end, &msg);
            sys::exit_group(EXIT_NOT_FOUND)
        }
        Ok(pid) => pid,
        Err(e) => {
            let _ = sys::close(read_end);
            let _ = sys::close(write_end);
            return Err(Error::Runtime(format!(
                "clone(2) for the payload failed: {} ({})",
                e.name(),
                e.0
            )));
        }
    };

    // -------------------------------------------------------------- the parent
    // ⛔ The parent closes ITS write end first, or the read below never sees
    // EOF: the pipe stays open on this process's own copy of it forever.
    let _ = sys::close(write_end);
    let mut buf = [0u8; 4];
    let got = loop {
        match sys::read(read_end, &mut buf) {
            Ok(n) => break n,
            Err(e) if e == sys::EINTR => continue,
            Err(_) => break 0,
        }
    };
    let _ = sys::close(read_end);
    if got > 0 {
        // The child failed before its exec and said where. It has already
        // exited, so it is reaped here rather than left as a zombie.
        let mut st = 0i32;
        let _ = sys::wait4(pid, &mut st);
        let errno = sys::Errno(i32::from(buf[1]) | (i32::from(buf[2]) << 8));
        // ⭐ T-1339. The namespace setup never fails the run: the caller
        // enters chroot instead, so these steps arrive as `NsSetup`
        // rather than as a refusal. The step names live in
        // [`ns_setup_step`], beside the unit test that pins them.
        if let Some(step) = ns_setup_step(buf[0]) {
            return Err(Error::NsSetup { step, errno });
        }
        let step = match buf[0] {
            1 => "fchdir onto the rootfs descriptor",
            2 => "chroot into the rootfs",
            3 => "chdir(\"/\") after the chroot",
            // ⭐ T-1003: the ladder's fd-exec, which never resolves a path.
            5 => "execveat of the memfd",
            // ⭐ T-1317: the userland entry argv, exec'd once.
            6 => "execve of the userland entry argv",
            // ⭐ T-1412: the working directory, raced missing past the
            // pre-fork check that ordinarily refuses it.
            11 => "chdir to the working directory",
            _ => "execve of every candidate path",
        };
        // ⭐ T-1412: the raced workdir names the directory it could not
        // enter, so the refusal answers which `-w` failed even past the
        // pre-fork check.
        let text = if buf[0] == 11 {
            format!(
                "the working directory {:?} could not be entered: {step} failed \
                 with {} ({}) (TODO/enter.md T-1412)",
                plan.working_dir,
                errno.name(),
                errno.0
            )
        } else {
            format!(
                "the payload could not be started: {step} failed with {} ({})",
                errno.name(),
                errno.0
            )
        };
        let named = format!(
            "{:?}: {text}",
            plan.argv.first().map(String::as_str).unwrap_or("")
        );
        return Err(if buf[0] != 4 && buf[0] != 5 && buf[0] != 6 {
            Error::Runtime(text)
        } else if invocable_but_refused(errno) {
            // ⭐ 126 and not 127, and the difference is docker's. Measured by
            // `experiments/330-exit-codes.sh` on 2026-09-09: `docker run alpine
            // /etc/passwd` exits **126** and podvm exited 127, because every
            // execve failure was folded into "not found". A caller branching on
            // 127 retries with a different path; one branching on 126 does not.
            Error::CannotInvoke(named)
        } else {
            Error::NotFound(named)
        });
    }
    Ok(Child { pid })
}
