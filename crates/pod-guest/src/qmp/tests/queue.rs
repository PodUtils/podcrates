//! A connection to a monitor whose queue is full. QEMU serves one client
//! at a time, and its QMP socket queues one more connection (`listen` with
//! a backlog of 1). A blocking `connect` to a full queue waits in the
//! kernel with no bound (PodUtils/podvm#633).

use std::io::ErrorKind;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use socket2::{Domain, SockAddr, Socket, Type};

use super::super::*;

/// The time limit of a call of a test: much longer than each bound of the
/// call, so that only a wait with no bound reaches it.
const LIMIT: Duration = Duration::from_secs(20);

/// A listener on `sock` with the queue of the QMP socket of QEMU: one
/// connection that is not taken yet (`listen` with a backlog of 1).
pub(crate) fn qemu_listener(sock: &Path) -> UnixListener {
    let _ = std::fs::remove_file(sock);
    let s = Socket::new(Domain::UNIX, Type::STREAM, None).expect("a Unix socket");
    s.bind(&SockAddr::unix(sock).expect("a socket path")).expect("the bind of the QMP socket");
    s.listen(1).expect("listen with a backlog of 1");
    UnixListener::from(std::os::fd::OwnedFd::from(s))
}

/// Connections to `sock` until its queue is full. Each one waits in the
/// queue while the caller holds it: a connection that closes stays in the
/// queue until the server takes it.
pub(crate) fn fill(sock: &Path) -> Vec<Socket> {
    let addr = SockAddr::unix(sock).expect("a socket path");
    let mut held = Vec::new();
    for _ in 0..64 {
        let s = Socket::new(Domain::UNIX, Type::STREAM, None).expect("a Unix socket");
        s.set_nonblocking(true).expect("O_NONBLOCK");
        match s.connect(&addr) {
            Ok(()) => held.push(s),
            Err(e) if e.kind() == ErrorKind::WouldBlock => return held,
            Err(e) => panic!("a connection that fills the queue of {}: {e}", sock.display()),
        }
    }
    panic!("the queue of {} is never full", sock.display());
}

/// `call` in a thread of its own, and its result with its time; `None`
/// when it did not end within `limit`. A call that waits with no bound
/// fails the test at `limit`, and its thread stays.
pub(crate) fn within<T: Send + 'static>(
    limit: Duration,
    call: impl FnOnce() -> T + Send + 'static,
) -> Option<(T, Duration)> {
    let (tx, rx) = mpsc::channel();
    let at = Instant::now();
    std::thread::spawn(move || {
        let _ = tx.send(call());
    });
    rx.recv_timeout(limit).ok().map(|got| (got, at.elapsed()))
}

fn sock(tag: &str) -> PathBuf {
    let dir = pod_testkit::host::socket_dir(&format!("pod-guest-queue-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("qmp.sock")
}

/// A connection to a full queue ends within the bound of the caller with
/// the line of the full queue: it does not wait in the kernel.
#[test]
fn a_connect_to_a_full_queue_ends_within_its_bound_and_says_the_queue_is_full() {
    pod_testkit::need_loopback!();
    let sock = sock("full");
    let _l = qemu_listener(&sock);
    let _held = fill(&sock);
    let bound = Duration::from_millis(300);
    let path = sock.clone();
    let (got, took) = within(LIMIT, move || Monitor::connect(&path, bound).map(drop))
        .unwrap_or_else(|| panic!("the connect did not end in {LIMIT:?}: it waits in the kernel"));
    let e = got.expect_err("a full queue gives no connection");
    assert_eq!(e.class, Class::Timeout, "{e}");
    assert!(e.to_string().contains("queue") && e.to_string().contains("is full"), "{e}");
    assert!(took >= bound && took < bound + Duration::from_secs(2), "{took:?}");
    let (got, _) = within(LIMIT, move || Monitor::connect(&sock, Duration::ZERO).map(drop)).expect("one try");
    assert!(got.is_err_and(|e| e.to_string().contains("is full")), "a zero bound is one try");
}

/// A missing socket and a socket with no listener each have their own
/// line at the bound.
#[test]
fn a_missing_socket_and_a_socket_with_no_listener_each_have_their_line() {
    pod_testkit::need_loopback!();
    let sock = sock("cause");
    let _ = std::fs::remove_file(&sock);
    let e = Monitor::connect(&sock, Duration::from_millis(200)).err().expect("no socket");
    assert!(e.to_string().contains("never appeared"), "{e}");
    // The file of a socket stays after its listener closes.
    drop(qemu_listener(&sock));
    let e = Monitor::connect(&sock, Duration::from_millis(200)).err().expect("no listener");
    assert!(e.to_string().contains("no emulator listens"), "{e}");
    assert_eq!(e.class, Class::Timeout, "{e}");
}

/// A connection in the queue behind an open one gets no greeting: the
/// failure says so at the bound of the greeting.
#[test]
fn a_connection_in_the_queue_gets_no_greeting_and_says_why() {
    pod_testkit::need_loopback!();
    let sock = sock("queued");
    let _l = qemu_listener(&sock);
    let path = sock.clone();
    let (got, took) = within(LIMIT, move || Monitor::connect(&path, Duration::from_millis(300)).map(drop))
        .unwrap_or_else(|| panic!("the connect did not end in {LIMIT:?}"));
    let e = got.expect_err("no greeting");
    assert_eq!(e.class, Class::Timeout, "{e}");
    assert!(e.to_string().contains("no greeting") && e.to_string().contains("another open connection"), "{e}");
    assert!(took < ANSWER_WAIT + Duration::from_secs(2), "{took:?}");
}
