//! The lifecycle of real payloads: start, the log, a stop with no grace, a
//! payload that exits by itself, a signal, and a launcher that was killed.
//!
//! The launcher forks from the test process. libtest runs each test in a
//! thread, and a fork from a process with more than one thread can stop in
//! a lock that another thread held. So this binary has no libtest
//! (`harness = false`): one thread runs each step in order.

#[cfg(target_os = "linux")]
fn main() {
    if let Err(why) = pod_testkit::host::tool("/bin/sh") {
        pod_testkit::host::skip_line(&why);
        return;
    }
    if let Err(why) = pod_testkit::host::tool("sleep") {
        pod_testkit::host::skip_line(&why);
        return;
    }
    let s = pod_supervise::Settings::new(
        std::env::temp_dir().join(format!("pod-supervise-lifecycle-{}", std::process::id())),
    );
    let _ = std::fs::remove_dir_all(&s.root);
    steps::a_stop_with_no_grace_kills_the_payload(&s);
    steps::a_payload_that_exits_gives_its_code(&s);
    steps::a_signal_reaches_the_payload(&s);
    steps::a_killed_launcher_leaves_a_dead_record(&s);
    steps::an_entry_that_fails_gives_its_text(&s);
    let _ = std::fs::remove_dir_all(&s.root);
    println!("lifecycle: 5 steps passed");
}

#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
mod steps {
    use std::os::fd::{BorrowedFd, OwnedFd};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use pod_supervise::launcher::{Launch, Payload};
    use pod_supervise::table::{Any, Container, State};
    use pod_supervise::{Settings, create, get, kill, logs, remove, start, stop, wait};

    /// The descriptor `n` of the payload, as a copy of the descriptor that
    /// the launcher passes.
    fn stdio(launch: &Launch<'_>, n: i64) -> Stdio {
        let (_, host) =
            launch.pass.iter().copied().find(|(child, _)| *child == n).expect("a descriptor for the payload");
        // SAFETY: the launcher holds `host` open for the whole entry.
        let fd: OwnedFd = unsafe { BorrowedFd::borrow_raw(host as i32) }.try_clone_to_owned().expect("dup");
        Stdio::from(fd)
    }

