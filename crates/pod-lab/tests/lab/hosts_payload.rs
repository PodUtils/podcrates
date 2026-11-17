//! The payloads of the classes of podssh and podnet: this binary with
//! `--payload KIND`, inside a lab. Each one gives one line when the lab is
//! what it must be, else the fault.

use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream, UdpSocket};

use pod_hostprobe::sys::{self as k, Errno};

type Out = Result<String, String>;

pub fn main(kind: &str) -> Out {
    match kind {
        "listen" => listen(),
        "udp" => udp(),
        "raw" => raw(),
        "pty" => pty(),
        "ids" => ids(),
        "home" => home(),
        "noproc" => noproc(),
        "nnp" => nnp(),
        "tun" => tun(),
        "netadmin" => netadmin(),
        "ssl" => ssl(),
        "loopback" => loopback(),
        "bindnetvm" => bind_netns_vm(),
        "caps" => {
            Ok(format!("CapEff {}", u64::from_str_radix(&status_field("CapEff")?, 16).map_err(|e| e.to_string())?))
        }
        _ => crate::net_payload::main(kind).unwrap_or_else(|| Err(format!("no payload {kind:?}"))),
    }
}

fn c(path: &str) -> k::CBuf {
    k::CBuf::new(path).expect("no NUL")
}

fn want(what: &str, r: Result<i64, Errno>, errnos: &[Errno]) -> Result<(), String> {
    match r {
        Err(e) if errnos.contains(&e) => Ok(()),
        Err(e) => Err(format!("{what}: {}; the lab wants {errnos:?}", e.name())),
        Ok(fd) => {
            let _ = k::close(fd);
            Err(format!("{what} succeeded; the lab wants {errnos:?}"))
        }
    }
}

fn status_field(name: &str) -> Result<String, String> {
    let text = std::fs::read_to_string("/proc/self/status").map_err(|e| format!("/proc/self/status: {e}"))?;
    let line = text.lines().find_map(|l| l.strip_prefix(name)).ok_or(format!("no {name}"))?;
    Ok(line.trim_start_matches(':').trim().to_string())
}

fn listen() -> Out {
    let fd = k::socket(k::AF_INET, k::SOCK_STREAM | k::SOCK_CLOEXEC, 0).map_err(|e| e.name().to_string())?;
    let a = k::sockaddr_in([127, 0, 0, 1], 0);
    let r = k::bind(fd, a.as_ptr() as u64, a.len() as u64).map_err(|e| format!("bind: {}", e.name()));
    let l = r.and_then(|_| want("listen", k::listen(fd, 1).map(|_| 0), &[k::EACCES]));
    let _ = k::close(fd);
    l?;
    Ok("bind ok, listen refused".to_string())
}

fn udp() -> Out {
    match UdpSocket::bind("127.0.0.1:0") {
        Err(e) if e.raw_os_error() == Some(k::EPERM.0) => {}
        r => return Err(format!("a UDP bind of IPv4: {r:?}; the lab wants EPERM")),
    }
    match UdpSocket::bind("[::1]:0") {
        Err(e) if [k::EPERM.0, 97].contains(&e.raw_os_error().unwrap_or(0)) => {}
        r => return Err(format!("a UDP bind of IPv6: {r:?}; the lab wants EPERM")),
    }
    TcpListener::bind("127.0.0.1:0").map_err(|e| format!("a TCP bind: {e}; the lab keeps TCP"))?;
    Ok("udp refused, tcp ok".to_string())
}

fn raw() -> Out {
    want("a raw socket", k::socket(k::AF_INET, k::SOCK_RAW | k::SOCK_CLOEXEC, 1), &[k::EPERM])?;
    want("a packet socket", k::socket(k::AF_PACKET, k::SOCK_RAW | k::SOCK_CLOEXEC, 0), &[k::EPERM])?;
    Ok("raw refused".to_string())
}

