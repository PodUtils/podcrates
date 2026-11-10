//! The reply to a notification: the seam of the hold. The notification
//! descriptor answers with `SECCOMP_IOCTL_NOTIF_SEND` and adds descriptors
//! with `SECCOMP_IOCTL_NOTIF_ADDFD`; [`Recorded`] keeps the answers in
//! memory for a test or a fuzz target.

use std::os::fd::RawFd;

use pod_core::{Class, Failure};

use crate::abi::{
    SECCOMP_ADDFD_FLAG_SEND, SECCOMP_ADDFD_FLAG_SETFD, SECCOMP_IOCTL_NOTIF_ADDFD, SECCOMP_IOCTL_NOTIF_ID_VALID,
    SECCOMP_IOCTL_NOTIF_SEND, SECCOMP_USER_NOTIF_FLAG_CONTINUE, SeccompNotifAddfd, SeccompNotifResp,
};
use crate::failure;

/// The answers to a notification. The notification descriptor
/// ([`RawFd`]) answers with `SECCOMP_IOCTL_NOTIF_SEND`.
pub trait Answer: Copy {
    /// The system call returns `val`, or fails with the errno `error` (a
    /// positive errno, for example `libc::EPERM`).
    fn respond(self, id: u64, val: i64, error: i32) -> Result<(), Failure>;
    /// The kernel runs the system call. A hold must not use this answer as
    /// a permission: the task can change the memory that the supervisor
    /// read before the kernel reads it.
    fn proceed(self, id: u64) -> Result<(), Failure>;
}

/// `SECCOMP_IOCTL_NOTIF_SEND` of `resp` on `lfd`. ENOENT is no failure: the
/// task died, or a signal stopped the call.
fn send(lfd: RawFd, resp: &SeccompNotifResp, what: &str) -> Result<(), Failure> {
    // SAFETY: `resp` is a valid `struct seccomp_notif_resp`.
    let r = unsafe { libc::ioctl(lfd, SECCOMP_IOCTL_NOTIF_SEND as _, resp) };
    if r < 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ENOENT) {
            return Ok(());
        }
        return Err(failure::errno(Class::Unavailable, what, e.raw_os_error().unwrap_or(libc::EIO)));
    }
    Ok(())
}

impl Answer for RawFd {
    fn respond(self, id: u64, val: i64, error: i32) -> Result<(), Failure> {
        // The kernel takes a nonzero error as the return value of the system
        // call, so it must be the negative errno: a positive errno is a
        // positive return, and a refused connect returns 111 as a success.
        let resp = SeccompNotifResp { id, val, error: -error.abs(), flags: 0 };
        send(self, &resp, "SECCOMP_IOCTL_NOTIF_SEND")
    }

    fn proceed(self, id: u64) -> Result<(), Failure> {
        let resp = SeccompNotifResp { id, val: 0, error: 0, flags: SECCOMP_USER_NOTIF_FLAG_CONTINUE };
        send(self, &resp, "SECCOMP_IOCTL_NOTIF_SEND (CONTINUE)")
    }
}

/// Answer the notification `id`: the system call returns `val`, or fails
/// with the errno `error` (a positive errno, for example `libc::EPERM`).
pub fn respond(lfd: impl Answer, id: u64, val: i64, error: i32) -> Result<(), Failure> {
    lfd.respond(id, val, error)
}

/// Let the kernel run the system call of the notification `id`.
pub fn proceed(lfd: impl Answer, id: u64) -> Result<(), Failure> {
    lfd.proceed(id)
}

fn addfd(lfd: RawFd, req: &SeccompNotifAddfd, what: &str) -> Result<i32, Failure> {
    // SAFETY: `req` is a valid `struct seccomp_notif_addfd`.
    let r = unsafe { libc::ioctl(lfd, SECCOMP_IOCTL_NOTIF_ADDFD as _, req) };
    if r < 0 {
        return Err(failure::last_os(what));
    }
    Ok(r)
}

