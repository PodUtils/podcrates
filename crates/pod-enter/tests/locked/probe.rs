//! The payload: each argument is `OP:PATH`, and the payload writes
//! `OP:PATH=RESULT` for each (0 or a number, or the negative errno) in one
//! line to its standard output, then reads its standard input to the end,
//! so that a join can find it while it runs. With `peek:PATH`, each line
//! of its standard input is a pid: the payload walks to `PATH` from each
//! descriptor and from the working directory of that process, and writes
//! `PID=RESULT` ([`peek`]).

use std::os::unix::fs::MetadataExt;

use pod_hostprobe::sys::{self as k, CBuf};

/// The first argument of the binary as the payload.
pub const PROBE: &str = "probe";

fn c(p: &str) -> CBuf {
    CBuf::new(p).expect("no NUL")
}

fn code(r: k::Sysres) -> i64 {
    match r {
        Ok(_) => 0,
        Err(e) => -i64::from(e.0),
    }
}

/// The way out of a `chroot(2)`: a chroot into a new directory, `..` 64
/// times, a chroot of `.`, then a `stat(2)` of `marker`, a host path. The
/// errno of the first call that fails, or of the stat.
fn escape(marker: &str) -> i64 {
    let at = c("/tmp/way-out");
    let _ = k::mkdir(&at, 0o755);
    if let Err(e) = k::chroot(&at) {
        return -i64::from(e.0);
    }
    for _ in 0..64 {
        let _ = k::chdir(&c(".."));
    }
    if let Err(e) = k::chroot(&c(".")) {
        return -i64::from(e.0);
    }
    k::stat(&c(marker)).map_or_else(|e| -i64::from(e.0), |_| 0)
}

/// Write `bytes` into the file `path` opened with `flags`: the count of the
/// bytes, or the negative errno.
fn put(path: &CBuf, flags: u64, bytes: &[u8]) -> i64 {
    match k::open(path, flags | k::O_WRONLY | k::O_TRUNC | k::O_CLOEXEC, 0o644) {
        Ok(fd) => {
            let r = code(k::write(fd, bytes).map(|_| 0)).min(0);
            let _ = k::close(fd);
            if r < 0 { r } else { bytes.len() as i64 }
        }
        Err(e) => -i64::from(e.0),
    }
}

/// The way out through a process `pid` of the host (T-1635): `PATH`
/// relative to each directory that the process holds (a descriptor, its
/// working directory). 1 where a walk reaches `path`, 0 where none does,
/// or the negative errno of the open of `/proc/PID/fd`.
fn peek(pid: &str, path: &str) -> i64 {
    let fds = match std::fs::read_dir(format!("/proc/{pid}/fd")) {
        Ok(d) => d,
        Err(e) => return -i64::from(e.raw_os_error().unwrap_or(1000)),
    };
    let mut dirs: Vec<String> =
        fds.flatten().map(|e| format!("/proc/{pid}/fd/{}", e.file_name().to_string_lossy())).collect();
    dirs.push(format!("/proc/{pid}/cwd"));
    i64::from(dirs.iter().any(|d| std::fs::metadata(format!("{d}/{path}")).is_ok()))
}

/// One operation.
fn op(op: &str, path: &str) -> i64 {
    let (empty, p) = (k::cempty(), c(path));
    match op {
        // A bind remount with no flag: it clears `ro`, `nosuid`, `nodev`
        // and `noexec` of the mount.
        "remount" => code(k::mount(&empty, &p, &empty, k::MS_REMOUNT | k::MS_BIND)),
        "size" => code(k::mount_data(&empty, &p, &empty, k::MS_REMOUNT, &c("size=64m"))),
        "umount" => code(k::umount(&p)),
        "write" => code(k::open(&p, k::O_CREAT | k::O_WRONLY | k::O_CLOEXEC, 0o644)),
        "escape" => escape(path),
        // A new file, and a change of a file that exists.
        "put" => put(&p, k::O_CREAT, b"new"),
        "change" => put(&p, 0, b"changed"),
        "unlink" => code(k::unlink(&p)),
        "stat" => code(k::stat(&p).map(|_| 0)),
        "rdwr" => code(k::open(&p, k::O_RDWR | k::O_NOCTTY | k::O_CLOEXEC, 0)),
        // 1 where the path is a character device, else 0.
        "chr" => k::stat(&p).map_or_else(|e| -i64::from(e.0), |st| i64::from(st.st_mode & 0o170000 == 0o020000)),
        // The hard limit of a resource of `setrlimit(2)`, by its number.
        "hard" => path.parse().ok().and_then(|r| k::prlimit(r).ok()).map_or(-1, |(_, h)| h as i64),
        "peek" => 0,
        // The inode of a namespace of this process.
        "ns" => std::fs::metadata(format!("/proc/self/ns/{path}")).map_or(-1, |m| m.ino() as i64),
        _ => -1000,
    }
}

/// The payload. Exit 0 after the line.
pub fn main(args: &[String]) -> i32 {
    let mut line = String::new();
    // The escape last: it changes the root of this process.
    let (last, first): (Vec<&String>, Vec<&String>) = args.iter().partition(|a| a.starts_with("escape:"));
    for a in first.into_iter().chain(last) {
        let (o, p) = a.split_once(':').unwrap_or((a, ""));
        line.push_str(&format!("{a}={} ", op(o, p)));
    }
    let _ = k::write(1, format!("{}\n", line.trim_end()).as_bytes());
    let peek_at = args.iter().find_map(|a| a.strip_prefix("peek:"));
    let mut b = [0u8; 64];
    let mut asked = Vec::new();
    while let Ok(n @ 1..) = k::read(0, &mut b) {
        asked.extend_from_slice(&b[..n as usize]);
        while let (Some(at), Some(path)) = (asked.iter().position(|c| *c == b'\n'), peek_at) {
            let pid = String::from_utf8_lossy(&asked[..at]).trim().to_string();
            asked.drain(..=at);
            let _ = k::write(1, format!("{pid}={}\n", peek(&pid, path)).as_bytes());
        }
    }
    0
}
