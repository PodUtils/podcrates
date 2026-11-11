//! The tree of a payload.
//!
//! The launcher is the child subreaper of its descendants: a process of the
//! tree whose parent ends becomes a child of the launcher, not of init, also
//! after a `setsid` or a double fork. So at the end of the payload each
//! process that stays is a descendant of the launcher, and the launcher
//! kills it.
//!
//! The count 0 comes from `wait4(-1)` and `ECHILD` (no child, alive or
//! ended), not from a list of `/proc`: a list can miss a process that forks
//! while it is read. `/proc` gives the pids to kill, and the count when a
//! process stays.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use pod_hostprobe::sys;

use crate::end::Residual;

/// How long the kill of the tree can take after the end of the payload. A
/// process in an uninterruptible wait can outlive a SIGKILL for this long;
/// the count then says that it stayed.
pub const TREE_KILL_MS: u64 = 5_000;

/// What one reap of the ended children found.
#[derive(Debug, Default)]
pub(super) struct Swept {
    /// The `wait4` status of `payload`, when this reap took it.
    pub payload: Option<i32>,
    /// `ECHILD`: the launcher has no child at all.
    pub none_left: bool,
}

/// Reap each child that ended, with no wait. A descendant that the launcher
/// took as subreaper is reaped here, so it is not a zombie for the life of
/// the payload.
pub(super) fn sweep(payload: i64) -> Swept {
    let mut out = Swept::default();
    loop {
        let mut status = 0i32;
        match sys::reap_any(&mut status) {
            Ok(Some(pid)) if pid == payload => out.payload = Some(status),
            Ok(Some(_)) => {}
            Ok(None) => return out,
            Err(e) if e == sys::EINTR => {}
            Err(e) if e == sys::ECHILD => {
                out.none_left = true;
                return out;
            }
            Err(_) => return out,
        }
    }
}

/// The parent pid in the text of `/proc/PID/stat`. The name of the command
/// is in parentheses and can hold a space or a `)`, so the fields start
/// after the last `)`.
pub fn parent_of(stat: &str) -> Option<i64> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    fields.next()?; // the state
    fields.next()?.parse().ok()
}

/// The children of `pid` from the `children` file of each of its tasks.
/// `None` when no such file can be read (a kernel with no
/// `CONFIG_PROC_CHILDREN`, or no `/proc`).
fn task_children(proc: &Path, pid: i64) -> Option<Vec<i64>> {
    let tasks = std::fs::read_dir(proc.join(pid.to_string()).join("task")).ok()?;
    let mut out = Vec::new();
    let mut read = false;
    for task in tasks.flatten() {
        if let Ok(text) = std::fs::read_to_string(task.path().join("children")) {
            read = true;
            out.extend(text.split_whitespace().filter_map(|w| w.parse::<i64>().ok()));
        }
    }
    read.then_some(out)
}

/// The parent of each process of `proc`, from each `PID/stat`. `None` when
/// `proc` cannot be listed or holds no `stat` that can be read.
fn parents(proc: &Path) -> Option<BTreeMap<i64, i64>> {
    let mut out = BTreeMap::new();
    for e in std::fs::read_dir(proc).ok()?.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<i64>().ok()) else { continue };
        // A process that ended between the list and the read is no
        // descendant.
        if let Some(ppid) = std::fs::read_to_string(e.path().join("stat")).ok().as_deref().and_then(parent_of) {
            out.insert(pid, ppid);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Each descendant of `me`, from `proc` (the mount of procfs): the
/// `children` files when the kernel gives them, else a scan of each
/// `PID/stat`. `None` when neither can be read.
pub fn descendants(proc: &Path, me: i64) -> Option<Vec<i64>> {
    let mut out = Vec::new();
    if let Some(first) = task_children(proc, me) {
        let mut next = first;
        while let Some(pid) = next.pop() {
            if out.contains(&pid) {
                continue;
            }
            out.push(pid);
            // A child that ended between two reads has no file: it has no
            // children either.
            next.extend(task_children(proc, pid).unwrap_or_default());
        }
        return Some(out);
    }
    let parents = parents(proc)?;
    let mut next = vec![me];
    while let Some(parent) = next.pop() {
        for (&pid, _) in parents.iter().filter(|(_, p)| **p == parent) {
            if !out.contains(&pid) {
                out.push(pid);
                next.push(pid);
            }
        }
    }
    Some(out)
}

/// The end of the tree after the end of the payload. With `kill`, each
/// descendant gets SIGKILL until none is left or [`TREE_KILL_MS`] has gone;
/// without it, the descendants are only counted. `subreaper` false (the
/// host refused it) makes each count `Unknown`: a descendant that went to
/// init is out of reach.
pub(super) fn end_tree(proc: &Path, me: i64, subreaper: bool, kill: bool) -> Residual {
    let until = Instant::now() + Duration::from_millis(TREE_KILL_MS);
    let counted = |n: usize| if subreaper { Residual::Count(n as u64) } else { Residual::Unknown };
    loop {
        if sweep(0).none_left {
            return counted(0);
        }
        let Some(tree) = descendants(proc, me) else { return Residual::Unknown };
        let left = until.saturating_duration_since(Instant::now());
        if tree.is_empty() {
            // A child exists and no list shows it: it is in its fork, or
            // `proc` is of another PID namespace. A short pause, then a new
            // look, up to the bound; at the bound the count is not known.
            if left.is_zero() {
                return Residual::Unknown;
            }
            let _ = sys::ppoll(&mut [], 10);
            continue;
        }
        if !kill || left.is_zero() {
            return counted(tree.len());
        }
        // Each process first, so that a parent cannot fork again for the
        // one that was killed.
        for &pid in &tree {
            let _ = sys::kill(pid, 9);
        }
        for &pid in &tree {
            // A pidfd of a process that ended reads at once; one that is
            // already reaped gives `ESRCH`.
            if let Ok(fd) = sys::pidfd_open(pid) {
                let mut fds = [sys::PollFd { fd: fd as i32, events: sys::POLLIN, revents: 0 }];
                let ms = until.saturating_duration_since(Instant::now()).as_millis() as i64;
                let _ = sys::ppoll(&mut fds, ms);
                let _ = sys::close(fd);
            }
        }
    }
}

#[cfg(test)]
mod tests;