/// Install `srcfd` in the task of the notification `id`, and answer the
/// call with its number (`SECCOMP_ADDFD_FLAG_SEND`, Linux 5.14).
pub fn addfd_send(lfd: RawFd, id: u64, srcfd: RawFd, newfd_flags: u32) -> Result<i32, Failure> {
    let req = SeccompNotifAddfd { id, flags: SECCOMP_ADDFD_FLAG_SEND, srcfd: srcfd as u32, newfd: 0, newfd_flags };
    addfd(lfd, &req, "SECCOMP_IOCTL_NOTIF_ADDFD")
}

/// Install `srcfd` in the task at the number `target`, in place of the fd
/// that has that number (the call is not answered).
pub fn addfd_setfd(lfd: RawFd, id: u64, srcfd: RawFd, target: i32, newfd_flags: u32) -> Result<i32, Failure> {
    let req = SeccompNotifAddfd {
        id,
        flags: SECCOMP_ADDFD_FLAG_SETFD,
        srcfd: srcfd as u32,
        newfd: target as u32,
        newfd_flags,
    };
    addfd(lfd, &req, "SECCOMP_IOCTL_NOTIF_ADDFD (SETFD)")
}

/// True while the notification `id` is still pending: the task that made
/// the call lives, and the call waits. A pid can be used again after its
/// task exits, so a read of `/proc/PID/mem` is of the task only when this
/// is true after the open.
pub fn id_valid(lfd: RawFd, id: u64) -> bool {
    let mut id = id;
    // SAFETY: the kernel reads one u64.
    unsafe { libc::ioctl(lfd, SECCOMP_IOCTL_NOTIF_ID_VALID as _, &mut id) == 0 }
}

/// The file status flags of the fd `fd` of the task `pid`
/// (`/proc/<pid>/fdinfo/<fd>`, the line `flags:` in octal).
pub fn child_fd_flags(pid: u32, fd: i32) -> Option<i32> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/fdinfo/{fd}")).ok()?;
    let value = text.lines().find_map(|l| l.strip_prefix("flags:"))?;
    i32::from_str_radix(value.trim(), 8).ok()
}

/// Set `O_NONBLOCK` on `fd`.
pub fn set_nonblocking(fd: RawFd) -> Result<(), Failure> {
    // SAFETY: fcntl on a descriptor; a bad one gives EBADF.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(failure::last_os("fcntl(F_GETFL)"));
    }
    // SAFETY: as above.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(failure::last_os("fcntl(F_SETFL)"));
    }
    Ok(())
}

/// One recorded answer: the id, and the return value and the errno
/// (`None`: the kernel runs the call).
pub type Reply = (u64, Option<(i64, i32)>);

/// The answers of a handler, in memory, for a test or a fuzz target.
#[derive(Default, Debug)]
pub struct Recorded(std::cell::RefCell<Vec<Reply>>);

impl Recorded {
    /// The answers in their order.
    pub fn replies(&self) -> Vec<Reply> {
        self.0.borrow().clone()
    }
}

impl Answer for &Recorded {
    fn respond(self, id: u64, val: i64, error: i32) -> Result<(), Failure> {
        self.0.borrow_mut().push((id, Some((val, error))));
        Ok(())
    }

    fn proceed(self, id: u64) -> Result<(), Failure> {
        self.0.borrow_mut().push((id, None));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recorded_answers_keep_their_order() {
        let answers = Recorded::default();
        respond(&answers, 1, -1, libc::EPERM).unwrap();
        proceed(&answers, 2).unwrap();
        assert_eq!(answers.replies(), vec![(1, Some((-1, libc::EPERM))), (2, None)]);
    }

    // A reply on a descriptor that is no notification descriptor fails, and
    // the failure names the ioctl.
    #[test]
    fn a_reply_on_another_descriptor_fails() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let fd = std::os::fd::AsRawFd::as_raw_fd(&a);
        let e = respond(fd, 1, 0, 0).unwrap_err();
        assert!(e.text().contains("NOTIF_SEND"), "{e}");
        assert!(!id_valid(fd, 1));
        assert!(addfd_send(fd, 1, fd, 0).is_err());
    }
}
