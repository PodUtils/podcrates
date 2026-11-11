//! The confinement of a real child, as experiment 414 of podvm checks it: a
//! read outside the read list, a write outside the write list, a TCP
//! connect and bind outside the port lists, an exec outside the execute
//! list, `NoNewPrivs` and `Seccomp` of the child, a denied call, and the
//! refusal where Landlock is absent; and the scopes, a closed UDP and an
//! MPTCP socket under a TCP list. Each test forks a child that applies the
//! compiled spec and makes its calls with no allocation; the results cross
//! a pipe. A host with no Landlock, or with an ABI under the one of a
//! check, skips with a line that says so.
#![cfg(target_os = "linux")]

mod child;

use std::path::{Path, PathBuf};

use child::{code, in_child};
use pod_confine::{Access, Call, Compiled, Deny, MIN_ABI, NET_ABI, Refusal, SCOPE_ABI, Spec, apply};
use pod_core::Class;
use pod_hostprobe::sys::{self as k, CBuf};

const EACCES: i64 = -13;
const EPERM: i64 = -1;
const ENOPROTOOPT: i64 = -92;

/// The ABI where a confinement loads here, else a skip line and `None`.
fn landlock(need: u32) -> Option<u32> {
    match pod_confine::check() {
        Ok(abi) if abi >= need => Some(abi),
        Ok(abi) => {
            pod_testkit::host::skip_line(&format!("Landlock ABI {abi} is here, and the test needs ABI {need}"));
            None
        }
        Err(e) => {
            pod_testkit::host::skip_line(&e.to_string());
            None
        }
    }
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-confine-apply-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn c(p: &Path) -> CBuf {
    CBuf::new(p.to_str().unwrap()).unwrap()
}

fn text(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

/// Apply `compiled` in the child; `Err` is the refusal as a negative call
/// code, so a test sees it in place of its results.
fn confined(compiled: &Compiled) -> Result<(), i64> {
    apply(compiled).map_err(|r| -1000 - i64::from(r.call.code()))
}

/// The probe gives the ABI, and the caller is not confined by it: its
/// `NoNewPrivs` and `Seccomp` stay, and it still reads a file.
#[test]
fn the_probe_gives_the_abi_and_leaves_the_caller_free() {
    let status = || std::fs::read_to_string("/proc/self/status").unwrap();
    let line = |s: &str, head: &str| s.lines().find(|l| l.starts_with(head)).unwrap_or_default().to_string();
    let before = status();
    let Some(abi) = landlock(1) else { return };
    assert!(abi >= 1);
    assert_eq!(pod_confine::probe(), Some(abi));
    let after = status();
    assert_eq!(line(&before, "NoNewPrivs:"), line(&after, "NoNewPrivs:"));
    assert_eq!(line(&before, "Seccomp:"), line(&after, "Seccomp:"));
    let d = dir("probe");
    std::fs::write(d.join("f"), b"still free").unwrap();
    assert_eq!(std::fs::read(d.join("f")).unwrap(), b"still free");
    let _ = std::fs::remove_dir_all(d);
}

/// A read outside the read list fails with EACCES (experiment 414: `cat
/// /etc/hostname`), and one inside passes.
#[test]
fn a_read_outside_the_read_list_is_eacces() {
    let Some(abi) = landlock(MIN_ABI) else { return };
    let d = dir("read");
    std::fs::create_dir_all(d.join("inside")).unwrap();
    std::fs::write(d.join("inside/ok"), b"ok").unwrap();
    std::fs::write(d.join("secret"), b"no").unwrap();
    let compiled = Spec::new().allow(text(&d.join("inside")), Access::READ).compile(abi).unwrap();
    let (ok, secret, hostname) = (c(&d.join("inside/ok")), c(&d.join("secret")), c(Path::new("/etc/hostname")));
    let has_hostname = Path::new("/etc/hostname").exists();
    let got = in_child(|| {
        if let Err(e) = confined(&compiled) {
            return [e; 3];
        }
        let open = |p: &CBuf| code(k::open(p, k::O_RDONLY | k::O_CLOEXEC, 0));
        [open(&ok).min(0), open(&secret), open(&hostname)]
    });
    assert_eq!(got[0], 0, "a read inside the list failed: {got:?}");
    assert_eq!(got[1], EACCES, "a read outside the list: {got:?}");
    if has_hostname {
        assert_eq!(got[2], EACCES, "/etc/hostname: {got:?}");
    }
    let _ = std::fs::remove_dir_all(d);
}

/// A write outside the write list fails with EACCES: a new file, and an
/// open for write of a file in a read-only path.
#[test]
fn a_write_outside_the_write_list_is_eacces() {
    let Some(abi) = landlock(MIN_ABI) else { return };
    let d = dir("write");
    std::fs::create_dir_all(d.join("w")).unwrap();
    std::fs::create_dir_all(d.join("r")).unwrap();
    std::fs::write(d.join("r/kept"), b"kept").unwrap();
    let spec =
        Spec::new().allow(text(&d.join("w")), Access::READ | Access::WRITE).allow(text(&d.join("r")), Access::READ);
    let compiled = spec.compile(abi).unwrap();
    let (new_w, new_r, kept) = (c(&d.join("w/new")), c(&d.join("r/new")), c(&d.join("r/kept")));
    let got = in_child(|| {
        if let Err(e) = confined(&compiled) {
            return [e; 4];
        }
        let create = k::O_CREAT | k::O_WRONLY | k::O_CLOEXEC;
        [
            code(k::open(&new_w, create, 0o600)).min(0),
            code(k::open(&new_r, create, 0o600)),
            code(k::open(&kept, k::O_WRONLY | k::O_TRUNC | k::O_CLOEXEC, 0)),
            // Only the write right stops this one.
            code(k::open(&kept, k::O_WRONLY | k::O_CLOEXEC, 0)),
        ]
    });
    assert_eq!(got, [0, EACCES, EACCES, EACCES], "inside, a new file outside, a truncate outside, a write outside");
    assert!(!d.join("r/new").exists());
    assert_eq!(std::fs::read(d.join("r/kept")).unwrap(), b"kept");
    let _ = std::fs::remove_dir_all(d);
}

/// A TCP connect to a port outside the list fails with EACCES, one inside
/// passes (experiment 414: 22 refused, 443 passes); a bind outside the
/// bind list fails with EACCES.
#[test]
fn a_tcp_connect_or_bind_outside_the_list_is_eacces() {
    pod_testkit::need_loopback!();
    let Some(abi) = landlock(NET_ABI) else { return };
    let allowed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let other = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (ok_port, other_port) = (allowed.local_addr().unwrap().port(), other.local_addr().unwrap().port());
    let compiled = Spec::new().connect_to(ok_port).bind_to(ok_port).compile(abi).unwrap();
    let (to_ok, to_other) = (k::sockaddr_in([127, 0, 0, 1], ok_port), k::sockaddr_in([127, 0, 0, 1], other_port));
    let got = in_child(|| {
        if let Err(e) = confined(&compiled) {
            return [e; 4];
        }
        let socket = || k::socket(k::AF_INET, k::SOCK_STREAM | k::SOCK_CLOEXEC, 0).unwrap_or(-1);
        let bind = |addr: &[u8; 16]| code(k::bind(socket(), addr.as_ptr() as u64, 16));
        [
            code(k::connect(socket(), &to_ok)),
            code(k::connect(socket(), &to_other)),
            bind(&to_other),
            // The port of the list is held by the listener: not EACCES.
            bind(&to_ok),
        ]
    });
    assert_eq!(got[0], 0, "a connect inside the list failed: {got:?}");
    assert_eq!(got[1], EACCES, "a connect outside the list: {got:?}");
    assert_eq!(got[2], EACCES, "a bind outside the list: {got:?}");
    assert_ne!(got[3], EACCES, "a bind inside the list: {got:?}");
}

/// The child has `NoNewPrivs: 1` and `Seccomp: 2` (experiment 414).
#[test]
fn the_child_has_no_new_privs_and_seccomp_mode_2() {
    let Some(abi) = landlock(MIN_ABI) else { return };
    let compiled = Spec::new().allow("/proc", Access::READ).compile(abi).unwrap();
    let status = CBuf::new("/proc/self/status").unwrap();
    let got = in_child(|| {
        if let Err(e) = confined(&compiled) {
            return [e; 2];
        }
        let mut buf = [0u8; 8192];
        let Ok(fd) = k::open(&status, k::O_RDONLY | k::O_CLOEXEC, 0) else { return [-2; 2] };
        let mut n = 0;
        while let Ok(got @ 1..) = k::read(fd, &mut buf[n..]) {
            n += got as usize;
        }
        let has = |pat: &[u8]| i64::from(buf[..n].windows(pat.len()).any(|w| w == pat));
        [has(b"NoNewPrivs:\t1\n"), has(b"Seccomp:\t2\n")]
    });
    assert_eq!(got, [1, 1], "NoNewPrivs 1 and Seccomp 2");
}

/// A call of the deny list fails with EPERM (`clone3` with ENOSYS); the
/// same calls with no filter give other answers, so the filter answered.
#[test]
fn a_denied_call_gives_eperm() {
    let Some(abi) = landlock(MIN_ABI) else { return };
    let compiled = Spec::new().compile(abi).unwrap();
    let calls = || {
        // SAFETY: each argument is an integer or a null pointer.
        let call = |nr: i64, a: [u64; 6]| code(unsafe { k::sys(nr, a) });
        // A clone that the filter lets through makes a child: it exits.
        // SAFETY: a fork (no CLONE_VM) with CLONE_NEWUSER; the new child
        // only exits.
        let clone = match unsafe { k::sys(k::SYS_CLONE, [0x1000_0000 | 17, 0, 0, 0, 0, 0]) } {
            Ok(0) => k::exit_group(0),
            other => code(other),
        };
        [
            code(k::unshare(0x1000_0000)),
            clone,
            call(k::SYS_SETNS, [u64::MAX, 0, 0, 0, 0, 0]),
            call(k::SYS_CLONE3, [0; 6]),
            code(k::unshare(0x400)),
        ]
    };
    let free = in_child(calls);
    let got = in_child(|| {
        if let Err(e) = confined(&compiled) {
            return [e; 5];
        }
        calls()
    });
    assert_eq!(got, [EPERM, EPERM, EPERM, -38, 0], "unshare(NEWUSER), clone(NEWUSER), setns, clone3, unshare(FILES)");
    assert_eq!(free[2], -9, "setns(-1) with no filter is EBADF: {free:?}");
    assert_ne!(free[3], -38, "clone3 with no filter: {free:?}");
}

/// An exec of a file outside the execute list fails with EACCES, also
/// where the file can be read.
#[test]
fn an_exec_outside_the_execute_list_is_eacces() {
    pod_testkit::need_tool!("true");
    let Some(abi) = landlock(MIN_ABI) else { return };
    let tool = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join("true"))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
        .unwrap();
    let compiled = Spec::new().allow(text(tool.parent().unwrap()), Access::READ).compile(abi).unwrap();
    let path = c(&tool);
    let argv: [*const u8; 2] = [path.ptr() as *const u8, std::ptr::null()];
    let envp: [*const u8; 1] = [std::ptr::null()];
    let got = in_child(|| {
        if let Err(e) = confined(&compiled) {
            return [e];
        }
        // SAFETY: both arrays end with a null pointer, and `path` lives.
        [code(unsafe { k::execve(&path, argv.as_ptr(), envp.as_ptr()) })]
    });
    assert_eq!(got, [EACCES], "{}", tool.display());
}

/// Where Landlock is absent, `apply` refuses at `landlock_create_ruleset`
/// and confines nothing more: the caller does not exec. A filter that
/// answers ENOSYS to the Landlock calls stands in for such a kernel.
#[test]
fn apply_refuses_where_landlock_is_absent() {
    let d = dir("absent");
    let compiled = Spec::new().allow(text(&d), Access::READ).compile(MIN_ABI).unwrap();
    let absent = pod_confine::deny::program(&[
        Deny::call("landlock_create_ruleset", k::SYS_LANDLOCK_CREATE_RULESET, 38),
        Deny::call("landlock_add_rule", k::SYS_LANDLOCK_ADD_RULE, 38),
        Deny::call("landlock_restrict_self", k::SYS_LANDLOCK_RESTRICT_SELF, 38),
    ])
    .unwrap();
    let got = in_child(|| {
        if k::set_no_new_privs().is_err() || k::seccomp_set_filter(&absent, 0).is_err() {
            return [-2; 3];
        }
        match apply(&compiled) {
            Ok(()) => [0; 3],
            Err(r) => [1, i64::from(r.errno.0), i64::from(r.call.code())],
        }
    });
    if got == [-2; 3] {
        pod_testkit::host::skip_line("this host refuses a seccomp filter");
        return;
    }
    assert_eq!(got[0], 1, "apply passed with no Landlock: the payload would run unconfined");
    assert_eq!(got[1], 38, "the errno: {got:?}");
    assert_eq!(got[2], i64::from(Call::CreateRuleset.code()), "the call: {got:?}");
    let r = Refusal { call: Call::CreateRuleset, index: 0, errno: k::Errno(38) };
    assert!(r.failure(&compiled).to_string().contains("landlock_create_ruleset: ENOSYS"), "{}", r.failure(&compiled));
    let _ = std::fs::remove_dir_all(d);
}

/// With the scopes (ABI 6), a signal to a process outside the domain (the
/// parent) and a connect to an abstract Unix socket of the parent fail
/// with EPERM; a signal to the child itself passes. With no scope, both
/// pass, so the scope answered.
#[test]
fn the_scopes_stop_a_signal_and_an_abstract_connect_outside_the_domain() {
    use std::os::linux::net::SocketAddrExt;
    let Some(abi) = landlock(SCOPE_ABI) else { return };
    let name = format!("pod-confine-scope-{}", std::process::id());
    let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
    let _listener = std::os::unix::net::UnixListener::bind_addr(&addr).unwrap();
    // A `struct sockaddr_un` of an abstract name: the family, a NUL byte,
    // the name, and no NUL at the end.
    let mut sun = (k::AF_UNIX as u16).to_ne_bytes().to_vec();
    sun.push(0);
    sun.extend_from_slice(name.as_bytes());
    let calls = |compiled: &Compiled| {
        if let Err(e) = confined(compiled) {
            return [e; 3];
        }
        let unix = k::socket(k::AF_UNIX, k::SOCK_STREAM | k::SOCK_CLOEXEC, 0).unwrap_or(-1);
        [code(k::kill(k::getppid(), 0)), code(k::connect(unix, &sun)), code(k::kill(k::getpid(), 0))]
    };
    let scoped = Spec::new().compile(abi).unwrap();
    assert_eq!(scoped.scoped(), pod_confine::spec::SCOPES);
    let got = in_child(|| calls(&scoped));
    assert_eq!(got, [EPERM, EPERM, 0], "a signal to the parent, an abstract connect, a signal to the child itself");
    let free = Spec::new().no_scope().compile(abi).unwrap();
    let got = in_child(|| calls(&free));
    assert_eq!(got, [0, 0, 0], "with no scope");
}

/// A closed UDP: a UDP socket of IPv4 and of IPv6 (with SOCK_CLOEXEC) fails
/// with EACCES, and io_uring_setup with EPERM; a TCP socket and a Unix
/// datagram socket still work. The deny list of the spec is empty, so the
/// rules come from the switch.
#[test]
fn a_closed_udp_gives_eacces_and_tcp_still_works() {
    let Some(abi) = landlock(MIN_ABI) else { return };
    let spec = Spec { deny: Vec::new(), ..Spec::new() }.close_udp();
    let compiled = match spec.compile(abi) {
        Ok(c) => c,
        Err(e) if e.class == Class::Unsupported => {
            pod_testkit::host::skip_line(&e.to_string());
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let calls = || {
        let socket = |family: u64, kind: u64| code(k::socket(family, kind, 0)).min(0);
        // `struct io_uring_params`: 120 bytes, which the kernel fills.
        let mut params = [0u8; 120];
        // SAFETY: `params` lives until the call returns, and holds 120 bytes.
        let ring = code(unsafe { k::sys(k::SYS_IO_URING_SETUP, [1, params.as_mut_ptr() as u64, 0, 0, 0, 0]) });
        [
            socket(k::AF_INET, k::SOCK_DGRAM),
            socket(k::AF_INET6, k::SOCK_DGRAM | k::SOCK_CLOEXEC),
            socket(k::AF_INET, k::SOCK_STREAM | k::SOCK_CLOEXEC),
            socket(k::AF_UNIX, k::SOCK_DGRAM | k::SOCK_CLOEXEC),
            ring.min(0),
        ]
    };
    let free = in_child(calls);
    assert_eq!(free[0], 0, "a UDP socket with no filter: {free:?}");
    let got = in_child(|| {
        if let Err(e) = confined(&compiled) {
            return [e; 5];
        }
        calls()
    });
    assert_eq!(got, [EACCES, EACCES, 0, 0, EPERM], "UDP, UDP of IPv6, TCP, Unix, io_uring_setup");
}

/// Landlock checks TCP sockets only: under a TCP list an MPTCP socket
/// fails with ENOPROTOOPT, else its connect passes each port rule. A TCP
/// connect inside the list still passes. A kernel with no MPTCP skips.
#[test]
fn an_mptcp_socket_fails_where_tcp_has_a_list() {
    pod_testkit::need_loopback!();
    let Some(abi) = landlock(NET_ABI) else { return };
    let allowed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let other = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (ok_port, other_port) = (allowed.local_addr().unwrap().port(), other.local_addr().unwrap().port());
    let compiled = Spec::new().connect_to(ok_port).compile(abi).unwrap();
    let (to_ok, to_other) = (k::sockaddr_in([127, 0, 0, 1], ok_port), k::sockaddr_in([127, 0, 0, 1], other_port));
    let calls = || {
        let mptcp = k::socket(k::AF_INET, k::SOCK_STREAM | k::SOCK_CLOEXEC, k::IPPROTO_MPTCP);
        let tcp = k::socket(k::AF_INET, k::SOCK_STREAM | k::SOCK_CLOEXEC, 0).unwrap_or(-1);
        [code(mptcp).min(0), mptcp.map_or(1, |fd| code(k::connect(fd, &to_other))), code(k::connect(tcp, &to_ok))]
    };
    let free = in_child(calls);
    if free[0] != 0 {
        pod_testkit::host::skip_line(&format!("this kernel makes no MPTCP socket (errno {})", -free[0]));
        return;
    }
    let got = in_child(|| {
        if let Err(e) = confined(&compiled) {
            return [e; 3];
        }
        calls()
    });
    assert_eq!(got, [ENOPROTOOPT, 1, 0], "an MPTCP socket, its connect outside the list, a TCP connect inside");
}

/// A confined root keeps docker's capabilities by default and none with
/// `no_caps()`; with either it reads no environment of its parent, an
/// unconfined process of the same uid (the read passes with no
/// confinement). `CAP_SYS_ADMIN` or `CAP_PERFMON` alone lets the read pass
/// the Landlock domain (measured on Linux 6.18, ABI 7); docker's set holds
/// neither. Root only: another user holds no capability to drop.
#[test]
fn a_confined_root_keeps_only_the_capabilities_of_the_spec() {
    let Some(abi) = landlock(MIN_ABI) else { return };
    if std::fs::read_to_string("/proc/self/status").unwrap().lines().all(|l| !l.starts_with("Uid:\t0\t")) {
        pod_testkit::host::skip_line("the test runs as root only");
        return;
    }
    let environ = CBuf::new(&format!("/proc/{}/environ", std::process::id())).unwrap();
    let run = |spec: Option<Spec>| {
        let compiled = spec.map(|s| s.allow("/proc", Access::READ).compile(abi).unwrap());
        in_child(|| {
            if let Some(c) = &compiled
                && let Err(e) = confined(c)
            {
                return [e; 3];
            }
            let caps = k::capget().map_or([-1, -1], |s| [s.effective as i64, s.permitted as i64]);
            [code(k::open(&environ, k::O_RDONLY | k::O_CLOEXEC, 0)).min(0), caps[0], caps[1]]
        })
    };
    assert_eq!(run(None)[0], 0, "with no confinement the parent's environment reads");
    assert_eq!(run(Some(Spec::new().no_caps())), [EACCES, 0, 0], "no capability kept: the read, CapEff, CapPrm");
    let docker = pod_confine::DOCKER_CAPS as i64;
    let got = run(Some(Spec::new()));
    assert_eq!(got[0], EACCES, "the default (docker's set) reads no environment of the parent: {got:?}");
    assert!(got[1] != 0 && got[1] & !docker == 0, "the default keeps docker's set and no other: {got:?}");
    let admin = 1i64 << k::CAP_SYS_ADMIN;
    let got = run(Some(Spec::new().no_caps().keep_cap(k::CAP_SYS_ADMIN)));
    assert_eq!(got, [0, admin, admin], "CAP_SYS_ADMIN alone passes the domain, and it is the one kept");
}
