//! A foreground hold: a payload that the test runs and waits for itself is
//! `Running`, and `kill`, `stop`, `wait`, `list` and `reconcile` work on
//! it with no launcher.
//!
//! No libtest (`harness = false`), for the reason of `lifecycle`: a step
//! calls `start`, which forks. The step of a killed holder runs this binary
//! again as the holder (`hold ROOT NAME`), the step of the shutdown
//! signals as a holder that blocks them (`mask ROOT NAME`), and the step of
//! a forward as a waiter of `pod-enter` with a hold (`waiter ROOT NAME`).

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("hold") {
        steps::be_the_holder(&args[2], &args[3]);
        return;
    }
    if args.get(1).map(String::as_str) == Some("mask") {
        steps::be_the_masked_holder(&args[2], &args[3]);
        return;
    }
    if args.get(1).map(String::as_str) == Some("waiter") {
        steps::be_the_waiter_with_a_hold(&args[2], &args[3]);
        return;
    }
    if let Err(why) = pod_testkit::host::tool("sleep") {
        pod_testkit::host::skip_line(&why);
        return;
    }
    let s = pod_supervise::Settings::new(
        std::env::temp_dir().join(format!("pod-supervise-foreground-{}", std::process::id())),
    );
    let _ = std::fs::remove_dir_all(&s.root);
    if let Some(why) = steps::no_pidfd() {
        pod_testkit::host::skip_line(&why);
        return;
    }
    steps::a_held_payload_is_running_and_reconcile_keeps_it(&s);
    steps::a_kill_reaches_the_payload(&s);
    steps::a_stop_ends_the_payload_within_its_grace(&s);
    steps::a_wait_gets_the_code_of_finish(&s);
    steps::a_drop_with_no_finish_writes_dead(&s);
    steps::a_killed_holder_leaves_a_dead_record(&s);
    steps::a_shutdown_signal_skips_the_thread_of_the_hold(&s);
    steps::a_forwarded_signal_reaches_the_payload_past_the_hold(&s);
    let _ = std::fs::remove_dir_all(&s.root);
    println!("foreground: 8 steps passed");
}

#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
mod steps {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use pod_hostprobe::sys;
    use pod_supervise::launcher::{Launch, Payload, exit_status};
    use pod_supervise::table::{self, Any, Container, State};
    use pod_supervise::{Foreground, Settings, create, foreground, get, kill, list, remove, start, stop, wait};

    /// A reason to skip on a kernel with no `pidfd_open` (before 5.3).
    pub fn no_pidfd() -> Option<String> {
        match sys::pidfd_open(sys::getpid()) {
            Ok(fd) => {
                let _ = sys::close(fd);
                None
            }
            Err(e) => Some(format!("no pidfd_open on this kernel: {}", e.name())),
        }
    }

    fn made(s: &Settings, name: &str) -> Container<Any> {
        create(s, Some(name), vec!["sleep".into(), "30".into()], Vec::new(), "/".into(), Any::new()).expect("create")
    }

    fn payload() -> Child {
        Command::new("sleep").arg("30").spawn().expect("sleep")
    }

    /// The payload, then the hold: the order that the crate asks.
    fn held(s: &Settings, name: &str) -> (Child, Foreground<Any>) {
        made(s, name);
        let child = payload();
        let hold = foreground::<Any>(s, name, child.id() as i64).expect("foreground");
        (child, hold)
    }

