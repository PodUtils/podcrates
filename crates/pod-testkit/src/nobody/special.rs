//! The callers that only root can make for a test (T-1641, T-1635): uid 0
//! of a user namespace of a range of ids, a user with a capability in its
//! ambient set, and pid 1 of a PID namespace over the host's proc.

use std::os::unix::process::CommandExt;

use super::{NOBODY, exit_with, is_root, os, pid_one_over_a_fresh_proc};

/// The first outer id of the range of [`again_as_root_of_a_range`].
pub const RANGE_START: u32 = 200_000;
/// The count of ids of that range, as `newuidmap` gives a user.
pub const RANGE_COUNT: u32 = 65_536;

/// `_LINUX_CAPABILITY_VERSION_3`.
const CAPABILITY_VERSION_3: u32 = 0x2008_0522;

#[repr(C)]
struct CapHeader {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

fn run(command: &mut std::process::Command, what: &str) -> Result<i32, String> {
    let status = command.status().map_err(|e| format!("this test binary {what}: {e}"))?;
    Ok(status.code().unwrap_or(128))
}

fn command(args: &[&str], env: &[(&str, &str)]) -> Result<std::process::Command, String> {
    if !is_root() {
        return Err("this caller needs root to make it".to_string());
    }
    let exe = std::env::current_exe().map_err(|e| format!("the path of this test binary: {e}"))?;
    let mut command = std::process::Command::new(&exe);
    command.args(args).envs(env.iter().copied());
    Ok(command)
}

/// As root only: start this test binary again as uid and gid 0 of a user
/// namespace that maps [`RANGE_COUNT`] ids from [`RANGE_START`], as pid 1 of
/// its own mount and PID namespaces over a fresh proc. The kernel takes a
/// map of more than one id only from a writer with `CAP_SETUID` over the
/// parent namespace: this process writes it, and the child waits for it.
pub fn again_as_root_of_a_range(args: &[&str], env: &[(&str, &str)]) -> Result<i32, String> {
    let mut command = command(args, env)?;
    let (mut ask, mut go) = ([0i32; 2], [0i32; 2]);
    os(unsafe { libc::pipe2(ask.as_mut_ptr(), libc::O_CLOEXEC) }).map_err(|e| e.to_string())?;
    os(unsafe { libc::pipe2(go.as_mut_ptr(), libc::O_CLOEXEC) }).map_err(|e| e.to_string())?;
    let (ask_w, go_r) = (ask[1], go[0]);
    // SAFETY: the closure runs in the child between the fork and the exec,
    // and makes only calls of the C library with no allocation.
    unsafe { command.pre_exec(move || ranged(ask_w, go_r)) };
    let (ask_r, go_w) = (ask[0], go[1]);
    let writer = std::thread::spawn(move || write_range_maps(ask_r, go_w));
    let child = command.spawn();
    unsafe {
        libc::close(ask_w);
        libc::close(go_r);
    }
    let _ = writer.join();
    let status = child.and_then(|mut c| c.wait()).map_err(|e| format!("this test binary in a range: {e}"))?;
    Ok(status.code().unwrap_or(128))
}

/// In this process: the pid that the child sends on `ask`, its two maps,
/// then one byte on `go`. Each descriptor closes here.
fn write_range_maps(ask: i32, go: i32) {
    let mut pid = [0u8; 4];
    let got = unsafe { libc::read(ask, pid.as_mut_ptr().cast(), 4) };
    if got == 4 {
        let pid = i32::from_ne_bytes(pid);
        let line = format!("0 {RANGE_START} {RANGE_COUNT}");
        let wrote = ["uid_map", "gid_map"].iter().all(|m| std::fs::write(format!("/proc/{pid}/{m}"), &line).is_ok());
        if wrote {
            unsafe { libc::write(go, [1u8].as_ptr().cast(), 1) };
        }
    }
    unsafe {
        libc::close(ask);
        libc::close(go);
    }
}

/// The child of [`again_as_root_of_a_range`]: the user namespace, the ask
/// for its maps, the ids 0, then the mount and PID namespaces and a fork.
fn ranged(ask: i32, go: i32) -> std::io::Result<()> {
    os(unsafe { libc::unshare(libc::CLONE_NEWUSER) })?;
    let pid = unsafe { libc::getpid() }.to_ne_bytes();
    os(unsafe { libc::write(ask, pid.as_ptr().cast(), 4) })?;
    let mut b = 0u8;
    if unsafe { libc::read(go, (&mut b as *mut u8).cast(), 1) } != 1 {
        return Err(std::io::Error::other("the maps of the range were not written"));
    }
    os(unsafe { libc::setresgid(0, 0, 0) })?;
    os(unsafe { libc::setresuid(0, 0, 0) })?;
    os(unsafe { libc::setgroups(0, std::ptr::null()) })?;
    os(unsafe { libc::unshare(libc::CLONE_NEWNS | libc::CLONE_NEWPID) })?;
    pid_one_over_a_fresh_proc()
}

/// As root only: start this test binary again as [`NOBODY`] with no
/// supplementary groups, with the capability `cap` in its effective,
/// permitted, inheritable and ambient sets: the ambient set crosses the
/// exec of a program for a uid that is not 0.
pub fn again_with_ambient(cap: u32, args: &[&str], env: &[(&str, &str)]) -> Result<i32, String> {
    let mut command = command(args, env)?;
    // SAFETY: as above.
    unsafe { command.pre_exec(move || ambient(cap)) };
    run(&mut command, "with an ambient capability")
}

fn ambient(cap: u32) -> std::io::Result<()> {
    os(unsafe { libc::prctl(libc::PR_SET_KEEPCAPS, 1, 0, 0, 0) })?;
    os(unsafe { libc::setgroups(0, std::ptr::null()) })?;
    os(unsafe { libc::setresgid(NOBODY, NOBODY, NOBODY) })?;
    os(unsafe { libc::setresuid(NOBODY, NOBODY, NOBODY) })?;
    let header = CapHeader { version: CAPABILITY_VERSION_3, pid: 0 };
    let mut data = [CapData::default(); 2];
    let bit = 1u32 << (cap % 32);
    data[(cap / 32) as usize] = CapData { effective: bit, permitted: bit, inheritable: bit };
    os(unsafe { libc::syscall(libc::SYS_capset, &header as *const CapHeader, data.as_ptr()) })?;
    let raise = libc::PR_CAP_AMBIENT_RAISE as libc::c_ulong;
    os(unsafe { libc::prctl(libc::PR_CAP_AMBIENT, raise, libc::c_ulong::from(cap), 0, 0) })?;
    Ok(())
}

/// As root only: start this test binary again as root and pid 1 of a PID
/// namespace of its own, with the proc of the host on `/proc`: `/proc/self`
/// there names a pid that is not the one of `getpid`.
pub fn again_in_a_pid_namespace(args: &[&str], env: &[(&str, &str)]) -> Result<i32, String> {
    let mut command = command(args, env)?;
    // SAFETY: as above.
    unsafe {
        command.pre_exec(|| {
            os(libc::unshare(libc::CLONE_NEWPID))?;
            let pid = os(libc::fork())?;
            if pid == 0 { Ok(()) } else { exit_with(pid) }
        })
    };
    run(&mut command, "in a PID namespace over the host's proc")
}
