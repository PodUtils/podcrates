//! The netns hold: a child enters a new network namespace (in a new user
//! namespace when it has no privilege), brings up `lo`, makes a TUN as its
//! only route, and gives the TUN to the parent with `SCM_RIGHTS`. The
//! parent writes the id maps of a user namespace. The stack that answers
//! on the TUN is the caller's.
//!
//! The handshake on the socket pair: the child writes `u` (a user
//! namespace) or `n`; after `u` the parent writes the maps and then `a`;
//! the child then sends the TUN.

use std::os::fd::{FromRawFd, OwnedFd, RawFd};

use pod_core::{Class, Failure};
use pod_hostprobe::sys;

use crate::failure;

mod fdpass;
mod tun;

pub use fdpass::{recv_fd, send_fd, send_fd_raw};
pub use tun::{IFF_NO_PI, IFF_TUN, Tun, bringup_loopback, create_tun, ifreq_for};

/// `CLONE_NEWNET`.
pub const CLONE_NEWNET: u64 = 0x4000_0000;

/// Enter a new network namespace. A network namespace alone needs
/// `CAP_SYS_ADMIN`; the pair of a user and a network namespace works with
/// no privilege, and the parent then writes the maps. Return true for the
/// pair.
pub fn enter() -> Result<bool, Failure> {
    let Err(alone) = sys::unshare(CLONE_NEWNET) else {
        return Ok(false);
    };
    match sys::unshare(sys::CLONE_NEWUSER | CLONE_NEWNET) {
        Ok(_) => Ok(true),
        Err(pair) => Err(failure::host(
            Class::Unsupported,
            format!(
                "unshare(CLONE_NEWNET) failed ({}) and unshare(CLONE_NEWUSER|CLONE_NEWNET) failed ({})",
                alone.name(),
                pair.name()
            ),
        )),
    }
}

/// The child side of the handshake: tell the parent the kind of the
/// namespace, and after a user namespace wait for its maps.
pub fn announce(sock: RawFd, userns: bool) -> Result<(), Failure> {
    write_all_fd(sock, if userns { b"u" } else { b"n" })?;
    if userns {
        let mut ack = [0u8; 1];
        if read_exact_fd(sock, &mut ack)? == 0 {
            return Err(failure::host(Class::Unavailable, "the parent died before it wrote the id maps"));
        }
    }
    Ok(())
}

/// The parent side of the handshake for the child `pid`: read the kind of
/// its namespace; for a user namespace map `uid` and `gid` to themselves
/// and answer. Return true for a user namespace.
pub fn answer(sock: RawFd, pid: u32, uid: u32, gid: u32) -> Result<bool, Failure> {
    let mut kind = [0u8; 1];
    if read_exact_fd(sock, &mut kind)? == 0 {
        return Err(failure::host(Class::Unavailable, "the child died before it named its namespace"));
    }
    if kind[0] != b'u' {
        return Ok(false);
    }
    write_id_maps(pid, uid, gid)?;
    write_all_fd(sock, b"a")?;
    Ok(true)
}

/// Write `data` to the file `path` of `/proc`.
pub fn write_proc(path: &str, data: &str) -> Result<(), Failure> {
    std::fs::write(path, data).map_err(|e| failure::host(Class::of_io(e.kind()), format!("write {path}: {e}")))
}

/// Map `uid` and `gid` of the user namespace of `pid` to themselves, after
/// `setgroups` is denied (a task with no privilege can write a gid map
/// only so).
pub fn write_id_maps(pid: u32, uid: u32, gid: u32) -> Result<(), Failure> {
    write_proc(&format!("/proc/{pid}/setgroups"), "deny")?;
    write_proc(&format!("/proc/{pid}/uid_map"), &format!("{uid} {uid} 1\n"))?;
    write_proc(&format!("/proc/{pid}/gid_map"), &format!("{gid} {gid} 1\n"))
}

/// Write each byte of `buf` to `fd`.
pub fn write_all_fd(fd: RawFd, mut buf: &[u8]) -> Result<(), Failure> {
    while !buf.is_empty() {
        // SAFETY: the buffer is valid for its length.
        let n = unsafe { libc::write(fd, buf.as_ptr() as *const _, buf.len()) };
        if n <= 0 {
            return Err(failure::last_os("write"));
        }
        buf = &buf[n as usize..];
    }
    Ok(())
}

/// Read `buf.len()` bytes from `fd`; fewer at the end of the stream.
/// Return the count.
pub fn read_exact_fd(fd: RawFd, buf: &mut [u8]) -> Result<usize, Failure> {
    let mut off = 0;
    while off < buf.len() {
        // SAFETY: the buffer is valid from `off` for the rest.
        let n = unsafe { libc::read(fd, buf[off..].as_mut_ptr() as *mut _, buf.len() - off) };
        if n < 0 {
            return Err(failure::last_os("read"));
        }
        if n == 0 {
            return Ok(off);
        }
        off += n as usize;
    }
    Ok(off)
}

/// A unix stream socket pair, close-on-exec.
pub fn socketpair() -> Result<(OwnedFd, OwnedFd), Failure> {
    let mut fds = [0 as RawFd; 2];
    // SAFETY: the kernel writes two descriptors.
    if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0, fds.as_mut_ptr()) } != 0 {
        return Err(failure::last_os("socketpair"));
    }
    // SAFETY: two new descriptors that nothing else owns.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

#[cfg(test)]
mod tests;
