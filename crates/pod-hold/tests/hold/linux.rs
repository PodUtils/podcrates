//! The holds of a real payload. The binary is the supervisor and, with a
//! first argument `payload-*`, the payload. A host that refuses a hold
//! (no seccomp listener, no user namespace, no TUN) skips its test with a
//! line that says so.

use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use pod_core::Class;
use pod_hold::filter::Calls;
use pod_hold::listen::Listener;
use pod_hold::mem::Access;
use pod_hold::spawn::{Program, Started, start};
use pod_hold::{Level, netns, reply, sockaddr};

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// The payload: connect a TCP socket to 127.0.0.1:PORT. The exit code is
/// 0 for a connection, the errno for a failure, 200 for any other return.
fn payload_connect(port: u16) -> i32 {
    // SAFETY: plain system calls on a local buffer.
    unsafe {
        let s = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        if s < 0 {
            return 201;
        }
        let addr = sockaddr::encode(libc::AF_INET as u16, [127, 0, 0, 1].into(), port);
        match libc::connect(s, addr.as_ptr().cast(), addr.len() as u32) {
            0 => 0,
            r if r < 0 => errno(),
            _ => 200,
        }
    }
}

/// The payload: make a socket and write `hold` on it.
fn payload_socket() -> i32 {
    // SAFETY: plain system calls on a local buffer.
    unsafe {
        let s = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        if s < 0 {
            return errno();
        }
        if libc::write(s, b"hold".as_ptr().cast(), 4) == 4 { 0 } else { 202 }
    }
}

/// The payload: open a raw ICMP socket, with flags in its type. The exit
/// code is 0 for a socket, else the errno.
fn payload_raw() -> i32 {
    let kind = libc::SOCK_RAW | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK;
    // SAFETY: a plain system call.
    if unsafe { libc::socket(libc::AF_INET, kind, libc::IPPROTO_ICMP) } < 0 { errno() } else { 0 }
}

/// The payload: `socket(family, kind, protocol)` from the arguments. The
/// exit code is 0 for a socket, else the errno; 203 for a bad argument.
fn payload_open(args: &[String]) -> i32 {
    let numbers: Vec<i32> = args.iter().filter_map(|a| a.parse().ok()).collect();
    let [family, kind, protocol] = numbers[..] else { return 203 };
    // SAFETY: a plain system call.
    if unsafe { libc::socket(family, kind, protocol) } < 0 { errno() } else { 0 }
}

fn wait(pid: i32) -> i32 {
    let mut status = 0;
    // SAFETY: waitpid of a child of this process.
    unsafe { libc::waitpid(pid, &mut status, 0) };
    if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 128 + libc::WTERMSIG(status) }
}

fn payload(part: &str, calls: &Calls) -> Option<Started> {
    payload_with(&["hold", part], calls)
}