fn pty() -> Out {
    for path in ["/dev/ptmx", "/dev/pts"] {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            r => return Err(format!("{path}: {r:?}; the lab wants ENOENT")),
        }
    }
    std::fs::write("/dev/null", b"x").map_err(|e| format!("/dev/null: {e}"))?;
    let stat = std::fs::read_to_string("/proc/self/stat").map_err(|e| e.to_string())?;
    let tty = stat.rsplit_once(')').and_then(|(_, r)| r.split_whitespace().nth(4).map(str::to_string));
    if tty.as_deref() != Some("0") {
        return Err(format!("tty_nr is {tty:?}"));
    }
    let fd = k::open(&c("/dev/tty"), k::O_RDWR | k::O_NOCTTY | k::O_NONBLOCK | k::O_CLOEXEC, 0)
        .map_err(|e| format!("open(/dev/tty): {}", e.name()))?;
    let r = k::read(fd, &mut [0u8; 1]);
    let _ = k::close(fd);
    want("a read of /dev/tty", r, &[k::EWOULDBLOCK])?;
    Ok("no ptmx, no pts, no terminal, tty waits".to_string())
}

fn ids() -> Out {
    let id = |nr| unsafe { k::sys(nr, [0; 6]) }.map_or(0, |n| n as u32).to_string();
    for (file, id) in [("/etc/passwd", id(k::SYS_GETUID)), ("/etc/group", id(k::SYS_GETGID))] {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        if text.lines().any(|l| l.split(':').nth(2) == Some(id.as_str())) {
            return Err(format!("{file} has a line of {id}"));
        }
        if text.is_empty() {
            return Err(format!("{file} is empty; the lab removes one line"));
        }
    }
    Ok("no line of the ids".to_string())
}

/// A script that exits 0, at `path`.
fn script(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, b"#!/bin/sh\nexit 0\n").map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

fn home() -> Out {
    let home = std::path::PathBuf::from(std::env::var("HOME").map_err(|_| "no HOME".to_string())?);
    let (in_home, in_tmp) =
        (home.join("run.sh"), std::path::PathBuf::from(format!("/tmp/pod-lab-run-{}.sh", std::process::id())));
    script(&in_home)?;
    script(&in_tmp)?;
    let ran_home = std::process::Command::new(&in_home).status();
    let ran_tmp = std::process::Command::new(&in_tmp).status();
    let _ = std::fs::remove_file(&in_home);
    let _ = std::fs::remove_file(&in_tmp);
    match ran_home {
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {}
        r => return Err(format!("a program in HOME: {r:?}; the lab wants EACCES")),
    }
    match ran_tmp {
        Ok(s) if s.success() => Ok("home noexec, tmp runs".to_string()),
        r => Err(format!("a program in /tmp: {r:?}")),
    }
}

fn noproc() -> Out {
    if let Ok(p) = std::env::current_exe() {
        return Err(format!("current_exe gave {}", p.display()));
    }
    match std::fs::symlink_metadata("/proc/self") {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok("no proc".to_string()),
        r => Err(format!("/proc/self: {r:?}")),
    }
}

fn nnp() -> Out {
    for (field, value) in [("NoNewPrivs", "1"), ("Seccomp", "2"), ("CapEff", "0000000000000000")] {
        let got = status_field(field)?;
        if got != value {
            return Err(format!("{field} is {got}"));
        }
    }
    let raw = |nr, a: [u64; 6]| unsafe { k::sys(nr, a) };
    want("unshare(CLONE_NEWUSER)", raw(k::SYS_UNSHARE, [k::CLONE_NEWUSER, 0, 0, 0, 0, 0]), &[k::EPERM])?;
    let root = c("/");
    want("chroot", raw(k::SYS_CHROOT, [root.ptr(), 0, 0, 0, 0, 0]), &[k::EPERM])?;
    want("ptrace", raw(k::SYS_PTRACE, [0; 6]), &[k::EPERM])?;
    want("mount", raw(k::SYS_MOUNT, [root.ptr(), root.ptr(), 0, k::MS_BIND, 0, 0]), &[k::EPERM])?;
    Ok("no new privs, no caps, calls refused".to_string())
}

