//! The PID namespace of the namespace rung (T-806, PodUtils/podvm#457), as
//! root (a PID namespace in the user namespace of the caller) and as a
//! caller with no capabilities (a PID namespace that the user namespace of
//! the rung owns). The payload is pid 2 under the init of the entry, its
//! `/proc` is a fresh proc of the namespace, it lists the processes of the
//! run alone, and a pid of the host is not there. A SIGTERM to the init
//! ends the payload; the end of the init ends each process of the
//! namespace; an exec that joins the run lands in its namespace.

use pod_enter::{Bounded, Child};

use super::*;

fn with_pid(script: &str) -> Plan {
    let mut p = plan(script, None);
    p.pid_namespace = true;
    p
}

/// As root: the test `test` as root first, then as uid 65534. As another
/// user: as this user.
pub fn as_root_then_nobody(test: &str, tag: &str, body: fn(&Path)) {
    if std::env::var_os(IMAGE).is_none() && nobody::is_root() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        let bb = match busybox() {
            Ok(bb) => bb,
            Err(why) => return skip_line(&format!("no static busybox: {why}")),
        };
        let root = image(&bb, &format!("{tag}-root"));
        body(&root);
        let _ = fs::remove_dir_all(&root);
    }
    as_nobody(test, tag, body);
}

/// The caller as root, or with no capabilities and a user namespace.
fn caller() -> bool {
    nobody::is_root() || no_capabilities()
}

/// Each host pid of the processes under `pid`, from the `children` files
/// of `/proc`; `None` where the kernel has no such file.
fn descendants(pid: i64) -> Option<Vec<i64>> {
    let mut out = Vec::new();
    let mut next = vec![pid];
    while let Some(p) = next.pop() {
        let text = fs::read_to_string(format!("/proc/{p}/task/{p}/children")).ok()?;
        for c in text.split_whitespace().filter_map(|c| c.parse::<i64>().ok()) {
            out.push(c);
            next.push(c);
        }
    }
    Some(out)
}

/// Wait until the payload of `run` has the name `comm`.
fn wait_for(run: &Child, comm: &str) {
    for _ in 0..200 {
        let kids = descendants(run.pid).unwrap_or_default();
        if kids.iter().any(|k| fs::read_to_string(format!("/proc/{k}/comm")).is_ok_and(|c| c.trim_end() == comm)) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// The payload in its PID namespace: pid 2, a fresh proc, no host pid.
pub fn hides_the_host(root: &Path) {
    if !caller() {
        return;
    }
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let host = std::process::id();
    let script = format!(
        "test $$ = 2 || exit 31; read p rest < /proc/self/stat; test \"$p\" = 2 || exit 32; \
         test \"$(cat /proc/1/cmdline | wc -c)\" -gt 0 || exit 33; \
         kill -0 {host} 2>/tmp/kill.err && exit 34; test -d /proc/{host} && exit 35; \
         n=$(ls /proc | grep -c '^[0-9][0-9]*$'); test \"$n\" -le 6 || exit 36; \
         grep -q ' /proc [a-z]*,nosuid,nodev,noexec' /proc/self/mountinfo || exit 37"
    );
    let mut err = Vec::new();
    let code = run_namespace(&dir, &with_pid(&script), &mut err);
    let err = String::from_utf8_lossy(&err);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the payload in its PID namespace: {err}");
    assert!(!err.contains("entered chroot instead"), "{err}");
}

/// A SIGTERM to the init ends the payload, which has no handler: the init
/// gives it the signal, and exits 143. Then the end of the init ends a
/// process that the payload left in the background.
pub fn ends_with_its_init(root: &Path) {
    if !caller() {
        return;
    }
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let mut err = Vec::new();
    let run = spawn_namespace(&dir, &with_pid("exec sleep 60"), &mut err).unwrap();
    wait_for(&run, "sleep");
    sys::kill(run.pid, sys::SIGTERM as i64).unwrap();
    let ended = run.wait_bounded(10_000).map_err(|e| e.to_string());
    assert_eq!(ended, Ok(Bounded::Exited(143)), "{}", String::from_utf8_lossy(&err));

    let run = spawn_namespace(&dir, &with_pid("sleep 61 & exec sleep 62"), &mut err).unwrap();
    wait_for(&run, "sleep");
    std::thread::sleep(std::time::Duration::from_millis(200));
    let Some(kids) = descendants(run.pid) else {
        let _ = sys::kill(run.pid, 9);
        let _ = run.wait();
        return skip_line("the kernel has no /proc/PID/task/TID/children");
    };
    assert!(kids.len() >= 2, "the payload and its background process: {kids:?}");
    sys::kill(run.pid, 9).unwrap();
    assert_eq!(run.wait_bounded(10_000).map_err(|e| e.to_string()), Ok(Bounded::Exited(137)));
    for k in kids {
        assert!(!Path::new(&format!("/proc/{k}")).exists(), "process {k} outlived the init of its namespace");
    }
}

/// An exec that joins a run in a PID namespace lands in it: its `/proc`
/// is the proc of the run, it sees the payload of the run as pid 2, and
/// its own pid is a pid of that namespace.
pub fn a_join_lands_in_it(root: &Path) {
    if !caller() {
        return;
    }
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let mut err = Vec::new();
    let run = spawn_namespace(&dir, &with_pid("touch /tmp/shared && exec sleep 60"), &mut err).unwrap();
    wait_for(&run, "sleep");
    let pidfd = sys::pidfd_open(run.pid).unwrap();
    let script = "test -e /tmp/shared || exit 41; read p rest < /proc/self/stat || exit 42; \
         test \"$p\" = $$ || exit 43; test \"$p\" -gt 2 || exit 44; \
         test \"$(cat /proc/2/comm)\" = sleep || exit 45";
    let mut joined = Vec::new();
    let code = spawn_joined(&dir, &plan(script, None), pidfd, &mut joined).and_then(|c| c.wait());
    let _ = sys::close(pidfd);
    let _ = sys::kill(run.pid, 9);
    let _ = run.wait();
    let (err, joined) = (String::from_utf8_lossy(&err), String::from_utf8_lossy(&joined));
    assert!(!err.contains("entered chroot instead"), "the run fell back: {err}");
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the joined exec: {joined}");
    assert!(!joined.contains("does not share the mounts"), "{joined}");
}

/// Whether the read end `r` gets its end of file within 5 s, on a thread
/// of its own.
fn ends_soon(mut r: std::io::PipeReader) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = tx.send(r.read_to_end(&mut buf).is_ok());
    });
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or(false)
}