fn payload_with(argv: &[&str], calls: &Calls) -> Option<Started> {
    let me = std::env::current_exe().expect("the test binary");
    let program = Program::new(&me, argv, &[] as &[&str]).expect("a program");
    match start(program, calls) {
        Ok(started) => Some(started),
        Err(e) => {
            println!("SKIP: the host refuses a seccomp listener: {}", e.chain());
            None
        }
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime")
}

/// The supervisor reads the address of the connect from the memory of the
/// task, and its refusal is the errno of the call. A reply with the wrong
/// sign would be a return value of 111, and the connect a success.
fn a_refused_connect_fails_with_the_errno_of_the_reply() {
    let Some(started) = payload("payload-connect", &Calls::of(&[libc::SYS_connect])) else { return };
    assert_eq!(started.hold.level, Level::Advisory);
    assert!(started.hold.proof.contains("1 system calls"), "{}", started.hold.proof);
    let pid = started.pid;
    runtime().block_on(async {
        let listener = Listener::new(started.listener, Access::default()).expect("a listener");
        let n = tokio::time::timeout(Duration::from_secs(10), listener.recv()).await.expect("in time").expect("a call");
        assert_eq!(n.nr as libc::c_long, libc::SYS_connect);
        assert_eq!(n.pid as i32, pid);
        let bytes = sockaddr::read(&n.memory, n.args[1], n.args[2]).expect("the address");
        let addr = sockaddr::parse(&bytes).expect("an AF_INET address");
        assert_eq!((addr.ip, addr.port), ([127, 0, 0, 1].into(), 9));
        reply::respond(listener.raw(), n.id, -1, libc::ECONNREFUSED).expect("the reply");
        let end = tokio::time::timeout(Duration::from_secs(10), listener.recv()).await.expect("in time");
        assert!(end.is_none(), "no task uses the filter after the exit");
    });
    assert_eq!(wait(pid), libc::ECONNREFUSED, "the connect fails with the errno of the reply");
    println!("ok: a_refused_connect_fails_with_the_errno_of_the_reply");
}

/// The answer to a stopped `socket` is a descriptor that the supervisor
/// adds to the task (`SECCOMP_ADDFD_FLAG_SEND`): the payload writes on it,
/// and the supervisor reads its end.
fn a_socket_call_gets_the_descriptor_of_the_supervisor() {
    let Some(started) = payload("payload-socket", &Calls::of(&[libc::SYS_socket])) else { return };
    let pid = started.pid;
    let (ours, theirs) = std::os::unix::net::UnixStream::pair().expect("a pair");
    runtime().block_on(async {
        let listener = Listener::new(started.listener, Access::default()).expect("a listener");
        let n = tokio::time::timeout(Duration::from_secs(10), listener.recv()).await.expect("in time").expect("a call");
        assert_eq!(n.nr as libc::c_long, libc::SYS_socket);
        let fd = reply::addfd_send(listener.raw(), n.id, theirs.as_raw_fd(), 0).expect("NOTIF_ADDFD");
        assert!(fd >= 0);
    });
    drop(theirs);
    assert_eq!(wait(pid), 0);
    let mut buf = [0u8; 4];
    std::io::Read::read_exact(&mut &ours, &mut buf).expect("the bytes of the payload");
    assert_eq!(&buf, b"hold");
    println!("ok: a_socket_call_gets_the_descriptor_of_the_supervisor");
}

/// A raw ICMP socket under the calls of a hold of each socket: the filter
/// answers `EPERM`, and no notification reaches the supervisor. With
/// `Calls::allow_raw`, the call reaches the supervisor (which proceeds
/// with it), and the answer is the kernel's: a socket, or the kernel's own
/// `EPERM` or `EACCES` on a host that gives no `CAP_NET_RAW` and no ping
/// group. The notification tells the two answers apart
/// (PodUtils/podnet#538).
fn a_raw_icmp_socket_gets_eperm_unless_the_hold_allows_it() {
    for (calls, allowed) in [(Calls::sockets(), false), (Calls::sockets().allow_raw(), true)] {
        let Some(started) = payload("payload-raw", &calls) else { return };
        let pid = started.pid;
        let raw = runtime().block_on(async {
            let listener = Listener::new(started.listener, Access::default()).expect("a listener");
            let mut raw = 0;
            loop {
                let next = tokio::time::timeout(Duration::from_secs(10), listener.recv()).await.expect("in time");
                let Some(n) = next else { break };
                let kind = (n.args[1] as i32) & 0xf;
                if n.nr as libc::c_long == libc::SYS_socket && kind == libc::SOCK_RAW {
                    raw += 1;
                }
                reply::proceed(listener.raw(), n.id).expect("the reply");
            }
            raw
        });
        let code = wait(pid);
        if allowed {
            assert_eq!(raw, 1, "the opt-in gives the call to the supervisor");
            assert!([0, libc::EPERM, libc::EACCES].contains(&code), "the kernel's answer: {code}");
            println!("the kernel answered the allowed raw socket with {code} (0: a socket)");
        } else {
            assert_eq!((raw, code), (0, libc::EPERM), "the filter answers EPERM, with no notification");
        }
    }
    println!("ok: a_raw_icmp_socket_gets_eperm_unless_the_hold_allows_it");
}

/// A held child opens `socket(family, kind, protocol)`; the supervisor
/// proceeds with each call. Give the count of the `socket` calls of
/// `family` that reached the supervisor and the exit code of the child.
fn open_held(calls: &Calls, family: i32, kind: i32, protocol: i32) -> Option<(u32, i32)> {
    let numbers = [family, kind, protocol].map(|n| n.to_string());
    let started = payload_with(&["hold", "payload-open", &numbers[0], &numbers[1], &numbers[2]], calls)?;
    let pid = started.pid;
    let seen = runtime().block_on(async {
        let listener = Listener::new(started.listener, Access::default()).expect("a listener");
        let mut seen = 0;
        loop {
            let next = tokio::time::timeout(Duration::from_secs(10), listener.recv()).await.expect("in time");
            let Some(n) = next else { break };
            if n.nr as libc::c_long == libc::SYS_socket && n.args[0] as i32 == family {
                seen += 1;
            }
            reply::proceed(listener.raw(), n.id).expect("the reply");
        }
        seen
    });
    Some((seen, wait(pid)))
}

/// The raw part (PodUtils/podcrates T-558) and the ping part
/// (PodUtils/podnet#550) of the socket rules, each with its own opt-in. A
/// refused socket gets `EPERM` from the filter, and the supervisor gets no
/// notification. An allowed socket reaches the supervisor, which proceeds
/// with it; the kernel's answer is then a socket or an errno of this host
/// (no `CAP_NET_RAW`, no ping group, no `AF_XDP`).
fn each_part_of_the_socket_rules_has_its_own_opt_in() {
    const SOCK_PACKET: i32 = 10;
    let raw_part = [
        (libc::AF_INET, libc::SOCK_RAW | libc::SOCK_CLOEXEC, libc::IPPROTO_ICMP),
        (libc::AF_INET6, libc::SOCK_RAW, libc::IPPROTO_ICMPV6),
        (libc::AF_PACKET, libc::SOCK_RAW, i32::from(0x0003u16.to_be())),
        (libc::AF_PACKET, libc::SOCK_DGRAM | libc::SOCK_NONBLOCK, i32::from(0x0800u16.to_be())),
        (libc::AF_INET, SOCK_PACKET, i32::from(0x0003u16.to_be())),
    ];
    let ping_part = [
        (libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_NONBLOCK, libc::IPPROTO_ICMP),
        (libc::AF_INET6, libc::SOCK_DGRAM, libc::IPPROTO_ICMPV6),
    ];
    let mut answers = Vec::new();
    for calls in [Calls::sockets(), Calls::sockets().allow_raw(), Calls::sockets().allow_ping()] {
        for (part, denied) in [(&raw_part[..], calls.denies_raw()), (&ping_part[..], calls.denies_ping())] {
            for &(family, kind, protocol) in part {
                let Some((seen, code)) = open_held(&calls, family, kind, protocol) else { return };
                let what = format!("socket({family}, {kind:#x}, {protocol})");
                if denied {
                    assert_eq!(
                        (seen, code),
                        (0, libc::EPERM),
                        "{what}: the filter answers EPERM, with no notification"
                    );
                } else {
                    assert_eq!(seen, 1, "{what}: the opt-in gives the call to the supervisor");
                    answers.push(format!("{what}: {code}"));
                }
            }
        }
    }
    println!("the kernel answered the allowed sockets (0: a socket): {}", answers.join(", "));
    println!("ok: each_part_of_the_socket_rules_has_its_own_opt_in");
}

/// The netns hold: a child enters a new network namespace, makes the TUN,
/// and gives it to the parent; the parent maps the ids.
fn a_namespace_gives_its_tun_to_the_parent() {
    let (parent, child) = netns::socketpair().expect("a pair");
    // SAFETY: this process has one thread here; the child exits with _exit.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        drop(parent);
        let code = match netns::enter() {
            Err(_) => 10,
            Ok(userns) => match netns::announce(child.as_raw_fd(), userns) {
                Err(_) => 11,
                Ok(()) => match netns::bringup_loopback().and_then(|()| netns::create_tun(&netns::Tun::default())) {
                    Err(_) => 12,
                    Ok(tun) => match netns::send_fd(child.as_raw_fd(), tun.as_raw_fd()) {
                        Ok(()) => 0,
                        Err(_) => 13,
                    },
                },
            },
        };
        // SAFETY: the end of the child.
        unsafe { libc::_exit(code) };
    }
    drop(child);
    // SAFETY: getuid and getgid do not fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let userns = match netns::answer(parent.as_raw_fd(), pid as u32, uid, gid) {
        Ok(userns) => userns,
        Err(e) => {
            let code = wait(pid);
            println!("SKIP: the host refuses a network namespace (child exit {code}): {}", e.chain());
            return;
        }
    };
    let tun: Result<OwnedFd, _> = netns::recv_fd(parent.as_raw_fd());
    let code = wait(pid);
    let Ok(tun) = tun else {
        println!("SKIP: the namespace made no TUN (child exit {code})");
        return;
    };
    assert_eq!(code, 0);
    assert_eq!(netns::Wire::of(tun.as_raw_fd()).expect("fstat"), netns::Wire::Tun);
    let hold = pod_hold::Hold::netns(pid, userns, netns::Wire::Tun, "eth0");
    assert_eq!(hold.level, Level::Enforced);
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", tun.as_raw_fd())).expect("fdinfo");
    assert!(info.contains("iff:"), "the descriptor is a TUN: {info}");
    println!("ok: a_namespace_gives_its_tun_to_the_parent");
}

