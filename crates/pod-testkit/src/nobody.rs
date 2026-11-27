//! A test as a caller with no capabilities. As root, the test starts its
//! own binary again as uid and gid [`NOBODY`] with no supplementary groups:
//! the kernel clears each capability at the change of the uid. As another
//! user, the test runs in its own process. [`again_over_a_bare_proc`] also
//! gives the test a proc with no flags of the host, and
//! [`again_over_a_bare_dev`] (root only) a `/dev` with no `nosuid` and no
//! `noexec`; [`again_as_root_of_a_user_namespace`] makes it uid 0 of a user
//! namespace of its own; [`again_over_a_flagged_tmpfs`] gives it a tmpfs with
//! `nosuid`, `nodev`, `noexec` and `noatime`.

use std::os::unix::process::CommandExt;

/// The uid and the gid of the caller with no capabilities.
pub const NOBODY: u32 = 65534;

/// Is the effective uid of this process 0?
pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// The effective capabilities of this process (`CapEff` of
/// `/proc/self/status`), or `None` where the file has no such line.
pub fn effective_caps() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let hex = status.lines().find_map(|l| l.strip_prefix("CapEff:"))?;
    u64::from_str_radix(hex.trim(), 16).ok()
}

/// Start this test binary again with `args` and `env` as [`NOBODY`], with
/// no supplementary groups (std calls `setgroups(0)` before `setuid` as
/// root), and give its exit code. `Err` where it cannot start: [`NOBODY`]
/// must be able to execute the binary through each directory of its path.
pub fn again(args: &[&str], env: &[(&str, &str)]) -> Result<i32, String> {
    let exe = std::env::current_exe().map_err(|e| format!("the path of this test binary: {e}"))?;
    let status = std::process::Command::new(&exe)
        .args(args)
        .envs(env.iter().copied())
        .uid(NOBODY)
        .gid(NOBODY)
        .status()
        .map_err(|e| format!("{} could not start as uid {NOBODY}: {e}", exe.display()))?;
    Ok(status.code().unwrap_or(128))
}

/// Start this test binary again as [`again`] does, as pid 1 of its own user,
/// mount and PID namespaces, over a fresh proc on `/proc` with no
/// `nosuid`, `nodev` or `noexec`. The user namespace maps the uid and the
/// gid of the caller ([`NOBODY`] where it is root) to themselves, so the
/// exec clears each capability. A check of the flags that the code under
/// test sets on a bind of `/proc` then sees them on each host: a bind keeps
/// the flags of the host's proc, and the proc of most hosts has them
/// already. `Err` where the kernel refuses a namespace or the proc.
pub fn again_over_a_bare_proc(args: &[&str], env: &[(&str, &str)]) -> Result<i32, String> {
    again_in_a_user_namespace(args, env, false)
}

/// Start this test binary again as [`again_over_a_bare_proc`] does, but as
/// uid and gid 0 of its user namespace: the maps are `0 <uid> 1` and
/// `0 <gid> 1`, as podvm's foreground run makes them. The binary holds each
/// capability in that namespace, which owns its PID namespace and its
/// proc; the namespace is not the initial one.
pub fn again_as_root_of_a_user_namespace(args: &[&str], env: &[(&str, &str)]) -> Result<i32, String> {
    again_in_a_user_namespace(args, env, true)
}

fn again_in_a_user_namespace(args: &[&str], env: &[(&str, &str)], as_zero: bool) -> Result<i32, String> {
    let exe = std::env::current_exe().map_err(|e| format!("the path of this test binary: {e}"))?;
    let (uid, gid) = if is_root() { (NOBODY, NOBODY) } else { unsafe { (libc::geteuid(), libc::getegid()) } };
    let (inside_uid, inside_gid) = if as_zero { (0, 0) } else { (uid, gid) };
    let uid_map = format!("{inside_uid} {uid} 1");
    let gid_map = format!("{inside_gid} {gid} 1");
    let mut command = std::process::Command::new(&exe);
    command.args(args).envs(env.iter().copied());
    if is_root() {
        command.uid(NOBODY).gid(NOBODY);
    }
    // SAFETY: the closure runs in the child between the fork and the exec,
    // and makes only calls of the C library with no allocation.
    unsafe { command.pre_exec(move || bare_proc(uid_map.as_bytes(), gid_map.as_bytes())) };
    let status = command.status().map_err(|e| format!("{} over a fresh proc as uid {uid}: {e}", exe.display()))?;
    Ok(status.code().unwrap_or(128))
}

