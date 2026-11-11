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
    // On a 32-bit target `long` is 4 bytes, there is no padding, the union
    // starts at 12 and `si_status` is at 20.
    let at = if cfg!(target_pointer_width = "64") { 24 } else { 20 };
    let status = i32::from_ne_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]);
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
    // The kernel's `timespec` has two `long` words: 32 bits on a 32-bit target.
    sec: isize,
    nsec: isize,
}

/// Wait until one of `fds` is ready, or until `timeout_ms`.
///
/// A supervisor waits on this, not on a sleep and a look. Each wait has an
/// upper bound and a distinct outcome when it gets there: `Ok(0)` is the
/// bound, and it is not an error and not a readiness.
pub fn ppoll(fds: &mut [PollFd], timeout_ms: i64) -> Sysres {
    let ts = Timespec { sec: (timeout_ms / 1000) as isize, nsec: ((timeout_ms % 1000) * 1_000_000) as isize };
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

/// The five `statfs` fields this project reads, widened to one shape.
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
    /// The mount flags (`ST_RDONLY`, `ST_NOSUID`, `ST_NODEV`, `ST_NOEXEC`,
    /// ...). The kernel sets `ST_VALID` beside them where it fills this
    /// field; a value without `ST_VALID` tells nothing.
    pub f_flags: u64,
}

impl Statfs {
    /// True where the kernel filled `f_flags` and `flag` is set in it.
    /// `None` where the kernel did not fill the field (no `ST_VALID`).
    pub fn flag(&self, flag: u64) -> Option<bool> {
        (self.f_flags & ST_VALID != 0).then_some(self.f_flags & flag != 0)
    }
}

