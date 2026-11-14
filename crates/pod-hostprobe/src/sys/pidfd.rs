//! A signal addressed by a pidfd. A process that is not the reaper of its
//! target cannot signal it by pid safely: the reaper can reap it and the
//! kernel can give the pid to another process between the decision and the
//! `kill`.

use super::{Sysres, sys};

pub const SYS_PIDFD_SEND_SIGNAL: i64 = nr!(pidfd_send_signal, __NR_pidfd_send_signal);

/// Send `sig` to the process of `pidfd`. A process that was reaped answers
/// `ESRCH`: a later process with the same pid never gets the signal.
pub fn pidfd_send_signal(pidfd: i64, sig: i64) -> Sysres {
    unsafe { sys(SYS_PIDFD_SEND_SIGNAL, [pidfd as u64, sig as u64, 0, 0, 0, 0]) }
}