/// The options of the top mount on `path` in `/proc/self/mountinfo` (the
/// flags of that mount), or an empty string where no mount is on `path`.
pub fn mount_options(path: &str) -> String {
    let info = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let top = info.lines().rfind(|l| l.split(' ').nth(4) == Some(path));
    top.and_then(|l| l.split(' ').nth(5)).unwrap_or_default().to_string()
}

/// As root only: start this test binary again with `args` and `env` as
/// [`NOBODY`] with no supplementary groups, in a private mount namespace
/// of its own where the mount on `/dev` has no `nosuid` and no `noexec`
/// (the other flags stay). A check of the flags that the code under test
/// sets on a bind of a device then sees them on each host: a bind keeps the
/// flags of the host's `/dev`, and a user namespace cannot clear them. The
/// mounts of the host do not change. `Err` where this process is not root,
/// or where the kernel refuses the namespace or the remount.
pub fn again_over_a_bare_dev(args: &[&str], env: &[(&str, &str)]) -> Result<i32, String> {
    if !is_root() {
        return Err("a /dev with no nosuid and no noexec needs root".to_string());
    }
    let exe = std::env::current_exe().map_err(|e| format!("the path of this test binary: {e}"))?;
    let mut command = std::process::Command::new(&exe);
    command.args(args).envs(env.iter().copied());
    // SAFETY: the closure runs in the child between the fork and the exec,
    // and makes only calls of the C library with no allocation.
    unsafe { command.pre_exec(bare_dev) };
    let status = command.status().map_err(|e| format!("{} over a bare /dev as uid {NOBODY}: {e}", exe.display()))?;
    Ok(status.code().unwrap_or(128))
}

/// The child of [`again_over_a_bare_dev`], as root: the mount namespace and
/// the remount of `/dev`, then the change to [`NOBODY`]. std changes the uid
/// before this closure, so the closure changes it itself, after the mounts.
fn bare_dev() -> std::io::Result<()> {
    let none = std::ptr::null();
    os(unsafe { libc::unshare(libc::CLONE_NEWNS) })?;
    os(unsafe { libc::mount(none, c"/".as_ptr(), none, libc::MS_REC | libc::MS_PRIVATE, none.cast()) })?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    os(unsafe { libc::statvfs(c"/dev".as_ptr(), &mut st) })?;
    // MS_BIND with MS_REMOUNT sets the flags of this mount alone; the kernel
    // keeps its atime flags.
    let mut flags = libc::MS_REMOUNT | libc::MS_BIND;
    for (st_flag, ms_flag) in [(libc::ST_RDONLY, libc::MS_RDONLY), (libc::ST_NODEV, libc::MS_NODEV)] {
        if st.f_flag & st_flag != 0 {
            flags |= ms_flag;
        }
    }
    os(unsafe { libc::mount(none, c"/dev".as_ptr(), none, flags, none.cast()) })?;
    os(unsafe { libc::setgroups(0, std::ptr::null()) })?;
    os(unsafe { libc::setresgid(NOBODY, NOBODY, NOBODY) })?;
    os(unsafe { libc::setresuid(NOBODY, NOBODY, NOBODY) })?;
    Ok(())
}

/// Start this test binary again with `args` and `env` in a mount namespace
/// of its own, where a fresh tmpfs with `nosuid`, `nodev`, `noexec` and
/// `noatime` (mode 0777) is on `dir`, a directory that exists. A test of a
/// bind then sees the flags of its source mount on each host. As root: the
/// binary runs as root where `as_root`, else as [`NOBODY`] with no
/// supplementary groups. As another user: in a user namespace of its own
/// that maps the uid and the gid of the caller to themselves, so the exec
/// clears each capability; `as_root` is then an `Err`. The mounts of the
/// host do not change.
pub fn again_over_a_flagged_tmpfs(
    dir: &std::path::Path,
    as_root: bool,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<i32, String> {
    if as_root && !is_root() {
        return Err("a run as root over a flagged tmpfs needs root".to_string());
    }
    let exe = std::env::current_exe().map_err(|e| format!("the path of this test binary: {e}"))?;
    let target = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()).map_err(|e| format!("{e}"))?;
    let maps = (!is_root()).then(|| {
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        (format!("{uid} {uid} 1"), format!("{gid} {gid} 1"))
    });
    let to_nobody = is_root() && !as_root;
    let mut command = std::process::Command::new(&exe);
    command.args(args).envs(env.iter().copied());
    // SAFETY: the closure runs in the child between the fork and the exec,
    // and makes only calls of the C library with no allocation.
    unsafe {
        command.pre_exec(move || {
            let maps = maps.as_ref().map(|(u, g)| (u.as_bytes(), g.as_bytes()));
            flagged_tmpfs(&target, maps, to_nobody)
        })
    };
    let status = command.status().map_err(|e| format!("{} over a flagged tmpfs: {e}", exe.display()))?;
    Ok(status.code().unwrap_or(128))
}

