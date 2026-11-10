//! The seccomp filter of the hold: the list of the stopped system calls and
//! the BPF program that returns `SECCOMP_RET_USER_NOTIF` for each of them.

use std::os::fd::{FromRawFd, OwnedFd};

use pod_core::{Class, Failure};
use pod_hostprobe::sys;

use crate::abi::{AUDIT_ARCH_NATIVE, SYS_ACCEPT, X32_SYSCALL_BIT};
use crate::failure;

/// The system calls that the filter stops. Each other call runs as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calls {
    numbers: Vec<libc::c_long>,
    /// A `recvfrom` with no source buffer runs in the kernel with no stop.
    recvfrom_with_source_only: bool,
}

impl Calls {
    /// The calls of a hold of each socket of a payload: the socket calls,
    /// `close`, and the three calls of `io_uring`, whose operations do not
    /// go through the filter (the handler refuses them).
    pub fn sockets() -> Calls {
        Calls {
            numbers: vec![
                libc::SYS_socket,
                libc::SYS_recvfrom,
                libc::SYS_recvmsg,
                libc::SYS_recvmmsg,
                libc::SYS_connect,
                libc::SYS_sendto,
                libc::SYS_sendmsg,
                libc::SYS_sendmmsg,
                libc::SYS_bind,
                libc::SYS_getsockname,
                libc::SYS_getpeername,
                libc::SYS_setsockopt,
                libc::SYS_getsockopt,
                libc::SYS_io_uring_setup,
                libc::SYS_close,
                libc::SYS_listen,
                SYS_ACCEPT,
                libc::SYS_accept4,
                libc::SYS_io_uring_enter,
                libc::SYS_io_uring_register,
                // An unconnected TCP socket gets ENOTCONN.
                libc::SYS_shutdown,
            ],
            recvfrom_with_source_only: true,
        }
    }

    /// A list of the caller. `recvfrom` in it stops each call.
    pub fn of(numbers: &[libc::c_long]) -> Calls {
        Calls { numbers: numbers.to_vec(), recvfrom_with_source_only: false }
    }

    /// The numbers of the stopped calls.
    pub fn numbers(&self) -> &[libc::c_long] {
        &self.numbers
    }

    /// True when the filter stops the call `nr`.
    pub fn contains(&self, nr: libc::c_long) -> bool {
        self.numbers.contains(&nr)
    }

    /// The BPF program of the filter.
    pub fn program(&self) -> Vec<libc::sock_filter> {
        program(self)
    }
}

const BPF_LD_ABS_W: u16 = 0x20;
const BPF_JEQ_K: u16 = 0x15;
const BPF_JGE_K: u16 = 0x35;
const BPF_RET_K: u16 = 0x06;
/// `SECCOMP_RET_USER_NOTIF`.
pub const RET_USER_NOTIF: u32 = 0x7fc0_0000;
/// `SECCOMP_RET_ALLOW`.
pub const RET_ALLOW: u32 = 0x7fff_0000;
/// `SECCOMP_RET_ERRNO`.
pub const RET_ERRNO: u32 = 0x0005_0000;
// seccomp_data: nr at offset 0, arch at offset 4.
const OFFSET_ARCH: u32 = 4;
// seccomp_data: args[] (u64 each) at offset 16.
const OFFSET_ARG4: u32 = 16 + 4 * 8;

fn stmt(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter { code, jt: 0, jf: 0, k }
}

fn jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

fn program(calls: &Calls) -> Vec<libc::sock_filter> {
    // A system call from a different ABI (i386 on x86_64, for example) has
    // different numbers and would pass the list below. Refuse it.
    let mut filter = vec![
        stmt(BPF_LD_ABS_W, OFFSET_ARCH),
        jump(BPF_JEQ_K, AUDIT_ARCH_NATIVE, 1, 0),
        stmt(BPF_RET_K, RET_ERRNO | libc::ENOSYS as u32),
        stmt(BPF_LD_ABS_W, 0),
    ];
    if X32_SYSCALL_BIT != 0 {
        // x32 system calls use the native arch value with this bit set.
        filter.push(jump(BPF_JGE_K, X32_SYSCALL_BIT, 0, 1));
        filter.push(stmt(BPF_RET_K, RET_ERRNO | libc::ENOSYS as u32));
    }
    // socketcall(2) multiplexes the socket calls on i386 and powerpc64. The
    // list names the direct calls only, so socketcall fails: a program then
    // cannot pass around the hold (the C libraries use the direct calls on
    // the kernels that have them).
    #[cfg(any(target_arch = "x86", target_arch = "powerpc64"))]
    {
        filter.push(jump(BPF_JEQ_K, libc::SYS_socketcall as u32, 0, 1));
        filter.push(stmt(BPF_RET_K, RET_ERRNO | libc::ENOSYS as u32));
    }
    for &syscall in &calls.numbers {
        if syscall == libc::SYS_recvfrom && calls.recvfrom_with_source_only {
            // A recvfrom with no source buffer (recv(2), the reads of a TCP
            // program) needs no source address: the kernel runs it, as a
            // read(2). The pointer is args[4]; the targets are
            // little-endian, so its low word is first.
            filter.push(jump(BPF_JEQ_K, syscall as u32, 0, 6));
            filter.push(stmt(BPF_LD_ABS_W, OFFSET_ARG4));
            filter.push(jump(BPF_JEQ_K, 0, 0, 3));
            filter.push(stmt(BPF_LD_ABS_W, OFFSET_ARG4 + 4));
            filter.push(jump(BPF_JEQ_K, 0, 0, 1));
            filter.push(stmt(BPF_RET_K, RET_ALLOW));
            filter.push(stmt(BPF_RET_K, RET_USER_NOTIF));
            continue;
        }
        filter.push(jump(BPF_JEQ_K, syscall as u32, 0, 1));
        filter.push(stmt(BPF_RET_K, RET_USER_NOTIF));
    }
    filter.push(stmt(BPF_RET_K, RET_ALLOW));
    filter
}

/// Set `no_new_privs` on this thread and install the filter of `calls` on
/// it. Return the notification descriptor (the kernel makes it
/// close-on-exec). An unprivileged task can install a filter only with
/// `no_new_privs` set; a child and an `execve` keep the flag.
pub fn install(calls: &Calls) -> Result<OwnedFd, Failure> {
    // SAFETY: prctl with integer arguments.
    if let Err(e) = unsafe { sys::sys(sys::SYS_PRCTL, [sys::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0, 0]) } {
        return Err(failure::errno(Class::Unsupported, "prctl(PR_SET_NO_NEW_PRIVS)", e.0));
    }
    let filter = calls.program();
    let prog = libc::sock_fprog { len: filter.len() as u16, filter: filter.as_ptr() as *mut _ };
    // SAFETY: `prog` points to `filter`, which lives until the call returns.
    let r = unsafe {
        sys::sys(
            sys::SYS_SECCOMP,
            [
                sys::SECCOMP_SET_MODE_FILTER,
                sys::SECCOMP_FILTER_FLAG_NEW_LISTENER,
                &prog as *const libc::sock_fprog as u64,
                0,
                0,
                0,
            ],
        )
    };
    match r {
        // SAFETY: a new descriptor that nothing else owns.
        Ok(fd) => Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) }),
        Err(e) => Err(failure::errno(Class::Unsupported, "seccomp(SECCOMP_FILTER_FLAG_NEW_LISTENER)", e.0)),
    }
}

#[cfg(test)]
mod tests;