/// Refuse `socket(AF_INET or AF_INET6, SOCK_DGRAM)` with `EPERM` in this
/// process, as the lab `udp-denied` of `pod-lab` does. Each other call runs.
/// False when the host refuses the filter.
fn refuse_udp_sockets() -> bool {
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xC000_00B7;
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    return false;
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        const LD_ABS: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
        const JEQ: u16 = 0x15; // BPF_JMP | BPF_JEQ | BPF_K
        const AND: u16 = 0x54; // BPF_ALU | BPF_AND | BPF_K
        const RET: u16 = 0x06; // BPF_RET | BPF_K
        const ERRNO: u32 = 0x0005_0000;
        const ALLOW: u32 = 0x7FFF_0000;
        let op = |code: u16, jt: u8, jf: u8, k: u32| libc::sock_filter { code, jt, jf, k };
        // The offsets of `struct seccomp_data`: the number 0, the arch 4,
        // the low half of argument 0 at 16 and of argument 1 at 24. A jump
        // counts from the next instruction; the last instruction allows.
        let program = [
            op(LD_ABS, 0, 0, 4),
            op(JEQ, 0, 9, ARCH),
            op(LD_ABS, 0, 0, 0),
            op(JEQ, 0, 7, libc::SYS_socket as u32),
            op(LD_ABS, 0, 0, 16),
            op(JEQ, 1, 0, libc::AF_INET as u32),
            op(JEQ, 0, 4, libc::AF_INET6 as u32),
            op(LD_ABS, 0, 0, 24),
            op(AND, 0, 0, 0xF),
            op(JEQ, 0, 1, libc::SOCK_DGRAM as u32),
            op(RET, 0, 0, ERRNO | libc::EPERM as u32),
            op(RET, 0, 0, ALLOW),
        ];
        let prog = libc::sock_fprog { len: program.len() as u16, filter: program.as_ptr() as *mut _ };
        // SAFETY: prctl of this process; the program lives for the call.
        unsafe {
            libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0
                && libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &prog as *const libc::sock_fprog) == 0
        }
    }
}

