use super::*;

pub fn wait4(pid: i64, status: &mut i32) -> Sysres {
    unsafe { sys(SYS_WAIT4, [pid as u64, status as *mut i32 as u64, 0, 0, 0, 0]) }
}

pub fn kill(pid: i64, sig: i64) -> Sysres {
    unsafe { sys(SYS_KILL, [pid as u64, sig as u64, 0, 0, 0, 0]) }
}

/// Start a new session, so a detached supervisor outlives the shell that
/// started it and holds no controlling terminal.
///
/// A launcher still attached to the session of the caller takes the signals
/// of that terminal, and a detached run then stops with the shell that
/// started it.
pub fn setsid() -> Sysres {
    unsafe { sys(SYS_SETSID, [0, 0, 0, 0, 0, 0]) }
}

pub fn getpid() -> i64 {
    unsafe { sys(SYS_GETPID, [0, 0, 0, 0, 0, 0]) }.unwrap_or(0)
}

/// A descriptor for a process, which cannot be redirected by pid reuse.
///
/// A pid plus a start time is a heuristic; a pidfd names one process for as long as it is
/// held and answers `ESRCH` for one that has been reaped. Say what it is not:
/// it addresses ONE process. It does not contain descendants and it is not a PID
/// namespace, so a grandchild that reparents is outside its reach.
pub fn pidfd_open(pid: i64) -> Sysres {
    unsafe { sys(SYS_PIDFD_OPEN, [pid as u64, 0, 0, 0, 0, 0]) }
}

/// Block the two shutdown signals and report the previous mask, so the
/// caller reads them from a signalfd and does not stop on them.
///
/// The mask is one `u64` and not a libc `sigset_t`: the raw call sizes the
/// set in bytes, 8 covers signals 1..64, and SIGINT (2) and SIGTERM (15) are
/// both inside it. A 128-byte libc set handed with size 8 would leave the
/// kernel reading the caller's first 8 bytes, which happen to be the same two
/// bits, but "happens to be" is not an argument.
pub fn sigblock_shutdown(old: &mut u64) -> Sysres {
    let set: u64 = SHUTDOWN_SIGNALS;
    unsafe { sys(SYS_RT_SIGPROCMASK, [SIG_BLOCK, &set as *const u64 as u64, old as *mut u64 as u64, 8, 0, 0]) }
}

/// Restore a mask [`sigblock_shutdown`] reported.
pub fn sigrestore(old: u64) -> Sysres {
    unsafe { sys(SYS_RT_SIGPROCMASK, [SIG_SETMASK, &old as *const u64 as u64, 0, 8, 0, 0]) }
}

/// A descriptor that reads as the blocked shutdown signals, `CLOEXEC` so an
/// `execve` never inherits it. The set is [`SHUTDOWN_SIGNALS`], the same word
/// [`sigblock_shutdown`] blocks: passed by address, because `signalfd4` takes
/// a pointer and the value itself is an address the kernel must not read.
pub fn signalfd_shutdown() -> Sysres {
    let mask: u64 = SHUTDOWN_SIGNALS;
    unsafe { sys(SYS_SIGNALFD4, [-1i64 as u64, &mask as *const u64 as u64, 8, SFD_CLOEXEC, 0, 0]) }
}

/// `waitid(P_PIDFD, ...)`: a child's status, addressed by descriptor.
pub const P_PIDFD: u64 = 3;
pub const WEXITED: u64 = 0x0000_0004;
pub const WNOHANG: u64 = 0x0000_0001;

/// What `waitid` fills in, of the kernel's `siginfo_t`, and nothing more.
///
/// **The kernel's own buffer, sized generously and read by offset.**
/// `siginfo_t` is 128 bytes on every Linux architecture and its layout after the
/// first three words is a union; the two fields wanted here (`si_code` and
/// `si_status`) sit at fixed offsets in the `SIGCHLD` arm. A hand-written
/// `#[repr(C)]` one field short is a kernel write past the end of it, which is
/// why the buffer is the full 128 bytes whatever is read out of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exited {
    /// `CLD_EXITED` (1), `CLD_KILLED` (2), `CLD_DUMPED` (3).
    pub code: i32,
    /// The exit status for `CLD_EXITED`, the signal number otherwise.
    pub status: i32,
}

