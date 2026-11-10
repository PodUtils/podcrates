//! The reads of `/proc` behind the table: the inode of a socket, the
//! sockets of a process, the processes of a payload, and the descriptor
//! limit of the supervisor.

use std::collections::HashSet;
use std::os::fd::RawFd;
use std::os::unix::ffi::OsStrExt;

use pod_core::Failure;
use pod_hostprobe::sys;

use super::Key;
use crate::failure;

/// `RLIMIT_NOFILE`.
const RLIMIT_NOFILE: u64 = 7;

/// The soft and the hard `RLIMIT_NOFILE` of this process (1024 and 1024
/// when the read fails). `prlimit64` gives 64-bit values on each target.
pub fn nofile() -> (u64, u64) {
    sys::prlimit(RLIMIT_NOFILE).unwrap_or((1024, 1024))
}

/// Raise the soft `RLIMIT_NOFILE` of the supervisor to the hard limit.
/// Call it after the fork of the program: the program keeps its limit.
pub fn raise_nofile() {
    let (soft, hard) = nofile();
    if soft < hard {
        let limit = [hard, hard];
        // SAFETY: prlimit64 reads one `struct rlimit64` (two u64) of this
        // process and writes none.
        let _ = unsafe { sys::sys(sys::SYS_PRLIMIT64, [0, RLIMIT_NOFILE, limit.as_ptr() as u64, 0, 0, 0]) };
    }
}

/// The inode number of the descriptor `fd` of the supervisor.
pub fn inode_of(fd: RawFd) -> Result<Key, Failure> {
    // SAFETY: `st` is a valid buffer for one `struct stat`.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return Err(failure::last_os("fstat"));
    }
    Ok(st.st_ino as Key)
}

/// The inode of the socket at the descriptor `fd` of the task `pid`; 0 when
/// the descriptor is not a socket or cannot be read.
pub fn socket_inode(pid: i32, fd: i32) -> Key {
    match std::fs::read_link(format!("/proc/{pid}/fd/{fd}")) {
        Ok(link) => parse_socket_link(link.as_os_str().as_bytes()),
        Err(_) => 0,
    }
}

/// The inode of a link `socket:[INODE]`; 0 for another link.
pub fn parse_socket_link(link: &[u8]) -> Key {
    link.strip_prefix(b"socket:[")
        .and_then(|rest| rest.strip_suffix(b"]"))
        .and_then(|digits| std::str::from_utf8(digits).ok()?.parse().ok())
        .unwrap_or(0)
}

/// Each socket of the process `pid`: (descriptor, inode). `None` when its
/// table of descriptors cannot be read.
pub fn sockets_of(pid: i32) -> Option<Vec<(i32, Key)>> {
    let entries = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Some(fd) = entry.file_name().to_str().and_then(|n| n.parse().ok()) else {
            continue;
        };
        if let Ok(link) = std::fs::read_link(entry.path()) {
            let key = parse_socket_link(link.as_os_str().as_bytes());
            if key != 0 {
                out.push((fd, key));
            }
        }
    }
    Some(out)
}

/// Each process of `todo` and each of its descendants.
pub(super) fn descendants(mut todo: Vec<i32>) -> HashSet<i32> {
    let mut found = HashSet::new();
    while let Some(process) = todo.pop() {
        if process <= 0 || !found.insert(process) {
            continue;
        }
        let Ok(threads) = std::fs::read_dir(format!("/proc/{process}/task")) else {
            continue;
        };
        for thread in threads.flatten() {
            if let Ok(text) = std::fs::read_to_string(thread.path().join("children")) {
                todo.extend(text.split_whitespace().filter_map(|p| p.parse::<i32>().ok()));
            }
        }
    }
    found
}

/// The thread group id of the task `tid` (`Tgid:` of its status).
pub(super) fn tgid_of(tid: u32) -> i32 {
    std::fs::read_to_string(format!("/proc/{tid}/status"))
        .ok()
        .and_then(|text| text.lines().find_map(|l| l.strip_prefix("Tgid:")).and_then(|v| v.trim().parse().ok()))
        .unwrap_or(tid as i32)
}

pub(super) fn alive(pid: i32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}
