//! The runs of the classes of podssh and podnet that a lab holds with no
//! root: each with a payload that shows its parts.

use std::ffi::OsString;

use crate::cases::{payload, ran, run, settings};
use pod_hostprobe::sys as k;
use pod_testkit::host::skip_line;

/// False, with a skip line, where a caller that is not root gets no user
/// namespace with a mount: the mounts of a lab need one.
/// `scripts/ci/userns-tests.sh` fails on the line.
fn mounts_here(name: &str) -> bool {
    if !pod_testkit::nobody::is_root()
        && let Err(why) = pod_testkit::host::userns()
    {
        skip_line(&format!("{name}: the mounts of the lab need a user namespace with a mount: {why}"));
        return false;
    }
    true
}

/// False, with a skip line, where the kernel has no Landlock.
fn landlock_here(name: &str) -> bool {
    match pod_hostprobe::sys::landlock_abi() {
        Ok(_) => true,
        Err(e) => {
            skip_line(&format!("{name}: the lab needs Landlock; this host gives {}", e.name()));
            false
        }
    }
}

/// Run the payload `kind` in the lab `name`, and check its line.
fn case(name: &str, kind: &str, line: &str) {
    let (out, text) = run(&settings(), name, &payload(kind), false);
    if crate::filter::refused_here(name, &out) {
        return;
    }
    assert_eq!(ran(name, out, &text), 0, "{name}: {text}");
    assert_eq!(text.trim(), line, "{name}");
    println!("{name}: ok");
}

pub fn listen_denied() {
    case("listen-denied", "listen", "bind ok, listen refused");
}

pub fn udp_denied() {
    case("udp-denied", "udp", "udp refused, tcp ok");
}

pub fn raw_denied() {
    case("raw-denied", "raw", "raw refused");
}

/// The flag of the child of [`no_ptmx`].
pub const CTTY_CHILD: &str = "--ctty-child";

/// The case of `no-ptmx` runs in a child of this binary that leads a new
/// session with a pseudo terminal as its controlling terminal: the lab
/// must leave that terminal.
pub fn no_ptmx() {
    if !mounts_here("no-ptmx") {
        return;
    }
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new(std::env::current_exe().expect("the path of this binary"));
    child.arg(CTTY_CHILD);
    // SAFETY: one system call between the fork and the exec.
    unsafe {
        child.pre_exec(|| k::setsid().map(drop).map_err(|e| std::io::Error::from_raw_os_error(e.0)));
    }
    let status = child.status().expect("the child of the terminal");
    assert!(status.success(), "no-ptmx: {status}");
}

const TIOCSCTTY: u64 = 0x540E;
const TIOCSPTLCK: u64 = 0x4004_5431;
const TIOCGPTPEER: u64 = 0x5441;

/// The child of [`no_ptmx`]: take a pseudo terminal as the controlling
/// terminal, then run the case.
pub fn ctty_child() -> ! {
    let ioctl = |fd: i64, req: u64, arg: u64| unsafe { k::sys(k::SYS_IOCTL, [fd as u64, req, arg, 0, 0, 0]) };
    let flags = k::O_RDWR | k::O_NOCTTY | k::O_CLOEXEC;
    let master = k::open(&k::CBuf::new("/dev/ptmx").expect("no NUL"), flags, 0).expect("/dev/ptmx of the host");
    let unlock = 0i32;
    ioctl(master, TIOCSPTLCK, &unlock as *const i32 as u64).expect("unlockpt");
    let slave = ioctl(master, TIOCGPTPEER, flags).expect("the peer of the terminal");
    ioctl(slave, TIOCSCTTY, 0).expect("the controlling terminal");
    let stat = std::fs::read_to_string("/proc/self/stat").expect("/proc/self/stat");
    let tty = stat.rsplit_once(')').and_then(|(_, r)| r.split_whitespace().nth(4).map(str::to_string));
    assert_ne!(tty.as_deref(), Some("0"), "the child has no controlling terminal");
    case("no-ptmx", "pty", "no ptmx, no pts, no terminal, tty waits");
    std::process::exit(0)
}

