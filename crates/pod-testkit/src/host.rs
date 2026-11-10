//! The host of a test can refuse
//! `bind(AF_INET)`, have no loopback interface, have no resolution of
//! `localhost`, refuse a user and a mount namespace, or have no tool that a
//! test runs. A test that needs one of these skips on such a host, with a
//! line `skipping, REASON` on standard error. It does not fail, and it does
//! not wait for a time limit.
//!
//! Each probe of the host runs one time for each test binary.
//!
//! Use it at the start of a test: `pod_testkit::need_loopback!();`.

use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::OnceLock;
use std::time::Duration;

/// The time limit of each step of a probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// `Err(reason)` when TCP or UDP on 127.0.0.1 does not work: a bind is
/// refused, or a connect or a datagram does not arrive (no loopback).
pub fn loopback() -> Result<(), String> {
    static PROBE: OnceLock<Result<(), String>> = OnceLock::new();
    PROBE.get_or_init(probe_loopback).clone()
}

/// `Err(reason)` when the system resolver gives no address for
/// `localhost`.
pub fn localhost() -> Result<(), String> {
    static PROBE: OnceLock<Result<(), String>> = OnceLock::new();
    PROBE.get_or_init(probe_localhost).clone()
}

/// The factor of a time bound under an emulator.
const EMULATOR_FACTOR: u32 = 5;

/// An emulator (`qemu-user`, by its variables or by `binfmt_misc`) runs
/// this test binary. Linux only: another target is never emulated here.
/// Each probe of the detection runs one time for each test binary.
pub fn emulated() -> bool {
    #[cfg(target_os = "linux")]
    {
        static PROBE: OnceLock<bool> = OnceLock::new();
        return *PROBE.get_or_init(|| {
            let set = |v: &str| std::env::var_os(v).is_some();
            pod_hostprobe::interp::detect(&set).is_emulated()
        });
    }
    #[allow(unreachable_code)]
    false
}

/// A time bound that measures speed, not correctness: `d` on a kernel,
/// five times `d` under an emulator. Do not use it for a timer that the
/// test checks (an idle limit): scale only the bound that guards a hang.
pub fn scaled(d: Duration) -> Duration {
    if emulated() { d * EMULATOR_FACTOR } else { d }
}

fn probe_loopback() -> Result<(), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("this host denies bind(AF_INET) on 127.0.0.1 (TCP): {e}"))?;
    let addr = listener.local_addr().map_err(|e| format!("this host gives no address of a TCP socket: {e}"))?;
    TcpStream::connect_timeout(&addr, PROBE_TIMEOUT)
        .map_err(|e| format!("this host has no loopback: TCP connect to {addr}: {e}"))?;

    let bind_udp = || {
        UdpSocket::bind("127.0.0.1:0").map_err(|e| format!("this host denies bind(AF_INET) on 127.0.0.1 (UDP): {e}"))
    };
    let (sender, receiver) = (bind_udp()?, bind_udp()?);
    let to: SocketAddr =
        receiver.local_addr().map_err(|e| format!("this host gives no address of a UDP socket: {e}"))?;
    let no_loopback = |e: std::io::Error| format!("this host has no loopback: UDP to {to}: {e}");
    receiver.set_read_timeout(Some(PROBE_TIMEOUT)).map_err(no_loopback)?;
    sender.send_to(b"probe", to).map_err(no_loopback)?;
    let mut buf = [0u8; 8];
    receiver.recv_from(&mut buf).map_err(no_loopback)?;
    Ok(())
}

fn probe_localhost() -> Result<(), String> {
    match ("localhost", 0).to_socket_addrs() {
        Ok(addrs) if addrs.len() > 0 => Ok(()),
        Ok(_) => Err("this host resolves localhost to no address".into()),
        Err(e) => Err(format!("this host does not resolve localhost: {e}")),
    }
}

/// Write `skipping, REASON` to the standard error of the process. The
/// test harness does not capture this write (it captures `eprintln!`), so
/// the line shows for a test that passes too.
pub fn skip_line(why: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "skipping, {why}");
}

/// Skip the test (return from it) when [`loopback`] fails. Give a value
/// for a test that returns one: `need_loopback!(Ok(()))`.
#[macro_export]
macro_rules! need_loopback {
    () => {
        $crate::need_loopback!(())
    };
    ($ret:expr) => {
        if let Err(why) = $crate::host::loopback() {
            $crate::host::skip_line(&why);
            return $ret;
        }
    };
}

/// `Err(reason)` when this process cannot make a user and a mount namespace
/// and mount a tmpfs in them. The probe forks one child, which calls
/// `unshare(CLONE_NEWUSER | CLONE_NEWNS)`, maps its own ids to 0, mounts a
/// tmpfs on `/tmp` inside the new namespace and exits. No mount of the host
/// changes. On a target other than Linux it is always `Err`.
pub fn userns() -> Result<(), String> {
    static PROBE: OnceLock<Result<(), String>> = OnceLock::new();
    PROBE.get_or_init(probe_userns).clone()
}