fn tun() -> Out {
    want("open(/dev/net/tun)", k::open(&c("/dev/net/tun"), k::O_RDWR | k::O_CLOEXEC, 0), &[k::ENOENT, k::EACCES])?;
    Ok("no tun".to_string())
}

fn netadmin() -> Out {
    let caps = u64::from_str_radix(&status_field("CapEff")?, 16).map_err(|e| e.to_string())?;
    if caps & (1 << 12) != 0 {
        return Err(format!("CapEff {caps:x} has CAP_NET_ADMIN"));
    }
    tun()?;
    Ok("no net admin, no tun".to_string())
}

fn ssl() -> Out {
    let s = k::statfs(&c("/etc/ssl")).map_err(|e| e.name().to_string())?;
    if s.flag(k::ST_RDONLY) != Some(true) {
        return Err("/etc/ssl is not on a read-only mount".to_string());
    }
    let path = format!("/etc/ssl/podlab-{}", std::process::id());
    let erofs = Errno(30);
    want(
        "a new file in /etc/ssl",
        k::open(&c(&path), k::O_WRONLY | k::O_CREAT | k::O_EXCL | k::O_CLOEXEC, 0o600),
        &[erofs],
    )?;
    Ok("etc ssl read-only".to_string())
}

fn loopback() -> Out {
    match TcpStream::connect("127.0.0.1:1") {
        Err(e) if e.raw_os_error() == Some(101) => {}
        r => return Err(format!("connect(127.0.0.1:1): {r:?}; the lab wants ENETUNREACH")),
    }
    use std::os::unix::net::{UnixListener, UnixStream};
    let dir = std::env::temp_dir().join(format!("pod-lab-lo-{}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("s");
    let r = UnixListener::bind(&path).and_then(|l| UnixStream::connect(&path).map(|_| drop(l)));
    let _ = std::fs::remove_dir_all(&dir);
    r.map_err(|e| format!("AF_UNIX: {e}"))?;
    Ok("no loopback, unix ok".to_string())
}

/// `bind-netns-vm-denied`: a TCP bind gives EACCES and a connect reaches
/// the kernel (ECONNREFUSED: nothing listens), a network namespace and a
/// write into the own memory give EPERM, and AF_UNIX holds.
fn bind_netns_vm() -> Out {
    match std::net::TcpListener::bind("127.0.0.1:0") {
        Err(e) if e.raw_os_error() == Some(k::EACCES.0) => {}
        r => return Err(format!("bind(127.0.0.1:0): {r:?}; the lab wants EACCES")),
    }
    match TcpStream::connect("127.0.0.1:1") {
        Err(e) if e.kind() == ErrorKind::ConnectionRefused => {}
        r => return Err(format!("connect(127.0.0.1:1): {r:?}; the lab wants ECONNREFUSED")),
    }
    let raw = |nr, a: [u64; 6]| unsafe { k::sys(nr, a) };
    let net = k::CLONE_NEWUSER | k::CLONE_NEWNET;
    want("unshare(CLONE_NEWUSER|CLONE_NEWNET)", raw(k::SYS_UNSHARE, [net, 0, 0, 0, 0, 0]), &[k::EPERM])?;
    let (mark, mut target) = ([1u8], [0u8]);
    let local = [mark.as_ptr() as u64, 1];
    let remote = [target.as_mut_ptr() as u64, 1];
    let iov = [k::getpid() as u64, local.as_ptr() as u64, 1, remote.as_ptr() as u64, 1, 0];
    want("process_vm_writev(own pid)", raw(k::SYS_PROCESS_VM_WRITEV, iov), &[k::EPERM])?;
    use std::os::unix::net::{UnixListener, UnixStream};
    let dir = std::env::temp_dir().join(format!("pod-lab-bnv-{}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("s");
    let r = UnixListener::bind(&path).and_then(|l| UnixStream::connect(&path).map(|_| drop(l)));
    let _ = std::fs::remove_dir_all(&dir);
    r.map_err(|e| format!("AF_UNIX: {e}"))?;
    Ok("bind refused, connect ok, no netns, no vm write, unix ok".to_string())
}
