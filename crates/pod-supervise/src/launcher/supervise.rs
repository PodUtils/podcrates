use super::*;

/// The launcher itself. ⚠ Runs in the forked child and never returns to the
/// caller's code: its return value is the process's exit code.
pub(super) fn supervise(
    store: &Store,
    container: &table::Container,
    record: &podvm_image::Record,
    log: std::fs::File,
    ready_w: i64,
) -> i32 {
    let say = |msg: &str| {
        let _ = sys::write(ready_w, msg.as_bytes());
    };

    // ⛔ A session of its own, so this outlives the shell that started it and
    // holds no controlling terminal. T-0603's other half.
    let _ = sys::setsid();

    // ⛔ **AND ITS OWN STDIO GOES TO /dev/null. THIS IS
    // [`TODO/supervise.md`](../../../TODO/supervise.md) T-0608.**
    //
    // A `clone` copies every descriptor and this process never execs, so
    // without this it holds the CALLER's stdin, stdout and stderr for the
    // container's whole life. ⭐ The caller then cannot finish reading its own
    // stdout: `id=$(podvm run -d ...)` is the way every script writes it, and
    // command substitution reads the pipe to EOF, which does not come until the
    // last holder closes it. Measured on 2026-09-09 by
    // `experiments/340-detached-stdio.sh`: `run -d` with a `sleep 25` payload
    // returned its id in under a second when stdout was a file and took 25
    // seconds under `$( )`, and the launcher's `/proc/<pid>/fd/1` was the
    // caller's pipe.
    //
    // ⚠ **This is what was seen as "`-d` returned an id and the container had
    // already exited".** It had: the caller was released when the payload
    // ended, and by then the launcher had written the terminal record. The two
    // candidates T-0608 wrote down -- the readiness bound, and `reconcile`
    // racing `start` -- were both wrong, which is why the entry refused to act
    // on either.
    //
    // ⛔ A refusal rather than a shrug when `/dev/null` cannot be opened: a
    // launcher that cannot let go of the caller's stdio will hang it, and
    // hanging silently is the failure this whole comment is about.
    match sys::CBuf::new("/dev/null")
        .ok_or(sys::Errno(2))
        .and_then(|c| {
            let fd = sys::open(&c, sys::O_RDWR, 0)?;
            for target in [0i64, 1, 2] {
                if fd != target {
                    sys::dup2(fd, target)?;
                }
            }
            if fd > 2 {
                let _ = sys::close(fd);
            }
            Ok(0)
        }) {
        Ok(_) => {}
        Err(e) => {
            say(&format!(
                "err the launcher could not put its own stdio on /dev/null: {} \
                 ({}). It would hold the caller's stdout open for the whole life \
                 of the container (T-0608)",
                e.name(),
                e.0
            ));
            return 1;
        }
    }

    // ⛔ THE LOCK BEFORE ANYTHING IS WRITTEN. T-0604: this lock IS the statement
    // "a launcher is alive for this container", and a reconciler reads it by
    // trying to take it. Taking it after writing `running` leaves a window in
    // which the table says running and nothing holds the lock.
    let lock_path = table::lock_path(store, &container.id);
    let lock = match Lock::try_exclusive(&lock_path) {
        Ok(Some(l)) => l,
        Ok(None) => {
            say(&format!(
                "err another launcher already holds {}",
                lock_path.display()
            ));
            return 1;
        }
        Err(e) => {
            say(&format!("err {e}"));
            return 1;
        }
    };

    // The control socket. ⚠ A stale one from a launcher that was killed is
    // removed first: nothing is listening on it, and its presence would make
    // `bind` fail for a container that is legitimately starting again.
    let ctl_path = table::control_path(store, &container.id);
    let _ = std::fs::remove_file(&ctl_path);
    let (_ctl_dir, bind_path) = match table::control_via_fd(store, &container.id) {
        Ok(x) => x,
        Err(e) => {
            say(&format!("err {e}"));
            return 1;
        }
    };
    let listener = match UnixListener::bind(&bind_path) {
        Ok(l) => l,
        Err(e) => {
            say(&format!("err binding the control socket: {e}"));
            return 1;
        }
    };

    // ⭐ The payload's stdio, opened before the root changes. Two dups of the
    // log, because the child closes each host fd after `dup2`ing it.
    let null = std::fs::OpenOptions::new()
        .read(true)
        .open("/dev/null")
        .ok();
    let mut pass = Vec::new();
    if let Some(n) = &null {
        pass.push((0i64, n.as_raw_fd() as i64));
    }
    let out = log.try_clone();
    let err_sink = log.try_clone();
    if let (Ok(o), Ok(e)) = (&out, &err_sink) {
        pass.push((1, o.as_raw_fd() as i64));
        pass.push((2, e.as_raw_fd() as i64));
    }
    // ⭐ T-0710: the host memo, beside this container's record. Opened here,
    // before the root changes, and dup'd to the constant child number the
    // environment names. An entry that cannot be handed one refuses rather
    // than starting with no record.
    let memo_path = table::memo_path(store, &container.id);
    let memo = match table::open_memo(&memo_path) {
        Ok(f) => f,
        Err(e) => {
            say(&format!("err the ownership memo could not be opened: {e}"));
            return 1;
        }
    };
    pass.push((table::MEMO_CHILD_FD, memo.as_raw_fd() as i64));

    // ⭐ TODO/enter.md T-0501. The container's device spec re-opens HERE,
    // in the process that enters: a descriptor cannot cross the client
    // that ran `create`, so the record carries the spec and each entry
    // opens it before its own root changes. A host path gone since
    // `create` refuses the start naming it, rather than starting a
    // container whose mapping serves nothing.
    let mut env = container.env.clone();
    match podvm_enter::device::specs_of(&env).and_then(|s| podvm_enter::device::open_all(&s)) {
        Ok(opened) => {
            pass.extend(opened.pass.iter().copied());
            podvm_enter::device::push_serve(&mut env, &opened);
        }
        Err(e) => {
            say(&format!("err starting with --device: {e}"));
            return 1;
        }
    }

    let root = match RootDir::open(&container.rootfs) {
        Ok(r) => r,
        Err(e) => {
            say(&format!("err {e}"));
            return 1;
        }
    };
    // The stored environment already carries the constant descriptor number
    // from `create`; containers made before T-0710 do not, so ensure it here
    // rather than refusing a record that predates the rule.
    if table::memo_fd_of(&env).is_none() {
        env.push(table::memo_fd_env());
    }
    let plan = Plan {
        argv: container.argv.clone(),
        env: env.clone(),
        working_dir: container.working_dir.clone(),
        fds: Fds { pass },
        // ⚠ Empty: the banner was printed by the process the caller was
        // watching, before it forked this one. Printing it again here would put
        // it in the container's LOG, where it is not the payload's output.
        banner: String::new(),
        path_dirs: podvm_enter::Plan::path_from(&env),
    };

    // ⭐ TODO/image.md T-0204 and T-0211. The image lock is taken here and handed
    // to the payload immediately before the fork that leads to its exec, so a
    // concurrent `rmi` or `prune` cannot delete the rootfs a running container
    // is executing out of. ⛔ In the LAUNCHER and not in the caller: the caller
    // exits as soon as the container is up, and a lock it held would go with it.
    let held = match store.hold(record) {
        Ok(h) => h,
        Err(e) => {
            say(&format!("err holding the image: {e}"));
            return 1;
        }
    };
    if let Err(e) = held.hand_to_payload() {
        say(&format!("err handing the image lock to the payload: {e}"));
        return 1;
    }

    let sink = std::io::sink();
    let mut entry_log: Box<dyn std::io::Write> = match &err_sink {
        Ok(f) => match f.try_clone() {
            Ok(c) => Box::new(c),
            Err(_) => Box::new(sink),
        },
        Err(_) => Box::new(sink),
    };
    // ⭐ TODO/enter.md T-1411. This reverses T-1317's last-resort decision
    // at the launcher: where the record carries the loader drive `create`
    // decided (the exact argv, with the libraries, the `PODVM_MAPS`
    // table and `PODVM_GUEST_EXE` in the stored environment), the
    // launcher execs it with no chroot through `spawn_userland` and
    // supervises the pidfd exactly as the chroot path below does. The
    // readiness answer and the `running` write are the same ones, so
    // `ps`, `logs`, `stop` and `rm` cannot tell the rung apart.
    let child = if container.userland_exec.is_empty() {
        // ⭐ TODO/enter.md T-1339. The rung the probe selects, decided where
        // the payload is entered: a detached start on a namespace host
        // enters the namespace rung like a foreground run does. The
        // fallback line joins the container log beside the payload's own
        // output: this process's stderr is /dev/null and the ready pipe
        // carries only the start protocol, so the log is the one place the
        // fallback stays readable.
        let findings = podvm_probe::run();
        let selection = podvm_probe::select::Selection::choose(&findings);
        match podvm_enter::spawn_selected(&root, &plan, selection.rung, &mut entry_log) {
            Ok(c) => c,
            Err(e) => {
                say(&format!("err {e}"));
                return 1;
            }
        }
    } else {
        let exec_argv = container.userland_exec.clone();
        match podvm_enter::spawn_userland(
            &root,
            &plan,
            exec_argv,
            podvm_probe::select::Rung::Userland.word(),
            None,
            &mut entry_log,
        ) {
            Ok(c) => c,
            Err(e) => {
                say(&format!("err {e}"));
                return 1;
            }
        }
    };
    drop(out);
    drop(err_sink);
    drop(null);

    // ⭐ T-0601. A pidfd from the moment the child exists, and every wait below
    // is on it rather than on a pid.
    let pidfd = match sys::pidfd_open(child.pid) {
        Ok(fd) => fd,
        Err(e) => {
            // ⛔ Not recoverable and not hidden: without a pidfd this launcher
            // would have to fall back to a pid, which is what T-0601 exists to
            // remove. The payload is killed rather than supervised badly.
            let _ = sys::kill(child.pid, 9);
            let mut st = 0i32;
            let _ = sys::wait4(child.pid, &mut st);
            say(&format!(
                "err pidfd_open on the payload failed: {} ({}). podvm will not \
                 supervise a container by pid",
                e.name(),
                e.0
            ));
            return 1;
        }
    };

    let launcher_pid = sys::getpid();
    if let Err(e) = table::update(store, |t| {
        if let Some(c) = t.containers.iter_mut().find(|c| c.id == container.id) {
            c.state = State::Running;
            c.pid = Some(child.pid);
            c.launcher_pid = Some(launcher_pid);
            c.started_at = Some(podvm_image::clock::now());
        }
        Ok(())
    }) {
        say(&format!("err recording the container as running: {e}"));
        let _ = sys::kill(child.pid, 9);
        return 1;
    }

    // ⭐ The payload is running and this is where the caller is released.
    say(&format!("ok {launcher_pid} {}", child.pid));
    let _ = sys::close(ready_w);

    let exit = serve(&listener, pidfd, &child);
    let _ = sys::close(pidfd);
    let _ = std::fs::remove_file(&ctl_path);

    let waiters_code = exit;
    let _ = table::update(store, |t| {
        if let Some(c) = t.containers.iter_mut().find(|c| c.id == container.id) {
            c.state = State::Exited;
            c.exit_code = Some(waiters_code);
            c.finished_at = Some(podvm_image::clock::now());
            c.pid = None;
            c.launcher_pid = None;
        }
        Ok(())
    });
    drop(lock);
    0
}
