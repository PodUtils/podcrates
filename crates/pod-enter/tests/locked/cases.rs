//! The cases of `locked.rs`, and the paths of the caller that run them.

use pod_enter::{RootDir, spawn_joined, spawn_namespace};
use pod_hostprobe::sys as k;
use pod_testkit::host::skip_line;
use pod_testkit::nobody;

use super::run::{EPERM, EROFS, borrowed, expect, host_root, lock_ops, plan, start, tree};

/// The first argument of the binary that runs the cases on one path; the
/// second names the path.
pub const CASES: &str = "cases";
/// The first argument of the binary that runs the case of the ambient set
/// (`more::ambient`).
pub const AMBIENT: &str = "ambient";
/// The first argument of the binary that runs the case of a proc of
/// another PID namespace (`more::foreign_proc`).
pub const FOREIGN_PROC: &str = "foreign-proc";

/// `CAP_SYS_ADMIN`.
const CAP_SYS_ADMIN: u32 = 21;

/// The cases on the path of this process, then on each other path that
/// this host gives. The exit code of the run. The paths that only root
/// makes (a range of ids, an ambient capability, a proc of another PID
/// namespace) run where this process is root, and print nothing
/// elsewhere.
pub fn each_path() -> i32 {
    let here = if nobody::is_root() { "host root" } else { "the user namespace of the rung" };
    let mut failed = run_all(here);
    let mut again = Vec::new();
    if nobody::is_root() {
        let label = "the user namespace of the rung (uid 65534)";
        again.push((label, nobody::again(&[CASES, label], &[])));
    }
    let label = "uid 0 of a user namespace of one id";
    again.push((label, nobody::again_as_root_of_a_user_namespace(&[CASES, label], &[])));
    if nobody::is_root() {
        let label = "uid 0 of a user namespace of a range of ids";
        again.push((label, nobody::again_as_root_of_a_range(&[CASES, label], &[])));
        let label = "uid 65534 with CAP_SYS_ADMIN in its ambient set";
        again.push((label, nobody::again_with_ambient(CAP_SYS_ADMIN, &[AMBIENT, label], &[])));
        let label = "host root in a PID namespace over the host's proc";
        again.push((label, nobody::again_in_a_pid_namespace(&[FOREIGN_PROC, label], &[])));
    }
    for (label, code) in again {
        match code {
            Ok(0) => {}
            Ok(code) => {
                println!("test {label} ... FAILED (exit {code})");
                failed += 1;
            }
            Err(why) => skip_line(&format!("{label}: {why}")),
        }
    }
    println!("test result: {failed} failed");
    if failed == 0 { 0 } else { 101 }
}

/// One case, with or with no PID namespace.
pub type Case = fn(bool) -> Result<(), String>;

/// The cases and the PID namespaces each one runs with. A case of a join
/// of the host's processes runs with no PID namespace only: in one, the
/// payload of the run sees no process of the host.
const BOTH: &[bool] = &[false, true];
const NONE: &[bool] = &[false];

/// Each case on the path `path` (the path of this process), with and with
/// no PID namespace. The count of the failed cases.
pub fn run_all(path: &str) -> i32 {
    if !nobody::is_root() {
        assert_eq!(nobody::effective_caps(), Some(0), "the caller must hold no capability");
        if let Err(why) = pod_testkit::host::userns() {
            skip_line(&format!("{path}: the kernel gives this caller no user namespace for the rung: {why}"));
            return 0;
        }
    }
    let cases: [(&str, Case, &[bool]); 10] = [
        ("the mounts of the rung are locked and the root holds", locked, BOTH),
        ("a join lands in the namespaces of the payload and is locked", joined, BOTH),
        ("a read-only root refuses a write", read_only, BOTH),
        ("a join is not dumpable between its join and its exec", super::join_dump::join_not_dumpable, NONE),
        ("an image with no /tmp runs locked", super::fallback::no_tmp, BOTH),
        ("an image whose /tmp is a link is refused", super::fallback::tmp_link, NONE),
        ("a payload of uid 0 refuses the fallback to chroot", super::fallback::refused_fallback, NONE),
        ("a hard limit over the one of the caller", super::more::limit_over_hard, BOTH),
        ("a read-only root has a /dev of its own", super::more::dev_of_its_own, BOTH),
        ("a read-only root hides a mount under its tree", super::more::mount_under_the_tree, BOTH),
    ];
    run_each(path, &cases)
}

