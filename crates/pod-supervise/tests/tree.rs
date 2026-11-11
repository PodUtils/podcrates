//! The tree of a payload: the kill of each descendant at the end, with its
//! count, the count with no kill and with no `/proc`, the wall bound, and
//! the limit of the file size; the private modes of the state (T-175); a
//! payload that kills its launcher (T-202).
//!
//! No libtest (`harness = false`), as `lifecycle.rs`: the launcher forks
//! from the test process, which has one thread.

#[cfg(target_os = "linux")]
fn main() {
    for tool in ["/bin/sh", "sleep"] {
        if let Err(why) = pod_testkit::host::tool(tool) {
            pod_testkit::host::skip_line(&why);
            return;
        }
    }
    if !std::path::Path::new("/proc/self/limits").exists() {
        pod_testkit::host::skip_line("no /proc on this host");
        return;
    }
    let root = std::env::temp_dir().join(format!("pod-supervise-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    steps::a_grandchild_that_left_and_ignores_sigterm_is_killed(&root);
    steps::with_no_kill_the_grandchild_is_counted(&root);
    steps::with_no_proc_the_count_is_unknown_never_zero(&root);
    steps::the_wall_bound_ends_the_tree(&root);
    steps::no_limit_of_the_file_size_is_set(&root);
    steps::the_state_is_private_under_umask_022(&root);
    steps::a_payload_that_kills_its_launcher_leaves_a_dead_record_and_no_count(&root);
    let _ = std::fs::remove_dir_all(&root);
    println!("tree: 7 steps passed");
}

#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
mod steps {
    use std::os::fd::{BorrowedFd, OwnedFd};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use pod_supervise::end::{Reason, Residual};
    use pod_supervise::launcher::tree::TREE_KILL_MS;
    use pod_supervise::launcher::{Launch, Payload};
    use pod_supervise::table::{Any, Container, State};
    use pod_supervise::{Settings, create, get, remove, set_wall, start, wait};

    fn stdio(launch: &Launch<'_>, n: i64) -> Stdio {
        let (_, host) =
            launch.pass.iter().copied().find(|(child, _)| *child == n).expect("a descriptor for the payload");
        // SAFETY: the launcher holds `host` open for the whole entry.
        let fd: OwnedFd = unsafe { BorrowedFd::borrow_raw(host as i32) }.try_clone_to_owned().expect("dup");
        Stdio::from(fd)
    }

    fn enter(c: &Container<Any>, launch: Launch<'_>) -> Result<Payload, String> {
        let child = Command::new("/bin/sh")
            .arg("-c")
            .arg(c.argv.join(" "))
            .stdin(stdio(&launch, 0))
            .stdout(stdio(&launch, 1))
            .stderr(stdio(&launch, 2))
            .spawn()
            .map_err(|e| format!("spawning /bin/sh: {e}"))?;
        Ok(Payload { pid: child.id() as i64, keep: Box::new(child) })
    }

    fn settings(root: &Path, name: &str) -> Settings {
        Settings::new(root.join(name))
    }

    /// A payload that starts a grandchild in a session of its own (with
    /// `setsid` where the host has it), which ignores SIGTERM and writes its
    /// pid to `pidfile`, and exits when the pid is there.
    fn leaver(pidfile: &Path, then: &str) -> String {
        let f = pidfile.display();
        format!(
            "S=; command -v setsid >/dev/null 2>&1 && S=setsid; \
             $S /bin/sh -c 'echo $$ > {f}.tmp; mv {f}.tmp {f}; trap \"\" TERM; exec sleep 300' & \
             while [ ! -s {f} ]; do sleep 0.05; done; {then}"
        )
    }

    fn run(s: &Settings, name: &str, script: &str, wall_ms: Option<u64>) -> (Container<Any>, Option<i32>, Duration) {
        create(s, Some(name), vec![script.to_string()], Vec::new(), "/".into(), Any::new()).expect("create");
        if wall_ms.is_some() {
            set_wall::<Any>(s, name, wall_ms).expect("set_wall");
        }
        let at = Instant::now();
        start(s, name, enter).expect("start");
        let (_, code) = wait::<Any>(s, name, 30_000).expect("wait");
        let took = at.elapsed();
        let c = get::<Any>(s, name).expect("get");
        assert_eq!(c.state, State::Exited, "{name}: the record after the wait");
        (c, code, took)
    }

    fn pid_in(pidfile: &Path) -> i64 {
        std::fs::read_to_string(pidfile).expect("the pid file").trim().parse().expect("a pid")
    }

    fn alive(pid: i64) -> bool {
        // A zombie is no process of the tree: it has ended.
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => pod_supervise::launcher::tree::parent_of(&stat).is_some() && !stat.contains(") Z "),
            Err(_) => false,
        }
    }

    /// The kill of a process that a step left, so that no step leaves one.
    fn clean(pid: i64) {
        let _ = pod_hostprobe::sys::kill(pid, 9);
    }

    pub fn a_grandchild_that_left_and_ignores_sigterm_is_killed(root: &Path) {
        let s = settings(root, "kill");
        let pidfile: PathBuf = root.join("kill.pid");
        std::fs::create_dir_all(root).unwrap();
        let (c, code, _) = run(&s, "kill", &leaver(&pidfile, "exit 0"), None);
        let grandchild = pid_in(&pidfile);
        let end = c.end.expect("the end of the run");
        let left = alive(grandchild);
        clean(grandchild);
        assert_eq!(code, Some(0));
        assert_eq!(end.reason, Reason::Exit);
        assert_eq!(end.residual_processes, Residual::Count(0), "{end:?}");
        assert!(!left, "the grandchild {grandchild} is alive after the end of the payload");
        assert!(end.max_rss_kib > 0, "{end:?}");
        remove::<Any>(&s, "kill", false).expect("remove");
    }

    /// The positive control of the step above: with the kill off, the
    /// same grandchild stays and the count says so.
    pub fn with_no_kill_the_grandchild_is_counted(root: &Path) {
        let mut s = settings(root, "nokill");
        s.kill_tree = false;
        let pidfile = root.join("nokill.pid");
        let (c, _, _) = run(&s, "nokill", &leaver(&pidfile, "exit 0"), None);
        let grandchild = pid_in(&pidfile);
        let left = alive(grandchild);
        clean(grandchild);
        let end = c.end.expect("the end of the run");
        assert!(left, "the control failed: the grandchild ended with no kill");
        assert!(matches!(end.residual_processes, Residual::Count(n) if n >= 1), "{end:?}");
        remove::<Any>(&s, "nokill", false).expect("remove");
    }

    /// With no way to find the descendants, the count is `unknown`.
    pub fn with_no_proc_the_count_is_unknown_never_zero(root: &Path) {
        let mut s = settings(root, "noproc");
        s.proc = root.join("no-proc-here");
        let pidfile = root.join("noproc.pid");
        let (c, _, _) = run(&s, "noproc", &leaver(&pidfile, "exit 0"), None);
        clean(pid_in(&pidfile));
        let end = c.end.expect("the end of the run");
        assert_eq!(end.residual_processes, Residual::Unknown, "{end:?}");
        remove::<Any>(&s, "noproc", false).expect("remove");
    }

    /// The wall bound kills the payload, which ignores SIGTERM, and its
    /// tree, within the bound and the stated bound of the kill of the tree.
    pub fn the_wall_bound_ends_the_tree(root: &Path) {
        let s = settings(root, "wall");
        let pidfile = root.join("wall.pid");
        let wall = 800;
        let (c, code, took) = run(&s, "wall", &leaver(&pidfile, "trap '' TERM; exec sleep 300"), Some(wall));
        let grandchild = pid_in(&pidfile);
        let left = alive(grandchild);
        clean(grandchild);
        let end = c.end.expect("the end of the run");
        assert_eq!(code, Some(137));
        assert_eq!(end.reason, Reason::Wall, "{end:?}");
        assert_eq!(end.residual_processes, Residual::Count(0), "{end:?}");
        assert!(!left, "the grandchild {grandchild} is alive after the wall bound");
        assert!(end.run_ms >= wall, "{end:?}");
        // The start and the wait add their own time; 5 s is a wide margin
        // for a loaded host.
        let most = Duration::from_millis(wall + TREE_KILL_MS + 5_000);
        assert!(took <= most, "the wall bound took {took:?}, more than {most:?}");
        assert_eq!(c.wall_ms, Some(wall));
        remove::<Any>(&s, "wall", false).expect("remove");
    }

    /// The launcher sets no `RLIMIT_FSIZE`: the payload has the limit of
    /// the process that started it (the .NET runner dies at its start under
    /// a limit below 2 TiB).
    pub fn no_limit_of_the_file_size_is_set(root: &Path) {
        let s = settings(root, "fsize");
        let mine = std::fs::read_to_string("/proc/self/limits")
            .unwrap()
            .lines()
            .find(|l| l.starts_with("Max file size"))
            .expect("the line of the file size")
            .to_string();
        let (c, code, _) = run(&s, "fsize", "grep '^Max file size' /proc/self/limits", None);
        assert_eq!(code, Some(0));
        let log = String::from_utf8_lossy(&pod_supervise::logs::<Any>(&s, "fsize").expect("logs")).to_string();
        assert_eq!(log.trim(), mine.trim(), "the payload has another limit of the file size");
        assert!(c.end.is_some());
        remove::<Any>(&s, "fsize", false).expect("remove");
    }

    /// The table, the staging directory, a container directory and a log
    /// are private under umask 022: the table holds the environment of
    /// each record (PodUtils/podvm#244).
    /// The payload kills its launcher (podrunner's case, T-202): the
    /// grandchild goes to init, out of reach of any count. The record is
    /// `Dead` with no exit code, and no end says a count of 0. The payload
    /// of this `enter` outlives its launcher: no code sets
    /// `PR_SET_PDEATHSIG` on it (T-205).
    pub fn a_payload_that_kills_its_launcher_leaves_a_dead_record_and_no_count(root: &Path) {
        let s = settings(root, "orphan");
        let (pidfile, payfile): (PathBuf, PathBuf) = (root.join("orphan.pid"), root.join("orphan.payload"));
        std::fs::create_dir_all(root).unwrap();
        let p = payfile.display();
        let then = format!("echo $$ > {p}.tmp; mv {p}.tmp {p}; kill -9 $PPID; exec sleep 300");
        create(&s, Some("orphan"), vec![leaver(&pidfile, &then)], Vec::new(), "/".into(), Any::new()).expect("create");
        start(&s, "orphan", enter).expect("start");
        let (c, code) = wait::<Any>(&s, "orphan", 30_000).expect("wait");
        let (grandchild, payload) = (pid_in(&pidfile), pid_in(&payfile));
        clean(grandchild);
        clean(payload);
        assert_eq!(c.state, State::Dead, "the record after the launcher ended");
        assert_eq!(code, None, "a Dead record has no exit code");
        if let Some(end) = &c.end {
            assert_ne!(end.residual_processes, Residual::Count(0), "{end:?}");
        }
        remove::<Any>(&s, "orphan", false).expect("remove");
    }

    pub fn the_state_is_private_under_umask_022(root: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let old = pod_hostprobe::sys::umask(0o022);
        let s = settings(root, "modes");
        let (c, _, _) = run(&s, "modes", "echo secret-of-the-log", None);
        pod_hostprobe::sys::umask(old);
        let mode = |p: &Path| {
            std::fs::metadata(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).permissions().mode() & 0o777
        };
        assert_eq!(mode(&s.root.join(pod_supervise::table::TABLE_FILE)), 0o600, "the table");
        assert_eq!(mode(&s.root.join(pod_supervise::table::STAGING_DIR)), 0o700, "the staging directory");
        assert_eq!(mode(&pod_supervise::table::dir(&s, &c.id)), 0o700, "the container directory");
        assert_eq!(mode(&pod_supervise::table::log_path(&s, &c.id)), 0o600, "the log");
        remove::<Any>(&s, "modes", false).expect("remove");
    }
}