    /// The caller's wait: reap the payload, then give its code to the hold.
    fn waiter(mut child: Child, hold: Foreground<Any>) -> std::thread::JoinHandle<i32> {
        std::thread::spawn(move || {
            let status = child.wait().expect("the reap of the payload");
            let code = exit_status(status.into_raw());
            hold.finish(code).expect("finish");
            code
        })
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

    pub fn a_held_payload_is_running_and_reconcile_keeps_it(s: &Settings) {
        let (mut child, hold) = held(s, "up");
        for _ in 0..3 {
            table::reconcile::<Any>(s).expect("reconcile");
        }
        let c = get::<Any>(s, "up").expect("get");
        assert_eq!(c.state, State::Running, "the reconcile took the record of a live hold");
        assert_eq!(c.status(), "Up");
        assert_eq!(c.pid, Some(child.id() as i64));
        assert_eq!(c.launcher_pid, Some(sys::getpid()));
        assert!(c.started_at.is_some() && c.end.is_none());
        let running = list::<Any>(s, false).expect("list");
        assert_eq!(running.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["up"]);
        let e = foreground::<Any>(s, "up", child.id() as i64).err().expect("a second hold");
        assert!(e.text().contains("already running"), "{e}");
        // A fork with the thread of a hold is refused.
        made(s, "forked");
        let e = start(s, "forked", |_: &Container<Any>, _: Launch<'_>| -> Result<Payload, String> {
            Err("no entry".into())
        })
        .expect_err("a start while a hold lives");
        assert!(e.text().contains("holds a foreground payload"), "a start while a hold lives was not refused: {e}");
        child.kill().expect("the kill of the payload");
        let code = exit_status(child.wait().expect("the reap").into_raw());
        hold.finish(code).expect("finish");
        let c = get::<Any>(s, "up").expect("get");
        assert_eq!((c.state, c.exit_code), (State::Exited, Some(137)));
        assert!(c.finished_at.is_some() && c.pid.is_none() && c.launcher_pid.is_none());
        assert!(!table::control_path(s, &c.id).exists(), "finish left the control socket");
        let e = kill::<Any>(s, "up", 15).expect_err("a kill after finish");
        assert!(e.text().contains("exited and not running"), "{e}");
        // With no hold, a start forks again.
        let e = start(s, "forked", |_: &Container<Any>, _: Launch<'_>| -> Result<Payload, String> {
            Err("no entry".into())
        })
        .expect_err("an entry that fails");
        assert!(e.text().contains("no entry"), "{e}");
        remove::<Any>(s, "up", false).expect("remove");
        remove::<Any>(s, "forked", false).expect("remove");
    }

    pub fn a_kill_reaches_the_payload(s: &Settings) {
        let (child, hold) = held(s, "signal");
        let pid = child.id() as i64;
        let reaped = waiter(child, hold);
        kill::<Any>(s, "signal", 15).expect("kill");
        assert_eq!(reaped.join().expect("the waiter"), 143);
        let c = get::<Any>(s, "signal").expect("get");
        assert_eq!((c.state, c.exit_code), (State::Exited, Some(143)));
        assert_eq!(wait::<Any>(s, "signal", 0).expect("wait").1, Some(143));
        assert!(sys::kill(pid, 0).is_err(), "the payload still runs");
        // A hold again: the record has no end of the run before
        // (PodUtils/podvm#343).
        let mut child = payload();
        let hold = foreground::<Any>(s, "signal", child.id() as i64).expect("a hold again");
        let c = get::<Any>(s, "signal").expect("get");
        assert_eq!((c.state, c.pid), (State::Running, Some(child.id() as i64)));
        assert_eq!(c.exit_code, None, "a held record keeps the exit code of the run before");
        assert!(c.finished_at.is_none() && c.end.is_none() && c.noticed.is_none(), "{c:?}");
        child.kill().expect("the kill of the payload");
        hold.finish(exit_status(child.wait().expect("the reap").into_raw())).expect("finish");
        remove::<Any>(s, "signal", false).expect("remove");
    }

    /// `stop` with a grace of 10 s: SIGTERM ends the payload, the waiter
    /// finishes, and `stop` returns with no SIGKILL, well before the grace.
    pub fn a_stop_ends_the_payload_within_its_grace(s: &Settings) {
        let (child, hold) = held(s, "stop");
        let reaped = waiter(child, hold);
        let t = Instant::now();
        let (_, killed) = stop::<Any>(s, "stop", 10_000).expect("stop");
        let took = t.elapsed();
        assert!(!killed, "SIGTERM ended the payload, and stop reported SIGKILL");
        assert!(took < Duration::from_secs(5), "stop took {took:?}");
        assert_eq!(reaped.join().expect("the waiter"), 143);
        let c = get::<Any>(s, "stop").expect("get");
        assert_eq!((c.state, c.exit_code), (State::Exited, Some(143)));
        remove::<Any>(s, "stop", false).expect("remove");
    }

    /// A wait before `finish` gets the bound, not a code; a wait that runs
    /// at `finish` gets the code that `finish` gave, also after the reap.
    pub fn a_wait_gets_the_code_of_finish(s: &Settings) {
        let (mut child, hold) = held(s, "wait");
        assert_eq!(wait::<Any>(s, "wait", 300).expect("wait").1, None, "a wait was answered before finish");
        let root = s.clone();
        let waiting = std::thread::spawn(move || wait::<Any>(&root, "wait", 10_000).expect("wait").1);
        child.kill().expect("the kill of the payload");
        child.wait().expect("the reap");
        // The payload is reaped and the hold lives: a signal now is no
        // failure, and it reaches no other process.
        kill::<Any>(s, "wait", 9).expect("a kill after the reap");
        hold.finish(42).expect("finish");
        assert_eq!(waiting.join().expect("the waiter"), Some(42));
        assert_eq!(get::<Any>(s, "wait").expect("get").exit_code, Some(42));
        remove::<Any>(s, "wait", false).expect("remove");
    }

    pub fn a_drop_with_no_finish_writes_dead(s: &Settings) {
        let (mut child, hold) = held(s, "dropped");
        drop(hold);
        // The table as the drop wrote it, with no reconcile.
        let t = table::read::<Any>(s).expect("read");
        let c = t.containers.iter().find(|c| c.name == "dropped").expect("the record");
        assert_eq!(c.state, State::Dead, "a drop with no finish left the record");
        assert!(c.noticed.is_some() && c.exit_code.is_none());
        assert_eq!((c.pid, c.launcher_pid), (None, None), "a dead record names a pid");
        assert_eq!(wait::<Any>(s, "dropped", 0).expect("wait").1, None);
        let _ = child.kill();
        let _ = child.wait();
        remove::<Any>(s, "dropped", false).expect("remove");
    }

    /// The holder is this binary, run again: it starts a payload, holds it,
    /// says `held PID`, and waits for its stdin to end.
    pub fn be_the_holder(root: &str, name: &str) {
        let s = Settings::new(root);
        let mut child = payload();
        let _hold = foreground::<Any>(&s, name, child.id() as i64).expect("foreground");
        println!("held {}", child.id());
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        // Reached only when the test did not kill this holder.
        let _ = child.kill();
        let _ = child.wait();
    }

    /// A holder that is killed leaves a free lock: the reconcile writes
    /// `Dead`, with no exit code.
    pub fn a_killed_holder_leaves_a_dead_record(s: &Settings) {
        made(s, "orphan");
        let mut holder = Command::new(std::env::current_exe().expect("this binary"))
            .arg("hold")
            .arg(&s.root)
            .arg("orphan")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the holder");
        let mut line = String::new();
        BufReader::new(holder.stdout.take().expect("stdout")).read_line(&mut line).expect("the word of the holder");
        let payload: i64 = line.trim().strip_prefix("held ").and_then(|p| p.parse().ok()).expect("held PID");
        let c = get::<Any>(s, "orphan").expect("get");
        assert_eq!((c.state, c.launcher_pid), (State::Running, Some(holder.id() as i64)));
        holder.kill().expect("the kill of the holder");
        let _ = holder.wait();
        let c = until(s, "orphan", |c| c.state == State::Dead);
        assert!(c.exit_code.is_none() && c.noticed.is_some());
        assert_eq!((c.pid, c.launcher_pid), (None, None), "a dead record names a pid");
        let _ = sys::kill(payload, 9);
        remove::<Any>(s, "orphan", false).expect("remove");
    }

    /// The signals that the waiter of `pod-enter` forwards, as a mask word.
    const SHUTDOWN: u64 = pod_enter::WAIT_FORWARDED;

    /// The holder of a caller that takes the signals of the list itself: it
    /// blocks them in its own thread after the start of the hold, says
    /// `held PID`, and at a line on stdin says `pending MASK` (`ShdPnd` of
    /// the process) and ends.
    pub fn be_the_masked_holder(root: &str, name: &str) {
        let s = Settings::new(root);
        let mut child = payload();
        let hold = foreground::<Any>(&s, name, child.id() as i64).expect("foreground");
        // After the hold: its thread keeps the mask that it took at its spawn.
        let mut old = 0u64;
        sys::sigblock(SHUTDOWN, &mut old).expect("sigblock");
        println!("held {}", child.id());
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        let pending = status.lines().find_map(|l| l.strip_prefix("ShdPnd:")).map(str::trim).unwrap_or("none");
        println!("pending {pending}");
        let _ = child.kill();
        let _ = child.wait();
        let _ = hold.finish(137);
    }

    /// Each signal of the list to a process whose caller thread blocks it
    /// stays pending: the thread of the hold blocks it too, so its default
    /// action does not end the process (PodUtils/podvm#616).
    pub fn a_shutdown_signal_skips_the_thread_of_the_hold(s: &Settings) {
        made(s, "masked");
        let mut holder = Command::new(std::env::current_exe().expect("this binary"))
            .arg("mask")
            .arg(&s.root)
            .arg("masked")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the holder");
        let mut out = BufReader::new(holder.stdout.take().expect("stdout"));
        let mut line = String::new();
        out.read_line(&mut line).expect("the word of the holder");
        let payload: i64 = line.trim().strip_prefix("held ").and_then(|p| p.parse().ok()).expect("held PID");
        for sig in (1..=64u64).filter(|&n| SHUTDOWN & sys::sigbit(n) != 0) {
            let _ = sys::kill(holder.id() as i64, sig as i64);
        }
        let mut stdin = holder.stdin.take().expect("stdin");
        let _ = writeln!(stdin, "check");
        drop(stdin);
        line.clear();
        let _ = out.read_line(&mut line);
        let status = holder.wait().expect("the end of the holder");
        let _ = sys::kill(payload, 9);
        let pending = line.trim().strip_prefix("pending ").and_then(|m| u64::from_str_radix(m, 16).ok());
        assert!(
            pending.is_some() && status.success(),
            "the holder ended at a shutdown signal: {status:?}, it said {line:?}"
        );
        assert_eq!(pending.unwrap_or(0) & SHUTDOWN, SHUTDOWN, "a shutdown signal was not pending: {line:?}");
        remove::<Any>(s, "masked", true).expect("remove");
    }

    /// A payload that ends with 7 on SIGUSR1. It writes "ready" after its
    /// trap, so no signal comes before it.
    fn trapping_payload() -> Child {
        let script = "trap 'kill $! 2>/dev/null; exit 7' USR1; echo ready; sleep 30 & wait $!";
        let mut c = Command::new("sh").args(["-c", script]).stdout(Stdio::piped()).spawn().expect("sh");
        let mut line = String::new();
        BufReader::new(c.stdout.take().expect("stdout")).read_line(&mut line).expect("the word of the payload");
        assert_eq!(line, "ready\n");
        c
    }

    /// The waiter of a caller with a hold: the payload, the hold, then the
    /// wait of `pod-enter` with its forward. The thread of the waiter blocks
    /// the list before the SIGUSR1 to the process, as the wait does, so the
    /// signal is for the process and not for this thread. It says
    /// `payload PID`, then `code N`.
    pub fn be_the_waiter_with_a_hold(root: &str, name: &str) {
        let s = Settings::new(root);
        let mut child = trapping_payload();
        let pid = child.id() as i64;
        println!("payload {pid}");
        let hold = foreground::<Any>(&s, name, pid).expect("foreground");
        let mut old = 0u64;
        sys::sigblock(SHUTDOWN, &mut old).expect("sigblock");
        sys::kill(sys::getpid(), sys::SIGUSR1 as i64).expect("the signal to this process");
        let mut err = Vec::new();
        let code = pod_enter::Child::of(pid).wait_forwarding(&mut err).expect("the wait");
        // The wait above reaped the payload: this one only ends the handle.
        let _ = child.try_wait();
        hold.finish(code).expect("finish");
        println!("code {code}");
        eprint!("{}", String::from_utf8_lossy(&err));
    }

    /// A SIGUSR1 to a waiter with a hold reaches the payload, the waiter
    /// gives the code of the payload, and the record holds it: the thread of
    /// the hold does not take the signal and end the process
    /// (PodUtils/podvm#616).
    pub fn a_forwarded_signal_reaches_the_payload_past_the_hold(s: &Settings) {
        made(s, "forwarded");
        let out = Command::new(std::env::current_exe().expect("this binary"))
            .arg("waiter")
            .arg(&s.root)
            .arg("forwarded")
            .stdin(Stdio::null())
            .output()
            .expect("the waiter");
        let said = String::from_utf8_lossy(&out.stdout);
        let why = String::from_utf8_lossy(&out.stderr);
        if !out.status.success() {
            // The waiter ended, and its payload is no child of the test.
            let payload = said.lines().find_map(|l| l.strip_prefix("payload ")?.parse::<i64>().ok());
            if let Some(p) = payload {
                let _ = sys::kill(p, 9);
            }
        }
        assert!(
            out.status.success() && said.contains("code 7"),
            "the waiter with a hold ended at a forwarded signal: {:?}, it said {said:?}, {why:?}",
            out.status
        );
        let c = get::<Any>(s, "forwarded").expect("get");
        assert_eq!((c.state, c.exit_code), (State::Exited, Some(7)), "{c:?}");
        remove::<Any>(s, "forwarded", true).expect("remove");
    }
}
