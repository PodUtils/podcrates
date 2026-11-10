//! The holds of a real payload. The binary is the supervisor and, with a
//! first argument `payload-*`, the payload. A host that refuses a hold
//! (no seccomp listener, no user namespace, no TUN) skips its test with a
//! line that says so.

#![cfg(target_os = "linux")]

use std::os::fd::{AsRawFd, OwnedFd};
use std::time::Duration;

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

fn wait(pid: i32) -> i32 {
    let mut status = 0;
    // SAFETY: waitpid of a child of this process.
    unsafe { libc::waitpid(pid, &mut status, 0) };
    if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 128 + libc::WTERMSIG(status) }
}

fn payload(part: &str, calls: &Calls) -> Option<Started> {
    let me = std::env::current_exe().expect("the test binary");
    let program = Program::new(&me, &["hold", part], &[] as &[&str]).expect("a program");
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
    let hold = pod_hold::Hold::netns(pid, userns, "eth0");
    assert_eq!(hold.level, Level::Enforced);
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", tun.as_raw_fd())).expect("fdinfo");
    assert!(info.contains("iff:"), "the descriptor is a TUN: {info}");
    println!("ok: a_namespace_gives_its_tun_to_the_parent");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("payload-connect") => std::process::exit(payload_connect(9)),
        Some("payload-socket") => std::process::exit(payload_socket()),
        _ => {}
    }
    // The fork of the namespace test comes first, while the process has one
    // thread.
    a_namespace_gives_its_tun_to_the_parent();
    a_refused_connect_fails_with_the_errno_of_the_reply();
    a_socket_call_gets_the_descriptor_of_the_supervisor();
}