pub const CLD_EXITED: i32 = 1;

/// Reap a child through its pidfd. `Ok(None)` with `WNOHANG` when it is still
/// running.
pub fn waitid_pidfd(pidfd: i64, nohang: bool) -> Result<Option<Exited>, Errno> {
    // 128 bytes, zeroed, so a short read leaves zeros rather than stack noise.
    let mut buf = [0u8; 128];
    let opts = WEXITED | if nohang { WNOHANG } else { 0 };
    unsafe { sys(SYS_WAITID, [P_PIDFD, pidfd as u64, buf.as_mut_ptr() as u64, opts, 0, 0]) }?;
    // `si_signo` is the first word and is zero when WNOHANG found nothing,
    // which is how "still running" is told from "exited with 0".
    let signo = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if signo == 0 {
        return Ok(None);
    }
    // si_signo, si_errno, si_code are the first three 32-bit words.
    let code = i32::from_ne_bytes([buf[8], buf[9], buf[10], buf[11]]);
    // THE UNION STARTS AT 16, NOT AT 12. `__ARCH_SI_PREAMBLE_SIZE` is
    // `3*sizeof(int) + sizeof(long)`-aligned, so on a 64-bit architecture the
    // three leading ints are followed by four bytes of padding and `_sifields`
    // begins at 16. The SIGCHLD arm is `{ pid_t si_pid; uid_t si_uid; int
    // si_status; ... }`, which puts `si_status` at 24.
    // A read at 20 gives `si_uid`: a payload stopped by SIGTERM then
    // reports exit 128 and not 143.
    let status = i32::from_ne_bytes([buf[24], buf[25], buf[26], buf[27]]);
    Ok(Some(Exited { code, status }))
}

pub const POLLIN: i16 = 0x0001;
pub const POLLHUP: i16 = 0x0010;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

/// Wait until one of `fds` is ready, or until `timeout_ms`.
///
/// A supervisor waits on this, not on a sleep and a look. Each wait has an
/// upper bound and a distinct outcome when it gets there: `Ok(0)` is the
/// bound, and it is not an error and not a readiness.
pub fn ppoll(fds: &mut [PollFd], timeout_ms: i64) -> Sysres {
    let ts = Timespec { sec: timeout_ms / 1000, nsec: (timeout_ms % 1000) * 1_000_000 };
    let tsp = if timeout_ms < 0 { 0 } else { &ts as *const Timespec as u64 };
    unsafe {
        sys(
            SYS_PPOLL,
            [
                fds.as_mut_ptr() as u64,
                fds.len() as u64,
                tsp,
                0, // sigmask: none
                0,
                0,
            ],
        )
    }
}

/// The four `statfs` fields this project reads, widened to one shape.
///
/// **The kernel's own `struct statfs64` is the buffer**, taken per
/// architecture from `linux_raw_sys`, and this is a normalised view built from
/// it. The two are different things on purpose: the kernel's layout and field
/// widths differ by architecture, and a hand-written `#[repr(C)]` that is one
/// field short is a kernel write past the end of it.
#[derive(Default, Clone, Copy)]
pub struct Statfs {
    pub f_bsize: i64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
}

/// `flock(2)`. An image store uses it to hold a rootfs against a concurrent
/// garbage collection.
///
/// An advisory lock on an **fd**, not a pid file. The lock is held through
/// exec with its fd inheritable, so the collection of a concurrent process
/// cannot delete the rootfs while it is in use. A pid file is stale the moment a process dies unexpectedly, and the check
/// that clears a stale one is the race this closes.
pub fn flock(fd: i64, op: u64) -> Sysres {
    unsafe { sys(SYS_FLOCK, [fd as u64, op, 0, 0, 0, 0]) }
}

