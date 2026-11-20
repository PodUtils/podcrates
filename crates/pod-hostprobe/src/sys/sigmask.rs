//! The signal mask of the calling thread, for any set of signals.
//! `rt_sigprocmask` changes the mask of one thread, and a new thread takes
//! the mask of the thread that made it.
//!
//! The kernel's mask is not one `u64` on each architecture: mips has 128
//! signals and a mask of 16 bytes, and a 32-bit target keeps the mask as
//! words of 32 bits. `rt_sigprocmask` and `signalfd4` refuse each size but
//! the kernel's with `EINVAL`. The API here keeps signals 1 to 64 in a `u64`
//! (bit `sig - 1`), and each call gives the kernel its own
//! `kernel_sigset_t`, word by word.

use linux_raw_sys::ctypes::c_ulong;
use linux_raw_sys::general::{self as g, kernel_sigset_t};

use super::{
    SFD_CLOEXEC, SIGALRM, SIGINT, SIGPWR, SIGTERM, SIGUSR1, SIGUSR2, SYS_RT_SIGPROCMASK, SYS_SIGNALFD4, Sysres, sys,
};

/// The terminal's hangup and quit signals, beside `SIGINT` and `SIGTERM`.
/// The four numbers are the same on each Linux architecture.
pub const SIGHUP: u64 = g::SIGHUP as u64;
pub const SIGQUIT: u64 = g::SIGQUIT as u64;

/// `rt_sigprocmask` "block these" and "set to this". mips and sparc give
/// 1 and 3, the other architectures 0 and 2.
pub const SIG_BLOCK: u64 = g::SIG_BLOCK as u64;
pub const SIG_SETMASK: u64 = g::SIG_SETMASK as u64;

/// The size of the kernel's signal mask in bytes: 8, and 16 on mips.
pub const SIGSET_BYTES: usize = core::mem::size_of::<kernel_sigset_t>();

/// The bit of signal number `sig` in a mask word: bit `sig - 1`.
pub const fn sigbit(sig: u64) -> u64 {
    1 << (sig - 1)
}

/// The mask of the kernel for the signals of `set`: the low word holds
/// signals 1 to 32 or 1 to 64, in the byte order of the target.
// `as c_ulong` is a cast to `u64` on a 64-bit target and a cut to the word
// on a 32-bit one.
#[allow(clippy::unnecessary_cast)]
pub fn to_kernel(set: u64) -> kernel_sigset_t {
    let mut k = kernel_sigset_t { sig: Default::default() };
    for (i, w) in k.sig.iter_mut().enumerate() {
        let shift = i as u32 * c_ulong::BITS;
        *w = set.checked_shr(shift).unwrap_or(0) as c_ulong;
    }
    k
}

/// Signals 1 to 64 of a mask of the kernel.
#[allow(clippy::unnecessary_cast)]
pub fn from_kernel(k: &kernel_sigset_t) -> u64 {
    let mut set = 0u64;
    for (i, w) in k.sig.iter().enumerate() {
        let shift = i as u32 * c_ulong::BITS;
        set |= (*w as u64).checked_shl(shift).unwrap_or(0);
    }
    set
}

/// `rt_sigprocmask(how, set, old)` with masks of the kernel's size.
fn sigprocmask(how: u64, set: u64, old: Option<&mut u64>) -> Sysres {
    let new = to_kernel(set);
    let mut was = to_kernel(0);
    let old_ptr = if old.is_some() { &mut was as *mut kernel_sigset_t as u64 } else { 0 };
    let r = unsafe {
        sys(SYS_RT_SIGPROCMASK, [how, &new as *const kernel_sigset_t as u64, old_ptr, SIGSET_BYTES as u64, 0, 0])
    }?;
    if let Some(old) = old {
        *old = from_kernel(&was);
    }
    Ok(r)
}

/// Block each signal of `set` in the calling thread, and report the earlier
/// mask in `old` for [`sigrestore`].
pub fn sigblock(set: u64, old: &mut u64) -> Sysres {
    sigprocmask(SIG_BLOCK, set, Some(old))
}

/// Set the mask of the calling thread to `old`, a mask that [`sigblock`] or
/// [`super::sigblock_shutdown`] reported.
pub fn sigrestore(old: u64) -> Sysres {
    sigprocmask(SIG_SETMASK, old, None)
}

/// A `signalfd4` with `CLOEXEC` that reads the signals of `set`. The mask
/// goes by address, in the kernel's size.
pub fn signalfd(set: u64) -> Sysres {
    let mask = to_kernel(set);
    unsafe {
        sys(
            SYS_SIGNALFD4,
            [-1i64 as u64, &mask as *const kernel_sigset_t as u64, SIGSET_BYTES as u64, SFD_CLOEXEC, 0, 0],
        )
    }
}

/// The signals that a process sends to tell or to stop a payload, as
/// docker's `--sig-proxy` with no tty: SIGHUP, SIGINT, SIGQUIT, SIGUSR1,
/// SIGUSR2, SIGTERM, SIGALRM and SIGPWR. That is each signal that a process
/// can catch, except SIGCHLD, SIGPIPE, SIGURG, SIGWINCH and the signals of
/// job control. Each relay of one payload (the waiter and the init of
/// `pod-enter`, the launcher and the hold of `pod-supervise`) blocks and
/// gives this one list: a signal that a relay does not block ends the relay
/// with its default action, and the payload stays.
pub const PROXIED_SIGNALS: u64 = sigbit(SIGHUP)
    | sigbit(SIGINT)
    | sigbit(SIGQUIT)
    | sigbit(SIGUSR1)
    | sigbit(SIGUSR2)
    | sigbit(SIGTERM)
    | sigbit(SIGALRM)
    | sigbit(SIGPWR);

/// Does a relay give the signal `signo` with the code `code` (`ssi_code`
/// of its signalfd) to its payload? Each signal of [`PROXIED_SIGNALS`], but
/// SIGINT and SIGQUIT only from a process (`code` 0 or less): a terminal
/// sends them to the process group of the payload too.
pub fn proxied_forwards(signo: u32, code: i32) -> bool {
    let signo = u64::from(signo);
    if !(1..=64).contains(&signo) || PROXIED_SIGNALS & sigbit(signo) == 0 {
        return false;
    }
    code <= 0 || (signo != SIGINT && signo != SIGQUIT)
}

/// Block [`PROXIED_SIGNALS`] in the calling thread, and report the earlier
/// mask in `old` for [`sigrestore`]. A process that relays the list blocks
/// it in each thread: the kernel gives a signal for the process to any
/// thread that does not block it. A new thread takes the mask of its maker.
pub fn sigblock_proxied(old: &mut u64) -> Sysres {
    sigblock(PROXIED_SIGNALS, old)
}

/// A `signalfd4` with `CLOEXEC` that reads [`PROXIED_SIGNALS`].
pub fn signalfd_proxied() -> Sysres {
    signalfd(PROXIED_SIGNALS)
}
