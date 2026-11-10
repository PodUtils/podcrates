//! The descriptor passing of the netns hold: one descriptor over a unix
//! socket with `SCM_RIGHTS` (the child sends the TUN to the parent).

use std::os::fd::{FromRawFd, OwnedFd, RawFd};

use pod_core::{Class, Failure};

use crate::failure;

/// A control-message buffer with the alignment of `cmsghdr`.
///
/// A plain `[u8; N]` has alignment 1, so `CMSG_FIRSTHDR` can return a
/// misaligned `*mut cmsghdr`. A debug build aborts on that dereference.
#[repr(C, align(8))]
struct CmsgBuf([u8; 64]);

/// [`send_fd`] for a child after `fork()` in a multi-threaded process: it
/// does not allocate. 0, or -1 on an error.
///
/// # Safety
///
/// `sock` and `fd` are open descriptors.
pub unsafe fn send_fd_raw(sock: RawFd, fd: RawFd) -> libc::c_int {
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec { iov_base: byte.as_mut_ptr() as *mut _, iov_len: 1 };
    let mut cmsg = CmsgBuf([0u8; 64]);
    let fd_len = size_of::<RawFd>() as u32;
    // SAFETY: a zeroed msghdr is valid; the fields below point to live
    // buffers of this frame.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg.0.as_mut_ptr() as *mut libc::c_void;
    // SAFETY: CMSG_SPACE is arithmetic.
    msg.msg_controllen = unsafe { libc::CMSG_SPACE(fd_len) } as _;
    // SAFETY: the control buffer holds one header and one descriptor.
    unsafe {
        let hdr = libc::CMSG_FIRSTHDR(&msg);
        if hdr.is_null() {
            return -1;
        }
        (*hdr).cmsg_level = libc::SOL_SOCKET;
        (*hdr).cmsg_type = libc::SCM_RIGHTS;
        (*hdr).cmsg_len = libc::CMSG_LEN(fd_len) as _;
        std::ptr::copy_nonoverlapping((&fd as *const RawFd).cast::<u8>(), libc::CMSG_DATA(hdr), fd_len as usize);
        if libc::sendmsg(sock, &msg, 0) < 0 { -1 } else { 0 }
    }
}

/// Send one fd over a unix socket with `SCM_RIGHTS`.
pub fn send_fd(sock: RawFd, fd: RawFd) -> Result<(), Failure> {
    // SAFETY: the caller gives open descriptors; a bad one gives EBADF.
    match unsafe { send_fd_raw(sock, fd) } {
        0 => Ok(()),
        _ => Err(failure::last_os("sendmsg(SCM_RIGHTS)")),
    }
}

/// Receive one fd over a unix socket. A message with no descriptor, with
/// more than one, or with a cut control part is refused, and each
/// descriptor in it is closed.
pub fn recv_fd(sock: RawFd) -> Result<OwnedFd, Failure> {
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec { iov_base: byte.as_mut_ptr() as *mut _, iov_len: 1 };
    let mut cmsg = CmsgBuf([0u8; 64]);
    // SAFETY: a zeroed msghdr is valid.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg.0.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = cmsg.0.len() as _;
    // SAFETY: the buffers live for the call.
    let n = unsafe { libc::recvmsg(sock, &mut msg, libc::MSG_CMSG_CLOEXEC) };
    if n < 0 {
        return Err(failure::last_os("recvmsg(SCM_RIGHTS)"));
    }
    if n == 0 {
        return Err(failure::host(Class::Unavailable, "the peer closed the socket before it sent a descriptor"));
    }
    let mut fds = Vec::new();
    // SAFETY: the kernel filled the control buffer; each header is in it.
    unsafe {
        let mut hdr = libc::CMSG_FIRSTHDR(&msg);
        while !hdr.is_null() {
            if (*hdr).cmsg_level == libc::SOL_SOCKET && (*hdr).cmsg_type == libc::SCM_RIGHTS {
                let data = libc::CMSG_DATA(hdr);
                let len = ((*hdr).cmsg_len as usize).saturating_sub(data.offset_from(hdr.cast::<u8>()) as usize);
                for i in 0..len / size_of::<RawFd>() {
                    let mut fd: RawFd = -1;
                    // Copy bytes: the data has no alignment.
                    std::ptr::copy_nonoverlapping(
                        data.add(i * size_of::<RawFd>()),
                        (&mut fd as *mut RawFd).cast::<u8>(),
                        size_of::<RawFd>(),
                    );
                    fds.push(OwnedFd::from_raw_fd(fd));
                }
            }
            hdr = libc::CMSG_NXTHDR(&msg, hdr);
        }
    }
    if msg.msg_flags & libc::MSG_CTRUNC != 0 {
        return Err(failure::host(Class::Protocol, "the control message of the descriptor was cut"));
    }
    match fds.len() {
        1 => Ok(fds.remove(0)),
        0 => Err(failure::host(Class::Protocol, "no descriptor in the received message")),
        n => Err(failure::host(Class::Protocol, format!("{n} descriptors in the message, not one"))),
    }
}