/// The mount flags of `Statfs::f_flags`, as the kernel defines them
/// (`include/linux/statfs.h`) on each architecture.
pub const ST_RDONLY: u64 = 0x0001;
pub const ST_NOSUID: u64 = 0x0002;
pub const ST_NODEV: u64 = 0x0004;
pub const ST_NOEXEC: u64 = 0x0008;
pub const ST_SYNCHRONOUS: u64 = 0x0010;
/// Set by the kernel where it filled `f_flags`.
pub const ST_VALID: u64 = 0x0020;
pub const ST_MANDLOCK: u64 = 0x0040;
pub const ST_NOATIME: u64 = 0x0400;
pub const ST_NODIRATIME: u64 = 0x0800;
pub const ST_RELATIME: u64 = 0x1000;

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
    let buf = &mut raw as *mut linux_raw_sys::general::statfs64 as u64;
    // A 64-bit kernel has one form, whose struct is the wide one.
    #[cfg(target_pointer_width = "64")]
    unsafe {
        sys(SYS_STATFS, [path.ptr(), buf, 0, 0, 0, 0])?
    };
    #[cfg(target_pointer_width = "32")]
    unsafe {
        let size = core::mem::size_of::<linux_raw_sys::general::statfs64>() as u64;
        sys(crate::sys::SYS_STATFS64, [path.ptr(), size, buf, 0, 0, 0])?
    };
    // As `Stat::from_kernel`: redundant here, load-bearing elsewhere.
    #[allow(clippy::unnecessary_cast)]
    Ok(Statfs {
        f_bsize: raw.f_bsize as i64,
        f_bavail: raw.f_bavail as u64,
        f_files: raw.f_files as u64,
        f_ffree: raw.f_ffree as u64,
        // A signed `long` on most 64-bit targets, a `u32` on the 32-bit
        // ones and on s390x: the flags are small and positive on each.
        f_flags: raw.f_flags as u64,
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

/// Make this process the child subreaper of its descendants: a descendant
/// whose parent ends becomes a child of this process, not of init. A
/// supervisor then reaches each process of its tree, also one that called
/// `setsid` or forked twice.
pub fn set_child_subreaper() -> Sysres {
    unsafe { sys(SYS_PRCTL, [PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0, 0]) }
}

/// `wait4(-1, WNOHANG)`: reap one child of any pid that ended, with no
/// wait. `Ok(Some(pid))` with its status; `Ok(None)` when each child is
/// alive. `Err(ECHILD)` when this process has no child at all: that answer
/// is the only one that says "none is left".
pub fn reap_any(status: &mut i32) -> Result<Option<i64>, Errno> {
    match unsafe { sys(SYS_WAIT4, [-1i64 as u64, status as *mut i32 as u64, WNOHANG, 0, 0, 0]) }? {
        0 => Ok(None),
        pid => Ok(Some(pid)),
    }
}

/// The use of the reaped children of this process and of their reaped
/// descendants (`getrusage(RUSAGE_CHILDREN)`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub user_us: u64,
    pub system_us: u64,
    /// The peak resident set of the largest one process, in KiB. It is not
    /// a sum over the tree.
    pub max_rss_kib: u64,
}

/// `getrusage(RUSAGE_CHILDREN)`. As [`Statfs`]: the kernel's own
/// `struct rusage` is the buffer, from `linux_raw_sys`, because its words
/// are 32 bits on a 32-bit target.
pub fn rusage_children() -> Result<Usage, Errno> {
    let mut raw = unsafe { core::mem::zeroed::<linux_raw_sys::general::rusage>() };
    let who = linux_raw_sys::general::RUSAGE_CHILDREN as i64 as u64;
    unsafe { sys(SYS_GETRUSAGE, [who, &mut raw as *mut linux_raw_sys::general::rusage as u64, 0, 0, 0, 0])? };
    // As `Stat::from_kernel`: redundant on a 64-bit target.
    #[allow(clippy::unnecessary_cast)]
    let us = |t: linux_raw_sys::general::__kernel_old_timeval| (t.tv_sec as u64) * 1_000_000 + t.tv_usec as u64;
    #[allow(clippy::unnecessary_cast)]
    Ok(Usage { user_us: us(raw.ru_utime), system_us: us(raw.ru_stime), max_rss_kib: raw.ru_maxrss as u64 })
}

/// `umask(2)`: set the file mode mask of this process and give the old
/// one. The call cannot fail.
pub fn umask(mask: u32) -> u32 {
    unsafe { sys(SYS_UMASK, [u64::from(mask), 0, 0, 0, 0, 0]) }.unwrap_or(0) as u32
}

/// The `RLIMIT_*` numbers of the kernel of this architecture: mips and
/// sparc number some of them differently, so each comes from
/// `linux_raw_sys`.
pub mod rlimit {
    use linux_raw_sys::general as g;
    pub const CPU: u64 = g::RLIMIT_CPU as u64;
    pub const FSIZE: u64 = g::RLIMIT_FSIZE as u64;
    pub const DATA: u64 = g::RLIMIT_DATA as u64;
    pub const STACK: u64 = g::RLIMIT_STACK as u64;
    pub const CORE: u64 = g::RLIMIT_CORE as u64;
    pub const NOFILE: u64 = g::RLIMIT_NOFILE as u64;
    pub const NPROC: u64 = g::RLIMIT_NPROC as u64;
    pub const AS: u64 = g::RLIMIT_AS as u64;
    pub const MEMLOCK: u64 = g::RLIMIT_MEMLOCK as u64;
    pub const MSGQUEUE: u64 = g::RLIMIT_MSGQUEUE as u64;
    pub const NICE: u64 = g::RLIMIT_NICE as u64;
    pub const RTPRIO: u64 = g::RLIMIT_RTPRIO as u64;
    pub const LOCKS: u64 = g::RLIMIT_LOCKS as u64;
    pub const SIGPENDING: u64 = g::RLIMIT_SIGPENDING as u64;
    pub const RTTIME: u64 = g::RLIMIT_RTTIME as u64;
    /// `RLIM64_INFINITY`: no limit.
    pub const INFINITY: u64 = u64::MAX;
}

/// `prlimit64(0, resource, &new, NULL)`: set a limit of this process. One
/// system call on a struct on the stack, so a child between its fork and
/// its exec can call it.
pub fn set_rlimit(resource: u64, soft: u64, hard: u64) -> Sysres {
    let mut new = unsafe { core::mem::zeroed::<linux_raw_sys::general::rlimit64>() };
    new.rlim_cur = soft as _;
    new.rlim_max = hard as _;
    unsafe { sys(SYS_PRLIMIT64, [0, resource, &new as *const linux_raw_sys::general::rlimit64 as u64, 0, 0, 0]) }
}

/// `prctl(PR_SET_NO_NEW_PRIVS, 1)`: no later `execve` gives privileges.
/// It cannot be undone, so a supervisor sets it in the child only.
pub fn set_no_new_privs() -> Sysres {
    unsafe { sys(SYS_PRCTL, [PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0, 0]) }
}
