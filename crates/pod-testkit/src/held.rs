//! A Unix listener that a forked child holds until its exec. A child that
//! another thread forks keeps a copy of each listener of its parent until
//! its exec, also a listener with `CLOEXEC`. While the copy is open, a
//! socket that its server closed still takes a connect. A probe of one
//! connect then reads the stale socket as a live one.
//!
//! The child of [`hold_until_exec`] takes one connect on its copy, and then
//! runs its exec. Thus the first connect of a probe always lands in the
//! window, and no time of the host decides the result. The child holds a
//! copy of each other descriptor of the test process too, until that
//! connect: probe at once after the call.

use std::io::Read as _;
use std::os::fd::AsRawFd as _;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt as _;
use std::process::ExitStatus;
use std::thread::JoinHandle;

/// The longest time that the child waits for the connect before its exec.
const WAIT_MS: libc::c_int = 10_000;

/// A child that holds a copy of a listener. Join it at the end of the test.
pub struct Held {
    child: JoinHandle<std::io::Result<ExitStatus>>,
}

impl Held {
    /// The exit status of the child, after its exec of `true`.
    pub fn join(self) -> ExitStatus {
        self.child.join().expect("the thread of the child").expect("the child of the test")
    }
}

/// Fork a child that holds a copy of `listener`, and return when the child
/// holds it. The child takes one connect (at most 10 s after its
/// fork), then runs its exec, which closes the copy. The caller closes its
/// own listener next: the socket is then stale, but it takes one connect.
pub fn hold_until_exec(listener: &UnixListener) -> Held {
    let fd = listener.as_raw_fd();
    let (mut ready, told) = std::io::pipe().expect("a pipe");
    let told_fd = told.as_raw_fd();
    // spawn() returns only after the exec of the child: spawn on a thread.
    let child = std::thread::spawn(move || {
        let mut command = std::process::Command::new("true");
        // SAFETY: between the fork and the exec the closure calls only
        // write, poll, accept and close, which are async-signal-safe; it
        // takes no lock and allocates nothing.
        unsafe {
            command.pre_exec(move || {
                libc::write(told_fd, b"h".as_ptr().cast(), 1);
                let mut poll = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
                if libc::poll(&mut poll, 1, WAIT_MS) == 1 {
                    let taken = libc::accept(fd, std::ptr::null_mut(), std::ptr::null_mut());
                    if taken >= 0 {
                        libc::close(taken);
                    }
                }
                Ok(())
            });
        }
        let status = command.status();
        drop(told);
        status
    });
    let mut byte = [0u8; 1];
    // A child that fails before its write closes the pipe: no byte.
    ready.read_exact(&mut byte).expect("the child holds the listener");
    Held { child }
}
