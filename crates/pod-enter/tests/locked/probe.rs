//! The payload: each argument is `OP:PATH`, and the payload writes
//! `OP:PATH=RESULT` for each (0 or a number, or the negative errno) in one
//! line to its standard output, then reads its standard input to the end,
//! so that a join can find it while it runs.

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
    let mut b = [0u8; 64];
    while let Ok(1..) = k::read(0, &mut b) {}
    0
}
