//! The cases of `locked.rs`, and the paths of the caller that run them.

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::path::PathBuf;

use pod_enter::{Child, Fds, Names, Plan, RootDir, TmpfsMount, VolumeBind, spawn_joined, spawn_namespace};
use pod_hostprobe::sys::{self as k, PollFd};
use pod_testkit::host::skip_line;
use pod_testkit::nobody;

use super::image::{GUEST_EXE, build};
use super::probe::PROBE;

/// The first argument of the binary that runs the cases on one path; the
/// second names the path.
pub const CASES: &str = "cases";

const EPERM: i64 = -(k::EPERM.0 as i64);
const EINVAL: i64 = -(k::EINVAL.0 as i64);
const ENOENT: i64 = -(k::ENOENT.0 as i64);
const EROFS: i64 = -(k::EROFS.0 as i64);

/// The cases on the path of this process, then on each other path that
/// this host gives. The exit code of the run.
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
    let cases: [(&str, fn(bool) -> Result<(), String>); 3] = [
        ("the mounts of the rung are locked and the root holds", locked),
        ("a join lands in the namespaces of the payload and is locked", joined),
        ("a read-only root refuses a write", read_only),
    ];
    let mut failed = 0;
    for (name, case) in cases {
        for pid in [false, true] {
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

/// The image, the host sides of a read-only and a writable volume, and a
/// host file that the payload must not reach.
struct Tree {
    dir: PathBuf,
    image: PathBuf,
    ro: PathBuf,
    rw: PathBuf,
    marker: String,
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn tree(tag: &str) -> Tree {
    let dir = std::env::temp_dir().join(format!("pod-enter-locked-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let image = dir.join("image");
    build(&image);
    for d in ["ro", "rw", "t", "proc", "dev"] {
        std::fs::create_dir_all(image.join(d)).unwrap();
    }
    let (ro, rw) = (dir.join("ro-host"), dir.join("rw-host"));
    std::fs::create_dir_all(&ro).unwrap();
    std::fs::create_dir_all(&rw).unwrap();
    std::fs::write(ro.join("f"), "host").unwrap();
    let marker = dir.join("marker");
    std::fs::write(&marker, "host").unwrap();
    Tree { marker: marker.to_str().unwrap().to_string(), dir, image, ro, rw }
}

fn plan(t: &Tree, ops: &[String], pid: bool, read_only_root: bool) -> Plan {
    let volume = |host: &PathBuf, guest: &str, read_only| VolumeBind {
        host: host.to_str().unwrap().to_string(),
        guest: guest.to_string(),
        read_only,
        host_base: None,
    };
    let bytes = NonZeroU64::new(1 << 20).unwrap();
    Plan {
        argv: [GUEST_EXE, PROBE].into_iter().map(String::from).chain(ops.iter().cloned()).collect(),
        env: Vec::new(),
        working_dir: "/".to_string(),
        fds: Fds::default(),
        banner: String::new(),
        path_dirs: Vec::new(),
        names: Names::DEFAULT,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
        confine: None,
        hostname: None,
        volumes: vec![volume(&t.ro, "/ro", true), volume(&t.rw, "/rw", false)],
        userns_ids: None,
        pid_namespace: pid,
        tmpfs: vec![TmpfsMount { guest: "/t".to_string(), bytes }],
        relay_title: None,
        read_only_root,
    }
}

/// A payload that runs: its line and the write end of its standard input.
struct Running {
    child: Child,
    results: HashMap<String, i64>,
    hold: i64,
}

impl Running {
    /// Let the payload end, and give its exit code.
    fn end(self) -> Result<i32, String> {
        let _ = k::close(self.hold);
        self.child.wait().map_err(|e| e.to_string())
    }

    fn get(&self, op: &str) -> Result<i64, String> {
        self.results.get(op).copied().ok_or_else(|| format!("no result of {op}: {:?}", self.results))
    }
}

fn pipe() -> (i64, i64) {
    let mut p = [0i32; 2];
    k::pipe2(&mut p, k::O_CLOEXEC).expect("a pipe");
    (i64::from(p[0]), i64::from(p[1]))
}

/// Start the payload of `plan` with `spawn`, and read its line.
fn start(plan: &mut Plan, spawn: impl FnOnce(&Plan) -> pod_enter::Result<Child>) -> Result<Running, String> {
    let (hold_r, hold) = pipe();
    let (out, out_w) = pipe();
    plan.fds.pass = vec![(0, hold_r), (1, out_w)];
    let spawned = spawn(plan);
    let _ = (k::close(hold_r), k::close(out_w));
    let child = spawned.map_err(|e| {
        let _ = (k::close(hold), k::close(out));
        e.to_string()
    })?;
    let mut line = Vec::new();
    let mut buf = [0u8; 512];
    while !line.ends_with(b"\n") {
        let mut fds = [PollFd { fd: out as i32, events: k::POLLIN, revents: 0 }];
        if k::ppoll(&mut fds, 60_000) != Ok(1) {
            break;
        }
        match k::read(out, &mut buf) {
            Ok(n @ 1..) => line.extend_from_slice(&buf[..n as usize]),
            _ => break,
        }
    }
    let _ = k::close(out);
    let text = String::from_utf8_lossy(&line);
    let results = text
        .split_whitespace()
        .filter_map(|w| w.rsplit_once('=').and_then(|(op, r)| Some((op.to_string(), r.parse().ok()?))))
        .collect();
    Ok(Running { child, results, hold })
}

/// The result of each operation, against the wanted one.
fn expect(r: &Running, want: &[(&str, i64)]) -> Result<(), String> {
    let wrong: Vec<String> = want
        .iter()
        .filter_map(|(op, w)| match r.get(op) {
            Ok(got) if got == *w => None,
            Ok(got) => Some(format!("{op} gave {got}, not {w}")),
            Err(e) => Some(e),
        })
        .collect();
    if wrong.is_empty() { Ok(()) } else { Err(wrong.join("; ")) }
}

/// The operations of the lock, and what each must give.
fn lock_ops(marker: &str) -> (Vec<String>, Vec<(String, i64)>) {
    let want = [
        ("remount:/ro".to_string(), EPERM),
        ("write:/ro/f".to_string(), EROFS),
        ("remount:/proc".to_string(), EPERM),
        ("size:/t".to_string(), EPERM),
        ("umount:/ro".to_string(), EINVAL),
        (format!("escape:{marker}"), ENOENT),
    ];
    (want.iter().map(|(op, _)| op.clone()).collect(), want.to_vec())
}

fn borrowed(want: &[(String, i64)]) -> Vec<(&str, i64)> {
    want.iter().map(|(op, w)| (op.as_str(), *w)).collect()
}

/// The payload gets `EPERM` or `EINVAL` on each change of a mount of the
/// rung, and its way out of a chroot reaches no host path. The host side
/// of the read-only volume keeps its content.
fn locked(pid: bool) -> Result<(), String> {
    let t = tree(if pid { "lock-pid" } else { "lock" });
    let (ops, want) = lock_ops(&t.marker);
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