/// The netns hold in the lab `udp-denied`: the host refuses each UDP
/// socket, and the namespace still brings up `lo` and makes its wire (the
/// TUN, else the veth pair) with its address and route. The child exits 20
/// when the interfaces fail.
fn a_namespace_makes_its_wire_where_udp_sockets_are_refused() {
    const NAME: &str = "a_namespace_makes_its_wire_where_udp_sockets_are_refused";
    let (parent, child) = netns::socketpair().expect("a pair");
    // SAFETY: this process has one thread here; the child exits with _exit.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        drop(parent);
        let code = (|| {
            if !refuse_udp_sockets() {
                return 30;
            }
            // SAFETY: a plain system call; the descriptor is not kept.
            let udp = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
            if udp >= 0 || errno() != libc::EPERM {
                return 31;
            }
            let Ok(userns) = netns::enter() else { return 10 };
            if netns::announce(child.as_raw_fd(), userns).is_err() {
                return 11;
            }
            let tun = netns::Tun::default();
            let made = netns::bringup_loopback().and_then(|()| netns::open_wire(&tun, netns::WireChoice::Auto));
            match made {
                Ok(_) => 0,
                Err(e) => {
                    let chain = e.chain();
                    eprintln!("{NAME}: child: {chain}");
                    // The refused socket of the ioctls is the defect. An ioctl
                    // that gives EPERM is the host: its namespace holds no
                    // CAP_NET_ADMIN (the filter refuses no ioctl). No TUN and no
                    // veth pair is the host too.
                    if chain.contains("for the interfaces") {
                        20
                    } else if chain.contains("ioctl SIOC") && chain.contains("Operation not permitted") {
                        13
                    } else {
                        12
                    }
                }
            }
        })();
        // SAFETY: the end of the child.
        unsafe { libc::_exit(code) };
    }
    drop(child);
    // SAFETY: getuid and getgid do not fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    if let Err(e) = netns::answer(parent.as_raw_fd(), pid as u32, uid, gid) {
        let code = wait(pid);
        assert_ne!(code, 31, "the filter does not refuse a UDP socket");
        let why = if code == 30 { "a seccomp filter" } else { "a network namespace" };
        println!("SKIP: {NAME}: the host refuses {why} (child exit {code}): {}", e.chain());
        return;
    }
    match wait(pid) {
        0 => println!("ok: {NAME}"),
        12 => println!("SKIP: {NAME}: the namespace has no TUN and no veth wire"),
        13 => println!("SKIP: {NAME}: the namespace of this caller holds no CAP_NET_ADMIN"),
        code => panic!("{NAME}: the interfaces fail where UDP sockets are refused (child exit {code})"),
    }
}

