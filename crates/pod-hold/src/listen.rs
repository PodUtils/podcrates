//! The receive part of the loop of the supervisor: wait for a pending
//! notification, take it (`SECCOMP_IOCTL_NOTIF_RECV`), open the memory of
//! its task, and check that the task still waits.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use pod_core::Failure;
use tokio::io::unix::AsyncFd;

use crate::abi::{SECCOMP_IOCTL_NOTIF_RECV, SeccompNotif};
use crate::failure;
use crate::mem::{Access, Memory};
use crate::reply::{id_valid, set_nonblocking};

/// One stopped system call, with the memory of its task.
#[derive(Debug)]
pub struct Notification {
    /// The id of the reply.
    pub id: u64,
    /// The task (a thread id).
    pub pid: u32,
    /// The number of the system call.
    pub nr: i32,
    /// The six arguments.
    pub args: [u64; 6],
    /// The memory of the task, open for this call. The supervisor checked
    /// after the open that the call still waits: the pid is the task of the
    /// call, not a task that took the pid again.
    pub memory: Memory,
}

/// The notification descriptor of a filter, on the reactor of tokio.
#[derive(Debug)]
pub struct Listener {
    fd: OwnedFd,
    ready: AsyncFd<OwnedFd>,
    access: Access,
}

impl Listener {
    /// The listener of the notification descriptor `fd` (of
    /// [`crate::filter::install`] or [`crate::spawn::start`]). Call it in a
    /// runtime of tokio with I/O.
    pub fn new(fd: OwnedFd, access: Access) -> Result<Listener, Failure> {
        // NOTIF_RECV blocks even on an O_NONBLOCK descriptor, so the loop
        // calls it only when poll(2) reports a pending notification.
        set_nonblocking(fd.as_raw_fd())?;
        // SAFETY: dup of an open descriptor.
        let copy = unsafe { libc::dup(fd.as_raw_fd()) };
        if copy < 0 {
            return Err(failure::last_os("dup"));
        }
        // SAFETY: a new descriptor that nothing else owns.
        let copy = unsafe { OwnedFd::from_raw_fd(copy) };
        // SAFETY: the OwnedFd holds `copy` open and returns it from
        // as_raw_fd until the AsyncFd drops it.
        let ready = unsafe { AsyncFd::register_with_interest(copy, tokio::io::Interest::READABLE) }
            .map_err(|e| failure::host(pod_core::Class::Unavailable, format!("the reactor: {e}")))?;
        Ok(Listener { fd, ready, access })
    }

    /// The notification descriptor: the replies go to it.
    pub fn raw(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// The next notification, or `None` when no task uses the filter any
    /// more (or the descriptor fails). A notification whose task left the
    /// call before the supervisor took it is skipped. The future can be
    /// dropped at each `await` with no loss: the receive itself does not
    /// wait.
    pub async fn recv(&self) -> Option<Notification> {
        loop {
            let mut guard = self.ready.readable().await.ok()?;
            match self.take() {
                Taken::Call(n) => return Some(n),
                Taken::Again => {}
                Taken::Idle => guard.clear_ready(),
                Taken::Ended => return None,
            }
        }
    }

    /// True when no task uses the filter (POLLHUP).
    pub fn unused(&self) -> bool {
        filter_unused(self.raw())
    }

    fn take(&self) -> Taken {
        let lfd = self.raw();
        let mut pfd = libc::pollfd { fd: lfd, events: libc::POLLIN, revents: 0 };
        // SAFETY: one pollfd; no wait.
        unsafe { libc::poll(&mut pfd, 1, 0) };
        if pfd.revents & libc::POLLIN == 0 {
            if pfd.revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                return Taken::Ended;
            }
            return Taken::Idle;
        }
        let mut req = SeccompNotif::default();
        // SAFETY: `req` is a valid `struct seccomp_notif`.
        let r = unsafe { libc::ioctl(lfd, SECCOMP_IOCTL_NOTIF_RECV as _, &mut req) };
        if r < 0 {
            return match std::io::Error::last_os_error().raw_os_error() {
                // ENOENT: the task left the call before it was read.
                Some(libc::EAGAIN) | Some(libc::EINTR) | Some(libc::ENOENT) => Taken::Again,
                _ => Taken::Ended,
            };
        }
        // Open the memory of the task that made this call, for each call. A
        // forked grandchild has a different pid.
        let memory = Memory::open(req.pid, self.access);
        // The pid can be used again after the task exits. NOTIF_ID_VALID
        // makes sure the task that made this call is still alive after the
        // open. A task that left needs no reply.
        if !id_valid(lfd, req.id) {
            return Taken::Again;
        }
        Taken::Call(Notification { id: req.id, pid: req.pid, nr: req.data.nr, args: req.data.args, memory })
    }
}

enum Taken {
    Call(Notification),
    Again,
    Idle,
    Ended,
}

/// True when no task uses the filter of the notification descriptor `lfd`
/// (POLLHUP).
pub fn filter_unused(lfd: RawFd) -> bool {
    let mut pfd = libc::pollfd { fd: lfd, events: libc::POLLIN, revents: 0 };
    // SAFETY: one pollfd; no wait.
    unsafe { libc::poll(&mut pfd, 1, 0) };
    pfd.revents & libc::POLLHUP != 0
}