    /// The entry: run the argv of the record with `/bin/sh -c`.
    fn enter(c: &Container<Any>, launch: Launch<'_>) -> Result<Payload, String> {
        let _ = writeln!(launch.log, "entry: {}", c.name);
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

    fn made(s: &Settings, name: &str, script: &str) -> Container<Any> {
        let mut of = Any::new();
        of.insert("rung".into(), "test".into());
        create(s, Some(name), vec![script.to_string()], Vec::new(), "/".into(), of).expect("create")
    }

    /// Wait, with a bound, until `test` accepts the record.
    fn until(s: &Settings, name: &str, test: impl Fn(&Container<Any>) -> bool) -> Container<Any> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let c = get::<Any>(s, name).expect("get");
            if test(&c) {
                return c;
            }
            assert!(Instant::now() < deadline, "{name}: no change in 10 s: {:?}, {:?}", c.state, c.exit_code);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn log_until(s: &Settings, name: &str, text: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let log = String::from_utf8_lossy(&logs::<Any>(s, name).expect("logs")).to_string();
            if log.contains(text) {
                return log;
            }
            assert!(Instant::now() < deadline, "{name}: the log has no {text:?} in 10 s: {log:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// `stop -t 0`: SIGTERM, a bound of zero, then SIGKILL; the stop
    /// reports the kill. The payload ignores SIGTERM, so that only SIGKILL
    /// ends it: a payload that ends at SIGTERM can end before the wait, and
    /// the stop then reports no kill.
    pub fn a_stop_with_no_grace_kills_the_payload(s: &Settings) {
        made(s, "nograce", "trap '' TERM; echo started; while :; do sleep 1; done");
        let c = start(s, "nograce", enter).expect("start");
        assert_eq!(c.state, State::Running);
        assert!(c.pid.is_some() && c.launcher_pid.is_some());
        assert_eq!(c.of.get("rung").and_then(|v| v.as_str()), Some("test"), "the fields of the caller went");
        assert_eq!(get::<Any>(s, "nograce").unwrap().state, State::Running);
        let log = log_until(s, "nograce", "started");
        assert!(log.contains("entry: nograce"), "{log}");
        let (_, killed) = stop::<Any>(s, "nograce", 0).expect("stop");
        assert!(killed, "a stop with no grace did not report the kill");
        let c = until(s, "nograce", |c| c.state == State::Exited);
        assert_eq!(c.exit_code, Some(137));
        assert!(c.finished_at.is_some() && c.pid.is_none() && c.launcher_pid.is_none());
        remove::<Any>(s, "nograce", false).expect("remove");
    }

    pub fn a_payload_that_exits_gives_its_code(s: &Settings) {
        made(s, "exits", "echo out; sleep 1; exit 3");
        start(s, "exits", enter).expect("start");
        let (_, code) = wait::<Any>(s, "exits", 10_000).expect("wait");
        let c = until(s, "exits", |c| c.state == State::Exited);
        assert_eq!(c.exit_code, Some(3));
        if let Some(code) = code {
            assert_eq!(code, 3);
        }
        assert!(log_until(s, "exits", "out").contains("out\n"));
        // An exited container answers a wait with its code at once.
        assert_eq!(wait::<Any>(s, "exits", 0).unwrap().1, Some(3));
        let e = start(s, "nosuch", enter).expect_err("a start of no container");
        assert_eq!(e.class, pod_core::Class::NotFound);
        remove::<Any>(s, "exits", false).expect("remove");
    }

    pub fn a_signal_reaches_the_payload(s: &Settings) {
        made(s, "signal", "exec sleep 30");
        start(s, "signal", enter).expect("start");
        let e = start(s, "signal", enter).expect_err("a second start");
        assert!(e.text().contains("already running"), "{e}");
        let e = remove::<Any>(s, "signal", false).expect_err("a remove of a running container");
        assert!(e.text().contains("Stop it first"), "{e}");
        kill::<Any>(s, "signal", 15).expect("kill");
        let c = until(s, "signal", |c| c.state == State::Exited);
        assert_eq!(c.exit_code, Some(143));
        remove::<Any>(s, "signal", false).expect("remove");
    }

    /// A launcher that was killed leaves no exit code: the reconcile finds
    /// its lock free and writes `Dead`, with the time of the notice.
    pub fn a_killed_launcher_leaves_a_dead_record(s: &Settings) {
        made(s, "orphan", "exec sleep 30");
        let c = start(s, "orphan", enter).expect("start");
        let (launcher, payload) = (c.launcher_pid.unwrap(), c.pid.unwrap());
        let _ = pod_hostprobe::sys::kill(launcher, 9);
        let c = until(s, "orphan", |c| c.state == State::Dead);
        let _ = pod_hostprobe::sys::kill(payload, 9);
        assert!(c.exit_code.is_none() && c.noticed.is_some());
        assert!(c.status().contains("not watching"), "{}", c.status());
        let (_, code) = wait::<Any>(s, "orphan", 0).unwrap();
        assert_eq!(code, None, "a dead record made up an exit code");
        remove::<Any>(s, "orphan", true).expect("remove");
    }

    pub fn an_entry_that_fails_gives_its_text(s: &Settings) {
        made(s, "refused", "true");
        let e = start(s, "refused", |_: &Container<Any>, _: Launch<'_>| Err("no entry for this test".to_string()))
            .expect_err("a start whose entry failed");
        assert!(e.text().contains("no entry for this test"), "{e}");
        let c = get::<Any>(s, "refused").unwrap();
        assert_eq!(c.state, State::Created, "a failed entry changed the record");
        remove::<Any>(s, "refused", false).expect("remove");
    }
}