/// The init and the relay of a join keep no descriptor of the caller: a
/// caller that waits for the end of file of a pipe with close-on-exec, as
/// the launcher of `pod-supervise` waits for its ready pipe, returns while
/// the payload runs (PodUtils/podvm#460).
pub fn keeps_no_descriptor(root: &Path) {
    if !caller() {
        return;
    }
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let mut err = Vec::new();
    let (r, w) = std::io::pipe().unwrap();
    let started = std::time::Instant::now();
    let run = spawn_namespace(&dir, &with_pid("exec sleep 60"), &mut err).unwrap();
    // The start returns at the exec of the payload: an init that held the
    // readiness pipe would hold it until the end of the payload.
    let start_waited = started.elapsed();
    drop(w);
    let ended = ends_soon(r);
    let alive = running(run.pid);
    wait_for(&run, "sleep");
    let pidfd = sys::pidfd_open(run.pid).unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let mut joined = Vec::new();
    let exec = spawn_joined(&dir, &plan("exec sleep 60", None), pidfd, &mut joined).unwrap();
    drop(w);
    let join_ended = ends_soon(r);
    let join_alive = running(exec.pid);
    let _ = sys::close(pidfd);
    let _ = sys::kill(exec.pid, 9);
    let _ = exec.wait();
    let _ = sys::kill(run.pid, 9);
    let _ = run.wait();
    let (err, joined) = (String::from_utf8_lossy(&err), String::from_utf8_lossy(&joined));
    assert!(
        start_waited < std::time::Duration::from_secs(20),
        "the init held a pipe of the caller: the start waited {start_waited:?}"
    );
    assert!(ended && alive, "the init held a pipe of the caller: end of file {ended}, payload running {alive}: {err}");
    assert!(!joined.contains("does not share the mounts"), "{joined}");
    assert!(join_ended && join_alive, "the relay of the join held a pipe of the caller: {joined}");
}

/// Whether the process `pid`, a child of this one, still runs: its pidfd
/// is not readable.
fn running(pid: i64) -> bool {
    let Ok(fd) = sys::pidfd_open(pid) else { return false };
    let mut fds = [sys::PollFd { fd: fd as i32, events: sys::POLLIN, revents: 0 }];
    let ended = !matches!(sys::ppoll(&mut fds, 0), Ok(0));
    let _ = sys::close(fd);
    !ended
}

/// The checks of the process `pid` (a word of the shell) that shows
/// `title`: its command line, its name, and no byte of its environment.
fn shows(pid: &str, title: &str) -> String {
    format!(
        "test \"$(tr '\\000' @ < /proc/{pid}/cmdline)\" = {title}@ || exit 51; \
         test \"$(cat /proc/{pid}/comm)\" = {title} || exit 52; \
         test \"$(cat /proc/{pid}/environ 2>/tmp/environ.err | wc -c)\" = 0 || exit 53; "
    )
}