/// `readlink(2)` into a caller-sized buffer, returned as a `String`.
///
/// The kernel does not NUL-terminate and does not report truncation, so a
/// return equal to the buffer size is reported as an error rather than as a
/// value: a silently truncated namespace identity would compare unequal to
/// itself and invalidate a cache on every run.
pub fn readlink(path: &CBuf) -> Result<String, Errno> {
    let mut buf = [0u8; 256];
    let n = unsafe { sys(SYS_READLINKAT, [AT_FDCWD, path.ptr(), buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0])? }
        as usize;
    if n >= buf.len() {
        return Err(ENAMETOOLONG);
    }
    Ok(String::from_utf8_lossy(&buf[..n]).into_owned())
}

pub fn statfs(path: &CBuf) -> Result<Statfs, Errno> {
    // `statfs64` and not `statfs`: on a 32-bit architecture the narrow form
    // truncates a filesystem larger than 4 GiB of blocks, and a free-space
    // check that silently wraps is worse than none. The syscall takes the
    // buffer's size so the kernel refuses a shape it does not know.
    let mut raw = unsafe { core::mem::zeroed::<linux_raw_sys::general::statfs64>() };
    unsafe { sys(SYS_STATFS, [path.ptr(), &mut raw as *mut linux_raw_sys::general::statfs64 as u64, 0, 0, 0, 0])? };
    // As `Stat::from_kernel`: redundant here, load-bearing elsewhere.
    #[allow(clippy::unnecessary_cast)]
    Ok(Statfs {
        f_bsize: raw.f_bsize as i64,
        f_bavail: raw.f_bavail as u64,
        f_files: raw.f_files as u64,
        f_ffree: raw.f_ffree as u64,
    })
}

/// `prlimit64(0, resource, NULL, &old)`: read this process's own limit
/// without changing it.
///
/// As [`Statfs`]: the kernel's own `struct rlimit64` is the buffer, taken
/// per architecture from `linux_raw_sys`, because a hand-written pair of the
/// wrong width is a kernel write past the end of it.
pub fn prlimit(resource: u64) -> Result<(u64, u64), Errno> {
    let mut raw = unsafe { core::mem::zeroed::<linux_raw_sys::general::rlimit64>() };
    unsafe { sys(SYS_PRLIMIT64, [0, resource, 0, &mut raw as *mut linux_raw_sys::general::rlimit64 as u64, 0, 0])? };
    // As `Stat::from_kernel`: redundant on this architecture and
    // load-bearing on one whose `__u64` is not `u64`.
    #[allow(clippy::unnecessary_cast)]
    Ok((raw.rlim_cur as u64, raw.rlim_max as u64))
}

// ------------------------------------------------------- sockets
//
// Some confined hosts answer EPERM to each TCP bind,
// loopback or wildcard, which kills every hostfwd-based manager. The probe
// binds 127.0.0.1 port 0 (the kernel picks the port, so no fixture can
// collide) and listens once; nothing is ever accepted on it.
pub const SYS_SOCKET: i64 = nr!(socket, __NR_socket);
pub const SYS_BIND: i64 = nr!(bind, __NR_bind);
pub const SYS_LISTEN: i64 = nr!(listen, __NR_listen);
/// `AF_INET`, 2 in every Linux `socket.h`. The probe speaks IPv4 loopback.
pub const AF_INET: u64 = 2;
/// `SOCK_STREAM`, 1 in every Linux `socket.h`.
pub const SOCK_STREAM: u64 = 1;
/// `SOCK_CLOEXEC`, `O_CLOEXEC` shifted into the socket type word. Same value,
/// separate name: one is a file-status flag and the other is not.
pub const SOCK_CLOEXEC: u64 = 0o2000000;

pub fn socket(domain: u64, stype: u64, protocol: u64) -> Sysres {
    unsafe { sys(SYS_SOCKET, [domain, stype, protocol, 0, 0, 0]) }
}

