//! The case of T-1635: the child of a join holds the descriptors of the
//! caller until its `execve`, and must not be dumpable after its join.
//!
//! A run with no PID namespace waits; its payload sees each process of the
//! host in its `/proc`. A thread of this process finds the child of a join
//! of that run, waits until the child is in the user namespace of the run
//! (or this process cannot read it: the child is not dumpable), stops it
//! with SIGSTOP, and checks that it has not run its `execve` yet. Then the
//! payload of the run walks to the host file `../marker` from each
//! descriptor and from the working directory of the child (`probe::peek`):
//! no walk may reach it. A try where the stop comes after the `execve` does
//! not count; the case tries again, and fails where no try counts.

use std::os::unix::fs::MetadataExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use pod_enter::{RootDir, spawn_joined, spawn_namespace};
use pod_hostprobe::sys as k;

use super::run::{Running, Tree, plan, start, tree};

/// The tries of one case.
const TRIES: usize = 40;

/// The name of the payload after its `execve` (`image::GUEST_EXE`).
const PAYLOAD_NAME: &str = "confine";

pub fn join_not_dumpable(pid: bool) -> Result<(), String> {
    let t = tree("dump");
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let ops = ["ns:user".to_string(), "peek:../marker".to_string()];
    let mut err = Vec::new();
    let run = start(&mut plan(&t, &ops, pid, false), |p| spawn_namespace(&root, p, &mut err))?;
    let user = run.get("ns:user")? as u64;
    let mut out = Err(format!("no try of {TRIES} stopped the child of the join between its join and its exec"));
    for _ in 0..TRIES {
        match one_try(&t, &root, &run, user) {
            Ok(None) => continue,
            Ok(Some(1)) => {
                out = Err("the payload of the run walked from a descriptor of the join to a host file".to_string());
                break;
            }
            Ok(Some(_)) => {
                out = Ok(());
                break;
            }
            Err(e) => {
                out = Err(e);
                break;
            }
        }
    }
    let code = run.end()?;
    out.and(if code == 0 { Ok(()) } else { Err(format!("the run gave {code}")) })
}

/// One join of `run`. `Some` with the answer of the payload of the run
/// where the thread stopped the child of the join before its `execve`.
fn one_try(t: &Tree, root: &RootDir, run: &Running, user: u64) -> Result<Option<i64>, String> {
    let known = children();
    let pidfd = k::pidfd_open(run.child.pid).map_err(|e| format!("pidfd_open: {e:?}"))?;
    let done = AtomicBool::new(false);
    let (caught, joined) = std::thread::scope(|s| {
        let catcher = s.spawn(|| catch(&known, user, &done, run));
        let mut err = Vec::new();
        let joined = start(&mut plan(t, &[], false, false), |p| spawn_joined(root, p, pidfd, &mut err));
        done.store(true, Ordering::SeqCst);
        (catcher.join().unwrap_or(None), joined)
    });
    let _ = k::close(pidfd);
    joined?.end()?;
    Ok(caught.and_then(|answer| answer.rsplit_once('=').and_then(|(_, r)| r.parse().ok())))
}

/// The children of each thread of this process.
fn children() -> Vec<i64> {
    let tasks = std::fs::read_dir("/proc/self/task").map(|d| d.flatten().collect::<Vec<_>>()).unwrap_or_default();
    let text: String = tasks.iter().filter_map(|t| std::fs::read_to_string(t.path().join("children")).ok()).collect();
    text.split_whitespace().filter_map(|w| w.parse().ok()).collect()
}

/// The state and the name of the process `pid` (`/proc/PID/stat`).
fn state(pid: i64) -> Option<(char, String)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (open, close) = (text.find('(')?, text.rfind(')')?);
    Some((text[close + 2..].chars().next()?, text[open + 1..close].to_string()))
}

/// Find the new child, wait for its join, stop it, and ask the payload of
/// `run` to walk from it. The answer, where the stop came before the exec.
fn catch(known: &[i64], user: u64, done: &AtomicBool, run: &Running) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let late = || Instant::now() > deadline;
    let pid = loop {
        if let Some(c) = children().into_iter().find(|c| !known.contains(c)) {
            break c;
        }
        if done.load(Ordering::SeqCst) || late() {
            return None;
        }
        std::thread::yield_now();
    };
    // In the user namespace of the run, or not dumpable: this process then
    // cannot read the link (`EACCES`), unless it holds `CAP_SYS_PTRACE`.
    loop {
        match std::fs::metadata(format!("/proc/{pid}/ns/user")) {
            Ok(m) if m.ino() == user => break,
            Ok(_) if !late() => std::thread::yield_now(),
            Err(e) if e.raw_os_error() == Some(k::EACCES.0) => break,
            _ => return None,
        }
    }
    let _ = k::kill(pid, k::SIGSTOP as i64);
    let stopped = loop {
        match state(pid) {
            Some(('T' | 't', name)) => break Some(name),
            Some(_) if !late() => std::thread::yield_now(),
            _ => break None,
        }
    };
    let answer = stopped.filter(|name| name != PAYLOAD_NAME).and_then(|_| run.ask(&pid.to_string()).ok());
    let _ = k::kill(pid, k::SIGCONT as i64);
    answer
}