/// The init and the relay of a join show the title of the plan, or the
/// product of the names, and no argument or variable of the caller
/// (T-1112, PodUtils/podvm#646); the payload keeps its own environment.
/// The relay is not in the PID namespace of the run (`setns` moves its
/// children only): the caller reads its `/proc/PID`. The init and the
/// relay are not dumpable (T-1113), so as a caller with no capabilities
/// the payload and the caller cannot read the environment or the memory
/// of the init, its files in `/proc/PID` belong to root, and a join of the
/// run still lands in it. As root, the bytes of the old command line and
/// environment in the memory of the init are NUL.
pub fn shows_a_title(root: &Path) {
    if !caller() {
        return;
    }
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    // A payload of a user namespace of the rung holds no capability over
    // the memory of the init: its environment is refused. The image has no
    // `/dev`: the output goes to `/tmp`.
    let refused = if nobody::is_root() { "" } else { "cat /proc/1/environ > /tmp/environ 2>&1 && exit 55; " };
    for (title, shown) in [(None, Names::DEFAULT.product), (Some("unit-init"), "unit-init")] {
        let mut p = with_pid(&format!("{}{refused}test -n \"$PATH\" || exit 54", shows("1", shown)));
        p.relay_title = title.map(str::to_string);
        let mut err = Vec::new();
        let code = run_namespace(&dir, &p, &mut err);
        let err = String::from_utf8_lossy(&err);
        assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the init shows {shown}: {err}");
    }
    let mut err = Vec::new();
    let run = spawn_namespace(&dir, &with_pid("exec sleep 60"), &mut err).unwrap();
    wait_for(&run, "sleep");
    let init = memory_of(run.pid);
    let pidfd = sys::pidfd_open(run.pid).unwrap();
    let mut p = plan("exec sleep 60", None);
    p.relay_title = Some("unit-exec".to_string());
    let mut joined = Vec::new();
    let exec = spawn_joined(&dir, &p, pidfd, &mut joined).unwrap();
    let at = |f: &str| format!("/proc/{}/{f}", exec.pid);
    let cmdline = fs::read(at("cmdline")).unwrap_or_default();
    let comm = fs::read_to_string(at("comm")).unwrap_or_default();
    let environ = fs::read(at("environ"));
    let owner = std::os::unix::fs::MetadataExt::uid(&fs::metadata(at("environ")).unwrap());
    let _ = sys::close(pidfd);
    let _ = sys::kill(exec.pid, 9);
    let _ = exec.wait();
    let _ = sys::kill(run.pid, 9);
    let _ = run.wait();
    let joined = String::from_utf8_lossy(&joined);
    match init {
        Init::Blank => {}
        Init::Shows(n) => panic!("the init keeps {n} bytes of the command line and the environment of the caller"),
        Init::Dumpable(why) => panic!("the init is dumpable: {why}"),
    }
    assert!(!joined.contains("does not share the mounts"), "{joined}");
    assert_eq!(String::from_utf8_lossy(&cmdline), "unit-exec\0", "the relay shows unit-exec: {joined}");
    assert_eq!(comm, "unit-exec\n", "the name of the relay");
    if nobody::is_root() {
        assert_eq!(environ.map_err(|e| e.to_string()), Ok(Vec::new()), "the environment of the relay");
    } else {
        assert!(environ.is_err(), "the relay is dumpable: its environment reads {environ:?}");
        assert_eq!(owner, 0, "the relay is dumpable: /proc/{}/environ belongs to {owner}", exec.pid);
    }
}

/// What the caller reads of the memory of an init.
enum Init {
    /// As root: NUL over the old command line and environment. As another
    /// user: the environment and the memory are refused, and the files of
    /// `/proc/PID` belong to root.
    Blank,
    /// As root: the count of bytes that are not NUL.
    Shows(usize),
    /// As another user: what read.
    Dumpable(String),
}

/// The memory of the init `pid`, a fork of this process: the bytes of the
/// command line and of the environment of this process, at the addresses
/// of fields 48 to 51 of `/proc/self/stat`.
fn memory_of(pid: i64) -> Init {
    use std::os::unix::fs::{FileExt, MetadataExt};
    let at = |f: &str| format!("/proc/{pid}/{f}");
    if !nobody::is_root() {
        let environ = fs::read(at("environ"));
        let mem = fs::File::open(at("mem"));
        let owner = fs::metadata(at("environ")).map(|m| m.uid());
        return match (environ, mem, owner) {
            (Err(_), Err(_), Ok(0)) => Init::Blank,
            (e, m, o) => Init::Dumpable(format!("environ {:?}, mem {:?}, owner {o:?}", e.map(|v| v.len()), m.is_ok())),
        };
    }
    let stat = fs::read_to_string("/proc/self/stat").unwrap();
    let f: Vec<u64> = stat[stat.rfind(')').unwrap() + 1..].split_whitespace().map(|w| w.parse().unwrap_or(0)).collect();
    // Field 3 is the first after the name.
    let field = |n: usize| f[n - 3];
    let mem = fs::File::open(at("mem")).unwrap();
    let mut shown = 0;
    for (start, end) in [(field(48), field(49)), (field(50), field(51))] {
        let mut buf = vec![0u8; (end - start) as usize];
        mem.read_exact_at(&mut buf, start).unwrap();
        shown += buf.iter().filter(|&&b| b != 0).count();
    }
    if shown == 0 { Init::Blank } else { Init::Shows(shown) }
}
