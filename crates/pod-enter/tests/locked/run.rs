//! The harness of the cases of `locked.rs`: the tree of a case, its plan,
//! the start of a payload and the check of its line.

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::path::PathBuf;

use pod_enter::{Child, Fds, Names, Plan, TmpfsMount, VolumeBind};
use pod_hostprobe::sys::{self as k, PollFd};

use super::image::{GUEST_EXE, build};
use super::probe::PROBE;

pub const EPERM: i64 = -(k::EPERM.0 as i64);
pub const EINVAL: i64 = -(k::EINVAL.0 as i64);
pub const ENOENT: i64 = -(k::ENOENT.0 as i64);
pub const EROFS: i64 = -(k::EROFS.0 as i64);
/// A wanted result: any refusal (a negative errno).
pub const DENIED: i64 = i64::MIN;

/// Is this process root of the initial user namespace?
pub fn host_root() -> bool {
    pod_testkit::nobody::is_root()
        && std::fs::read_to_string("/proc/self/uid_map").is_ok_and(|m| pod_hostprobe::userns::is_initial_map(&m))
}

/// The image, the host sides of a read-only and a writable volume, and a
/// host file that the payload must not reach.
pub struct Tree {
    pub dir: PathBuf,
    pub image: PathBuf,
    pub ro: PathBuf,
    pub rw: PathBuf,
    pub marker: String,
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn tree(tag: &str) -> Tree {
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

pub fn plan(t: &Tree, ops: &[String], pid: bool, read_only_root: bool) -> Plan {
    let volume = |host: &PathBuf, guest: &str, read_only| VolumeBind {
        host: host.to_str().unwrap().to_string(),
        guest: guest.to_string(),
        read_only,
        host_base: None,
        overlay: None,
    };
    let bytes = NonZeroU64::new(1 << 20).unwrap();
    Plan {
        volumes: vec![volume(&t.ro, "/ro", true), volume(&t.rw, "/rw", false)],
        tmpfs: vec![TmpfsMount { guest: "/t".to_string(), bytes, mode: None }],
        ..bare(ops, pid, read_only_root)
    }
}

/// A plan with no volume and no tmpfs: the fallback of the rung takes it.
pub fn bare(ops: &[String], pid: bool, read_only_root: bool) -> Plan {
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
        volumes: Vec::new(),
        userns_ids: None,
        pid_namespace: pid,
        tmpfs: Vec::new(),
        relay_title: None,
        read_only_root,
    }
}

/// A payload that runs: its line, the write end of its standard input and
/// the read end of its standard output.
pub struct Running {
    pub child: Child,
    results: HashMap<String, i64>,
    hold: i64,
    out: i64,
}

impl Running {
    /// Let the payload end, and give its exit code.
    pub fn end(self) -> Result<i32, String> {
        let _ = (k::close(self.hold), k::close(self.out));
        self.child.wait().map_err(|e| e.to_string())
    }

    pub fn get(&self, op: &str) -> Result<i64, String> {
        self.results.get(op).copied().ok_or_else(|| format!("no result of {op}: {:?}", self.results))
    }

    /// One line to the standard input of the payload, and the next line of
    /// its standard output (`probe::peek`).
    pub fn ask(&self, line: &str) -> Result<String, String> {
        k::write(self.hold, format!("{line}\n").as_bytes()).map_err(|e| format!("the ask: {e:?}"))?;
        Ok(String::from_utf8_lossy(&read_line(self.out)).trim().to_string())
    }
}

pub fn pipe() -> (i64, i64) {
    let mut p = [0i32; 2];
    k::pipe2(&mut p, k::O_CLOEXEC).expect("a pipe");
    (i64::from(p[0]), i64::from(p[1]))
}

/// The bytes of `fd` to the first newline, with a time limit of a minute.
fn read_line(fd: i64) -> Vec<u8> {
    let mut line = Vec::new();
    let mut buf = [0u8; 1];
    while !line.ends_with(b"\n") {
        let mut fds = [PollFd { fd: fd as i32, events: k::POLLIN, revents: 0 }];
        if k::ppoll(&mut fds, 60_000) != Ok(1) {
            break;
        }
        match k::read(fd, &mut buf) {
            Ok(1) => line.push(buf[0]),
            _ => break,
        }
    }
    line
}

/// Start the payload of `plan` with `spawn`, and read its line.
pub fn start(plan: &mut Plan, spawn: impl FnOnce(&Plan) -> pod_enter::Result<Child>) -> Result<Running, String> {
    let (hold_r, hold) = pipe();
    let (out, out_w) = pipe();
    plan.fds.pass = vec![(0, hold_r), (1, out_w)];
    let spawned = spawn(plan);
    let _ = (k::close(hold_r), k::close(out_w));
    let child = spawned.map_err(|e| {
        let _ = (k::close(hold), k::close(out));
        e.to_string()
    })?;
    let text = String::from_utf8_lossy(&read_line(out)).into_owned();
    let results = text
        .split_whitespace()
        .filter_map(|w| w.rsplit_once('=').and_then(|(op, r)| Some((op.to_string(), r.parse().ok()?))))
        .collect();
    Ok(Running { child, results, hold, out })
}

/// The result of each operation, against the wanted one.
pub fn expect(r: &Running, want: &[(&str, i64)]) -> Result<(), String> {
    let wrong: Vec<String> = want
        .iter()
        .filter_map(|(op, w)| match r.get(op) {
            Ok(got) if got == *w || (*w == DENIED && got < 0) => None,
            Ok(got) => Some(format!(
                "{op} gave {got}, not {}",
                if *w == DENIED { "a refusal".to_string() } else { w.to_string() }
            )),
            Err(e) => Some(e),
        })
        .collect();
    if wrong.is_empty() { Ok(()) } else { Err(wrong.join("; ")) }
}

/// The operations of the lock, and what each must give. A write of
/// `/proc/sys` and `/proc/sysrq-trigger` is refused on each path (T-1636):
/// the probe opens them, it never writes.
pub fn lock_ops(marker: &str) -> (Vec<String>, Vec<(String, i64)>) {
    let want = [
        ("remount:/ro".to_string(), EPERM),
        ("write:/ro/f".to_string(), EROFS),
        ("remount:/proc".to_string(), EPERM),
        ("size:/t".to_string(), EPERM),
        ("umount:/ro".to_string(), EINVAL),
        ("write:/proc/sys/kernel/core_pattern".to_string(), DENIED),
        ("write:/proc/sysrq-trigger".to_string(), DENIED),
        (format!("escape:{marker}"), ENOENT),
    ];
    (want.iter().map(|(op, _)| op.clone()).collect(), want.to_vec())
}

pub fn borrowed(want: &[(String, i64)]) -> Vec<(&str, i64)> {
    want.iter().map(|(op, w)| (op.as_str(), *w)).collect()
}