/// Each case of `cases` on `path`. The count of the failed cases.
pub fn run_each(path: &str, cases: &[(&str, Case, &[bool])]) -> i32 {
    let mut failed = 0;
    for (name, case, pids) in cases {
        for pid in pids.iter().copied() {
            let ns = if pid { "with" } else { "with no" };
            let r = std::panic::catch_unwind(|| case(pid)).unwrap_or_else(|_| Err("panicked".to_string()));
            match r {
                Ok(()) => println!("test {path}: {name}, {ns} PID namespace ... ok"),
                Err(why) => {
                    println!("test {path}: {name}, {ns} PID namespace ... FAILED: {why}");
                    failed += 1;
                }
            }
        }
    }
    failed
}

/// The payload gets `EPERM` or `EINVAL` on each change of a mount of the
/// rung, and its way out of a chroot reaches no host path. The host side
/// of the read-only volume keeps its content.
fn locked(pid: bool) -> Result<(), String> {
    let t = tree(if pid { "lock-pid" } else { "lock" });
    let (mut ops, mut want) = lock_ops(&t.marker);
    // A masked file of the proc of host root is `/dev/null` (T-1636).
    if host_root() && std::path::Path::new("/proc/timer_list").exists() {
        ops.push("chr:/proc/timer_list".to_string());
        want.push(("chr:/proc/timer_list".to_string(), 1));
    }
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let mut err = Vec::new();
    let r = start(&mut plan(&t, &ops, pid, false), |p| spawn_namespace(&root, p, &mut err))?;
    let out = expect(&r, &borrowed(&want));
    let code = r.end()?;
    out?;
    let host = std::fs::read_to_string(t.ro.join("f")).map_err(|e| e.to_string())?;
    if code != 0 || host != "host" {
        return Err(format!("exit {code}, the host file holds {host:?}: {}", String::from_utf8_lossy(&err)));
    }
    Ok(())
}

/// A run that waits, then a join of it: the join is in the user and the
/// mount namespace of the payload of the run, and is locked as the payload.
fn joined(pid: bool) -> Result<(), String> {
    let t = tree(if pid { "join-pid" } else { "join" });
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let ns = ["ns:mnt".to_string(), "ns:user".to_string()];
    let mut err = Vec::new();
    let run = start(&mut plan(&t, &ns, pid, false), |p| spawn_namespace(&root, p, &mut err))?;
    let (mut ops, mut want) = lock_ops(&t.marker);
    for n in &ns {
        want.push((n.clone(), run.get(n)?));
    }
    ops.extend(ns.iter().cloned());
    let pidfd = k::pidfd_open(run.child.pid).map_err(|e| format!("pidfd_open: {e:?}"))?;
    let mut joined_err = Vec::new();
    let joined = start(&mut plan(&t, &ops, pid, false), |p| spawn_joined(&root, p, pidfd, &mut joined_err));
    let _ = k::close(pidfd);
    let out = joined.and_then(|j| {
        let out = expect(&j, &borrowed(&want));
        j.end().map(|_| ()).and(out)
    });
    let code = run.end()?;
    let text = String::from_utf8_lossy(&joined_err);
    if text.contains("does not share the mounts") {
        return Err(format!("the join fell back: {text}"));
    }
    out.and(if code == 0 { Ok(()) } else { Err(format!("the run gave {code}")) })
}

/// A read-only root: a write to `/` gives `EROFS`, and the payload cannot
/// remount it writable; `/tmp`, `/dev`, a volume and a tmpfs stay
/// writable. The chroot rung refuses the plan.
fn read_only(pid: bool) -> Result<(), String> {
    let t = tree(if pid { "ro-pid" } else { "ro" });
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let want = [
        ("write:/made", EROFS),
        ("remount:/", EPERM),
        ("write:/tmp/made", 0),
        ("write:/dev/made", 0),
        ("write:/rw/made", 0),
        ("write:/t/made", 0),
    ];
    let ops: Vec<String> = want.iter().map(|(op, _)| op.to_string()).collect();
    let dev = t.image.join("dev");
    std::fs::set_permissions(&dev, std::os::unix::fs::PermissionsExt::from_mode(0o777)).map_err(|e| e.to_string())?;
    let mut err = Vec::new();
    let mut p = plan(&t, &ops, pid, true);
    let r = start(&mut p, |p| spawn_namespace(&root, p, &mut err))?;
    let out = expect(&r, &want);
    let code = r.end()?;
    out?;
    if code != 0 || t.image.join("made").exists() || !t.rw.join("made").exists() {
        return Err(format!("exit {code}, or a write landed in the wrong place: {}", String::from_utf8_lossy(&err)));
    }
    // The read-only root alone: the first mount that the chroot rung names.
    (p.volumes, p.tmpfs, p.pid_namespace) = (Vec::new(), Vec::new(), false);
    let refused = pod_enter::spawn(&root, &p, &mut std::io::sink()).err().map(|e| e.to_string());
    match refused {
        Some(e) if e.contains("the read-only root needs the namespace rung") => Ok(()),
        other => Err(format!("the chroot rung took a read-only root: {other:?}")),
    }
}
