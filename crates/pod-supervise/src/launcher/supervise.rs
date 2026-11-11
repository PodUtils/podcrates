use super::*;

use crate::end::End;
use crate::lock::Lock;

/// The launcher. It runs in the forked child and never returns to the code
/// of the caller: its value is the exit code of the process.
pub(super) fn supervise<X, F>(s: &Settings, container: &Container<X>, enter: F, log: std::fs::File, ready_w: i64) -> i32
where
    X: Clone + Serialize + DeserializeOwned,
    F: FnOnce(&Container<X>, Launch<'_>) -> std::result::Result<Payload, String>,
{
    let say = |msg: &str| {
        let _ = sys::write(ready_w, msg.as_bytes());
    };

    // A session of its own, so that the launcher outlives the shell that
    // started it and has no controlling terminal.
    let _ = sys::setsid();

    // The child subreaper of the tree, before the payload exists: each
    // descendant whose parent ends comes here, not to init. A host that
    // refuses it gives the count `unknown` at the end.
    let subreaper = sys::set_child_subreaper().is_ok();

    // Its own stdio goes to /dev/null. A `clone` copies each descriptor and
    // this process never execs, so without this it holds the stdin, stdout
    // and stderr of the caller for the whole life of the container. The
    // caller then cannot read its own stdout to the end: `id=$(tool run -d
    // ...)` reads the pipe to its end, which comes when the last holder
    // closes it, so the command took as long as the payload ran.
    //
    // A launcher that cannot open /dev/null refuses: one that cannot let go
    // of the stdio of the caller makes it wait with no message.
    match sys::CBuf::new("/dev/null").ok_or(sys::Errno(2)).and_then(|c| {
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
                "err the launcher could not put its own stdio on /dev/null: {} ({}). It would hold the \
                 stdout of the caller open for the whole life of the container",
                e.name(),
                e.0
            ));
            return 1;
        }
    }

    // The lock before each write: this lock is the statement "a launcher is
    // alive for this container", and a reconcile reads it with a try to
    // take it. A lock taken after the write of `running` leaves a time in
    // which the table says running and nothing holds the lock.
    let lock_path = table::lock_path(s, &container.id);
    let lock = match Lock::try_exclusive(&lock_path) {
        Ok(Some(l)) => l,
        Ok(None) => {
            say(&format!("err another launcher already holds {}", lock_path.display()));
            return 1;
        }
        Err(e) => {
            say(&format!("err {e}"));
            return 1;
        }
    };

    // The control socket. A stale one of a launcher that was killed goes
    // first: nothing listens on it, and it would make `bind` fail for a
    // container that starts again.
    let ctl_path = table::control_path(s, &container.id);
    let _ = std::fs::remove_file(&ctl_path);
    let (_ctl_dir, bind_path) = match table::control_via_fd(s, &container.id) {
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

    // The stdio of the payload, opened before a root changes. Two copies of
    // the log, because the child closes each descriptor of the host after
    // its `dup2`.
    let null = std::fs::OpenOptions::new().read(true).open("/dev/null").ok();
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

    let mut sink = std::io::sink();
    let mut entry_log = err_sink.as_ref().ok().and_then(|f| f.try_clone().ok());
    let log_for_entry: &mut dyn Write = match entry_log.as_mut() {
        Some(f) => f,
        None => &mut sink,
    };
    let payload = match enter(container, Launch { pass, log: log_for_entry }) {
        Ok(p) => p,
        Err(e) => {
            say(&format!("err {e}"));
            return 1;
        }
    };
    drop(entry_log);
    drop(out);
    drop(err_sink);
    drop(null);
    let pid = payload.pid;
    let started = Instant::now();
    let wall = container.wall_ms.map(|ms| started + std::time::Duration::from_millis(ms));

    // A pidfd from the moment the child exists, and each wait below is on
    // it, not on a pid.
    let pidfd = match sys::pidfd_open(pid) {
        Ok(fd) => fd,
        Err(e) => {
            // Without a pidfd the launcher would fall back to a pid. The
            // payload is killed, not supervised badly.
            let _ = sys::kill(pid, 9);
            let mut st = 0i32;
            let _ = sys::wait4(pid, &mut st);
            say(&format!(
                "err pidfd_open on the payload failed: {} ({}). The launcher does not supervise a container \
                 by pid",
                e.name(),
                e.0
            ));
            return 1;
        }
    };

    let launcher_pid = sys::getpid();
    if let Err(e) = record::<X>(s, &container.id, |c| {
        c.state = State::Running;
        c.pid = Some(pid);
        c.launcher_pid = Some(launcher_pid);
        c.started_at = Some(table::now());
        c.end = None;
    }) {
        say(&format!("err recording the container as running: {e}"));
        let _ = sys::kill(pid, 9);
        return 1;
    }

    // The payload runs, and the caller goes on from here.
    say(&format!("ok {launcher_pid} {pid}"));
    let _ = sys::close(ready_w);

    let served = serve(&listener, pidfd, pid, s.failed_code, wall);
    let _ = sys::close(pidfd);
    let run_ms = started.elapsed().as_millis() as u64;

    // The tree, then its use: the use counts each process that the kill
    // reaped. The record holds the end before a waiter gets the code.
    let residual = tree::end_tree(&s.proc, launcher_pid, subreaper, s.kill_tree);
    let usage = sys::rusage_children().unwrap_or_default();
    let end = End {
        reason: served.reason,
        run_ms,
        user_ms: usage.user_us / 1000,
        system_ms: usage.system_us / 1000,
        max_rss_kib: usage.max_rss_kib,
        residual_processes: residual,
    };
    let _ = record::<X>(s, &container.id, |c| {
        ended(c, served.code);
        c.end = Some(end);
    });
    let _ = std::fs::remove_file(&ctl_path);
    for mut w in served.waiters {
        let _ = writeln!(w, "exit {}", served.code);
    }
    drop(payload.keep);
    drop(lock);
    0
}