pub fn no_passwd_entry() {
    if !mounts_here("no-passwd-entry") {
        return;
    }
    case("no-passwd-entry", "ids", "no line of the ids");
}

/// `HOME` is a new directory of the test: the lab binds it with `noexec`.
pub fn noexec_home() {
    if !mounts_here("noexec-home") {
        return;
    }
    let home = std::env::temp_dir().join(format!("pod-lab-home-{}", std::process::id()));
    std::fs::create_dir_all(&home).expect("a HOME");
    let old = std::env::var_os("HOME");
    // SAFETY: the test binary has one thread (no libtest).
    unsafe { std::env::set_var("HOME", &home) };
    let (out, text) = run(&settings(), "noexec-home", &payload("home"), false);
    match old {
        // SAFETY: as above.
        Some(v) => unsafe { std::env::set_var("HOME", v) },
        None => unsafe { std::env::remove_var("HOME") },
    }
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(ran("noexec-home", out, &text), 0, "{text}");
    assert_eq!(text.trim(), "home noexec, tmp runs");
    println!("noexec-home: ok");
}

/// The command of a lab with no `/proc` is a path: `/proc/self/exe` is not
/// there.
pub fn no_proc() {
    if !mounts_here("no-proc") {
        return;
    }
    let exe: OsString = std::env::current_exe().expect("the path of this binary").into();
    let cmd = vec![exe, "--payload".into(), "noproc".into()];
    let (out, text) = run(&settings(), "no-proc", &cmd, false);
    assert_eq!(ran("no-proc", out, &text), 0, "{text}");
    assert_eq!(text.trim(), "no proc");
    println!("no-proc: ok");
}

pub fn seccomp_nonewprivs() {
    case("seccomp-nonewprivs", "nnp", "no new privs, no caps, calls refused");
}

pub fn no_tun() {
    if !landlock_here("no-tun") {
        return;
    }
    case("no-tun", "tun", "no tun");
    case("no-net-admin", "netadmin", "no net admin, no tun");
}

pub fn ro_etc_ssl() {
    if !mounts_here("ro-etc-ssl") {
        return;
    }
    case("ro-etc-ssl", "ssl", "etc ssl read-only");
}

pub fn no_loopback() {
    if !mounts_here("no-loopback") {
        return;
    }
    case("no-loopback", "loopback", "no loopback, unix ok");
}

/// No TCP bind, no network namespace and no write of another process's
/// memory together (PodUtils/podnet#649): Landlock of the TCP bind alone
/// (ABI 4) and seccomp.
pub fn bind_netns_vm_denied() {
    match k::landlock_abi() {
        Ok(abi) if abi >= 4 => {}
        other => {
            skip_line(&format!("bind-netns-vm-denied: the lab needs Landlock ABI 4; this host gives {other:?}"));
            return;
        }
    }
    case("bind-netns-vm-denied", "bindnetvm", "bind refused, connect ok, no netns, no vm write, unix ok");
}

/// A class of the machine tier of podvm: the run refuses it.
pub fn a_machine_class_is_refused() {
    for name in ["cgroup-v1", "arm64-tcg"] {
        let (out, _) = run(&settings(), name, &[OsString::from("/bin/true")], false);
        assert!(matches!(out, Err(pod_lab::RunError::Refused(_))), "{name}: {out:?}");
    }
    println!("a_machine_class_is_refused: ok");
}

pub fn all() {
    listen_denied();
    udp_denied();
    raw_denied();
    no_ptmx();
    no_passwd_entry();
    noexec_home();
    no_proc();
    seccomp_nonewprivs();
    no_tun();
    ro_etc_ssl();
    no_loopback();
    bind_netns_vm_denied();
    a_machine_class_is_refused();
}