pub fn bind(fd: i64, addr: u64, addrlen: u64) -> Sysres {
    unsafe { sys(SYS_BIND, [fd as u64, addr, addrlen, 0, 0, 0]) }
}

pub fn listen(fd: i64, backlog: u64) -> Sysres {
    unsafe { sys(SYS_LISTEN, [fd as u64, backlog, 0, 0, 0, 0]) }
}

/// The `stat` fields this project reads, widened to one shape.
///
/// As [`Statfs`]: **the kernel's own `struct stat` is the buffer**, taken
/// per architecture from `linux_raw_sys`, and this is a normalised view built
/// from it. It is 144 bytes on x86_64 and 128 on aarch64, and a buffer short by
/// one field is a kernel write past the end of it, which is exactly what a
/// hand-written layout ported to a second architecture does.
#[derive(Default, Clone, Copy)]
pub struct Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_rdev: u64,
    pub st_size: i64,
}

impl Stat {
    /// **Every cast below is redundant on x86_64 and load-bearing somewhere
    /// else.** The kernel's field widths differ by architecture, `st_mode` and
    /// `st_uid` are not the same type on every one of them, so clippy's
    /// `unnecessary_cast` is correct for the architecture it was run on and
    /// wrong for this crate. Removing them breaks the cross-build, which is the
    /// only reason this allow is here.
    #[allow(clippy::unnecessary_cast)]
    pub(super) fn from_kernel(raw: &KernelStat) -> Stat {
        Stat {
            st_dev: raw.st_dev as u64,
            st_ino: raw.st_ino as u64,
            st_mode: raw.st_mode as u32,
            st_uid: raw.st_uid as u32,
            st_gid: raw.st_gid as u32,
            st_rdev: raw.st_rdev as u64,
            st_size: raw.st_size as i64,
        }
    }
}

impl Stat {
    /// The file type, as the word `ls -l` would put in column one.
    pub fn kind(&self) -> &'static str {
        match self.st_mode & S_IFMT {
            0o140000 => "socket",
            0o120000 => "symlink",
            0o100000 => "file",
            0o060000 => "blockdev",
            0o040000 => "directory",
            0o020000 => "chardev",
            0o010000 => "fifo",
            _ => "unknown",
        }
    }

    pub fn is_chardev(&self) -> bool {
        self.st_mode & S_IFMT == S_IFCHR as u32
    }

    /// The glibc encoding, which is what the kernel writes: the major and minor
    /// are interleaved rather than a simple high and low half.
    pub fn rdev_major(&self) -> u64 {
        ((self.st_rdev >> 8) & 0xfff) | ((self.st_rdev >> 32) & !0xfff)
    }

    pub fn rdev_minor(&self) -> u64 {
        (self.st_rdev & 0xff) | ((self.st_rdev >> 12) & !0xff)
    }
}

/// `stat(2)` by name, `newfstatat(2)` in fact.
///
/// Identical by definition: `stat(path)` is `newfstatat(AT_FDCWD, path, 0)`.
/// Routing through the `*at` form is what lets this compile on an architecture
/// whose kernel has no `stat` entry point at all, and it changes nothing on one
/// that does. It is a **utility** call and not a probe: nothing in the report
/// says the word `stat`, so the entry point is not the measurement. The three
/// calls where it is are [`CHOWN`], [`LCHOWN`] and [`MKNOD`].
pub fn stat(path: &CBuf) -> Result<Stat, Errno> {
    fstatat(AT_FDCWD as i64, path, 0)
}

pub fn getgroups() -> Result<Vec<i32>, Errno> {
    // NGROUPS_MAX is 65536; ask for the count first so the buffer is the size
    // the kernel says rather than a ceiling this file picked.
    let n = unsafe { sys(SYS_GETGROUPS, [0, 0, 0, 0, 0, 0])? };
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0i32; n as usize];
    let got = unsafe { sys(SYS_GETGROUPS, [n as u64, buf.as_mut_ptr() as u64, 0, 0, 0, 0])? };
    buf.truncate(got as usize);
    Ok(buf)
}