/// A failed `execve` is a failure of `start` with the errno and its class,
/// not a child that exits 127 with no word.
fn a_program_that_cannot_run_gives_the_errno_of_execve() {
    const NAME: &str = "a_program_that_cannot_run_gives_the_errno_of_execve";
    let calls = Calls::of(&[libc::SYS_connect]);
    let Some(good) = payload("payload-none", &calls) else { return };
    wait(good.pid);
    let dir = std::env::temp_dir().join(format!("pod-hold-exec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let text = dir.join("text");
    std::fs::write(&text, "not a program\n").expect("a file");
    std::fs::set_permissions(&text, std::fs::Permissions::from_mode(0o755)).expect("a mode");
    let rows = [
        (dir.join("absent"), Class::NotFound, "ENOENT"),
        (dir.clone(), Class::Permission, "EACCES"),
        (text, Class::Unsupported, "ENOEXEC"),
    ];
    for (path, class, word) in rows {
        let program = Program::new(&path, &["x"], &[] as &[&str]).expect("a program");
        match start(program, &calls) {
            Ok(started) => panic!("{NAME}: {path:?} started (child exit {})", wait(started.pid)),
            Err(e) => {
                let line = e.chain().to_string();
                assert_eq!(e.class, class, "{NAME}: {line}");
                assert!(line.contains(word) && line.contains(path.to_str().unwrap()), "{NAME}: {line}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    println!("ok: {NAME}");
}

pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("payload-connect") => std::process::exit(payload_connect(9)),
        Some("payload-socket") => std::process::exit(payload_socket()),
        Some("payload-raw") => std::process::exit(payload_raw()),
        Some("payload-open") => std::process::exit(payload_open(&args[2..])),
        Some("payload-none") => std::process::exit(0),
        _ => {}
    }
    // The fork of the namespace test comes first, while the process has one
    // thread.
    a_namespace_gives_its_tun_to_the_parent();
    a_namespace_makes_its_wire_where_udp_sockets_are_refused();
    a_refused_connect_fails_with_the_errno_of_the_reply();
    a_socket_call_gets_the_descriptor_of_the_supervisor();
    a_raw_icmp_socket_gets_eperm_unless_the_hold_allows_it();
    each_part_of_the_socket_rules_has_its_own_opt_in();
    a_program_that_cannot_run_gives_the_errno_of_execve();
}