/// The child of [`again_over_a_flagged_tmpfs`]: the namespaces (a user
/// namespace where `maps` gives its two lines), the tmpfs, then the change
/// to [`NOBODY`] where `to_nobody`.
fn flagged_tmpfs(dir: &std::ffi::CStr, maps: Option<(&[u8], &[u8])>, to_nobody: bool) -> std::io::Result<()> {
    let none = std::ptr::null();
    match maps {
        Some((uid_map, gid_map)) => {
            os(unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) })?;
            put(b"/proc/self/setgroups\0", b"deny")?;
            put(b"/proc/self/uid_map\0", uid_map)?;
            put(b"/proc/self/gid_map\0", gid_map)?;
        }
        None => drop(os(unsafe { libc::unshare(libc::CLONE_NEWNS) })?),
    }
    os(unsafe { libc::mount(none, c"/".as_ptr(), none, libc::MS_REC | libc::MS_PRIVATE, none.cast()) })?;
    let flags = libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC | libc::MS_NOATIME;
    let tmpfs = c"tmpfs".as_ptr();
    os(unsafe { libc::mount(tmpfs, dir.as_ptr(), tmpfs, flags, c"mode=0777".as_ptr().cast()) })?;
    if to_nobody {
        os(unsafe { libc::setgroups(0, std::ptr::null()) })?;
        os(unsafe { libc::setresgid(NOBODY, NOBODY, NOBODY) })?;
        os(unsafe { libc::setresuid(NOBODY, NOBODY, NOBODY) })?;
    }
    Ok(())
}

fn os<T: PartialEq + From<i8>>(r: T) -> std::io::Result<T> {
    if r == T::from(-1) { Err(std::io::Error::last_os_error()) } else { Ok(r) }
}

/// Write `data` to the file `path` (a C string).
fn put(path: &[u8], data: &[u8]) -> std::io::Result<()> {
    let fd = os(unsafe { libc::open(path.as_ptr().cast(), libc::O_WRONLY | libc::O_CLOEXEC) })?;
    let wrote = os(unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) });
    unsafe { libc::close(fd) };
    wrote.map(drop)
}

/// The child of [`again_over_a_bare_proc`]: the namespaces, then a fork.
/// The grandchild is pid 1 of the new PID namespace: it mounts the proc
/// and goes on to the exec; this process waits for it and exits with its
/// code.
fn bare_proc(uid_map: &[u8], gid_map: &[u8]) -> std::io::Result<()> {
    // The change of the uid made the process not dumpable: its files of
    // /proc then belong to root, and it cannot write its own maps.
    os(unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0) })?;
    os(unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS | libc::CLONE_NEWPID) })?;
    put(b"/proc/self/setgroups\0", b"deny")?;
    put(b"/proc/self/uid_map\0", uid_map)?;
    put(b"/proc/self/gid_map\0", gid_map)?;
    pid_one_over_a_fresh_proc()
}

/// A fork: the child (pid 1 of the PID namespace of an `unshare` before)
/// makes `/` private, mounts a fresh proc on `/proc` and goes on to the
/// exec; this process waits for it and exits with its code.
fn pid_one_over_a_fresh_proc() -> std::io::Result<()> {
    let pid = os(unsafe { libc::fork() })?;
    if pid == 0 {
        let none = std::ptr::null();
        os(unsafe { libc::mount(none, c"/".as_ptr(), none, libc::MS_REC | libc::MS_PRIVATE, none.cast()) })?;
        let proc = c"proc".as_ptr();
        os(unsafe { libc::mount(proc, c"/proc".as_ptr(), proc, 0, none.cast()) })?;
        return Ok(());
    }
    exit_with(pid)
}

/// Wait for the child `pid` and exit with its code (128 + the signal).
fn exit_with(pid: libc::pid_t) -> ! {
    let mut status = 0;
    while unsafe { libc::waitpid(pid, &mut status, 0) } == -1 {
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            unsafe { libc::_exit(127) }
        }
    }
    let code = if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 128 + libc::WTERMSIG(status) };
    unsafe { libc::_exit(code) }
}

#[path = "nobody/special.rs"]
mod special;
pub use special::{RANGE_COUNT, RANGE_START, again_as_root_of_a_range, again_in_a_pid_namespace, again_with_ambient};