/// `Err(reason)` when `tool` is not a file that can execute. A name with a
/// `/` is a path; another name is looked up in `PATH`.
pub fn tool(tool: &str) -> Result<(), String> {
    let found = if tool.contains('/') {
        executable(std::path::Path::new(tool))
    } else {
        std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).any(|dir| executable(&dir.join(tool))))
            .unwrap_or(false)
    };
    if found { Ok(()) } else { Err(format!("this host has no {tool} that can execute")) }
}

/// `Err(reason)` when the effective user of this process is not root: a
/// test that starts a child as another user needs root. On a target other
/// than Linux it is always `Err`.
pub fn root() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let euid = unsafe { libc::geteuid() };
        if euid == 0 { Ok(()) } else { Err(format!("this test needs root, and the effective uid is {euid}")) }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("this test needs root on Linux".into())
    }
}

fn executable(path: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

#[cfg(not(target_os = "linux"))]
fn probe_userns() -> Result<(), String> {
    Err("user and mount namespaces are Linux only".into())
}

#[cfg(target_os = "linux")]
fn probe_userns() -> Result<(), String> {
    use std::ffi::CString;
    // Each value that the child uses exists before the fork: between the
    // fork and the exit the child calls only async-signal-safe functions.
    let uid_map = format!("0 {} 1\n", unsafe { libc::getuid() });
    let gid_map = format!("0 {} 1\n", unsafe { libc::getgid() });
    let c = |s: &str| CString::new(s).expect("no NUL");
    let (uid_path, gid_path, groups_path) =
        (c("/proc/self/uid_map"), c("/proc/self/gid_map"), c("/proc/self/setgroups"));
    let (tmpfs, tmp) = (c("tmpfs"), c("/tmp"));
    let write = |path: &CString, text: &[u8]| -> bool {
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC) };
        if fd < 0 {
            return false;
        }
        let n = unsafe { libc::write(fd, text.as_ptr().cast(), text.len()) };
        unsafe { libc::close(fd) };
        n == text.len() as isize
    };
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(format!("this host refuses fork: {}", std::io::Error::last_os_error()));
    }
    if pid == 0 {
        let drive = || -> i32 {
            if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) } != 0 {
                return 1;
            }
            if !write(&uid_path, uid_map.as_bytes()) {
                return 2;
            }
            // `setgroups` must read `deny` before a write of `gid_map`; a
            // kernel with no such file needs no write.
            let _ = write(&groups_path, b"deny");
            if !write(&gid_path, gid_map.as_bytes()) {
                return 3;
            }
            if unsafe { libc::mount(tmpfs.as_ptr(), tmp.as_ptr(), tmpfs.as_ptr(), 0, std::ptr::null()) } != 0 {
                return 4;
            }
            0
        };
        let code = drive();
        unsafe { libc::_exit(code) };
    }
    let mut status = 0;
    if unsafe { libc::waitpid(pid, &mut status, 0) } != pid {
        return Err(format!("the probe child of the namespaces was lost: {}", std::io::Error::last_os_error()));
    }
    match (libc::WIFEXITED(status), libc::WEXITSTATUS(status)) {
        (true, 0) => Ok(()),
        (true, 1) => Err("this host refuses unshare(CLONE_NEWUSER | CLONE_NEWNS)".into()),
        (true, 2) => Err("this host refuses a write of /proc/self/uid_map in a new user namespace".into()),
        (true, 3) => Err("this host refuses a write of /proc/self/gid_map in a new user namespace".into()),
        (true, 4) => Err("this host refuses a tmpfs mount in a new user and mount namespace".into()),
        _ => Err(format!("the probe child of the namespaces ended with status {status}")),
    }
}

/// Skip the test (return from it) when [`localhost`] fails.
#[macro_export]
macro_rules! need_localhost {
    () => {
        if let Err(why) = $crate::host::localhost() {
            $crate::host::skip_line(&why);
            return;
        }
    };
}

/// Skip the test (return from it) when [`userns`] fails: the test needs a
/// user and a mount namespace.
#[macro_export]
macro_rules! need_userns {
    () => {
        if let Err(why) = $crate::host::userns() {
            $crate::host::skip_line(&why);
            return;
        }
    };
}

/// Skip the test (return from it) when [`tool`] fails for the tool: a path
/// (`need_tool!("/bin/sh")`) or a name in `PATH` (`need_tool!("qemu-img")`).
#[macro_export]
macro_rules! need_tool {
    ($tool:expr) => {
        if let Err(why) = $crate::host::tool($tool) {
            $crate::host::skip_line(&why);
            return;
        }
    };
}

/// Skip the test (return from it) when [`root`] fails: the test needs root.
#[macro_export]
macro_rules! need_root {
    () => {
        if let Err(why) = $crate::host::root() {
            $crate::host::skip_line(&why);
            return;
        }
    };
}
