//! The PID namespace of the namespace rung ([`Plan::pid_namespace`],
//! PodUtils/podvm#457).
//!
//! The entry clones its child with `CLONE_NEWPID`, and with
//! `CLONE_NEWUSER` where the rung makes a user namespace (`ns_user`): the
//! kernel makes the user namespace first, so it owns the PID namespace, and
//! the child mounts a fresh `proc` of it on the `/proc` of the image. The
//! bind of the host's `/proc` is never the fallback: that proc is not the
//! proc of the PID namespace of the child (`ns_proc_bind`).
//!
//! The child is pid 1 of the namespace and stays as its init. Just before
//! the exec it forks the payload (pid 2), gives each signal of [`FORWARDED`]
//! that a process sent to the payload (a signal of the terminal reached the
//! payload already), reaps each process that the namespace orphans, and exits
//! with the code of the payload. A process of the parent namespace can
//! send pid 1 only the signals that pid 1 handles, so the payload is not
//! pid 1: a SIGTERM of the caller reaches it. The init holds the mounts, the
//! root, the limits and the confinement of the payload. The [`crate::Child`]
//! of the caller is the host pid of the init: the end of the init ends each
//! process of the namespace.
//!
//! A join of a run in a PID namespace (`ns_join`) joins it for the children
//! of the child: the child forks the payload of the exec into it and stays
//! as its relay, with the same loop.

use pod_core::Class;
use pod_hostprobe::sys::{self, Errno};

use crate::entry::NsEntry;
use crate::{Error, Plan, Result};

/// The step bytes of the clone with a PID namespace (in the parent: the
/// clone fails before a child exists), and of the fork of the payload under
/// its init or relay.
pub const NS_PID_STEP: u8 = 41;
pub const NS_PID_USER_STEP: u8 = 42;
pub const NS_PID_FORK_STEP: u8 = 45;

/// The signals that a process sends to tell or to stop the payload, as
/// docker's `--sig-proxy` with no tty: each signal that a process can catch,
/// except SIGCHLD, SIGPIPE, SIGURG, SIGWINCH and the signals of job control.
/// The waiter of [`crate::Child::wait_forwarding`] and the init give each one
/// to the payload. One list for both: a signal that one of them does not
/// read ends that process with its default action, and the payload stays.
pub const PROXIED: u64 = sys::sigbit(sys::SIGHUP)
    | sys::sigbit(sys::SIGINT)
    | sys::sigbit(sys::SIGQUIT)
    | sys::sigbit(sys::SIGUSR1)
    | sys::sigbit(sys::SIGUSR2)
    | sys::sigbit(sys::SIGTERM)
    | sys::sigbit(sys::SIGALRM)
    | sys::sigbit(sys::SIGPWR);

/// The signals that the init or the relay gives to the payload: [`PROXIED`]
/// and SIGWINCH. A resize that a process sends to the init reaches the
/// payload; the resize of a terminal reached it already.
pub const FORWARDED: u64 = PROXIED | sys::sigbit(sys::SIGWINCH);

/// [`FORWARDED`] and SIGCHLD: the init blocks them and reads them from a
/// signalfd. A blocked signal is never discarded, also for pid 1.
const MASK: u64 = FORWARDED | sys::sigbit(sys::SIGCHLD);

/// The exit code of an init that lost its payload, which cannot happen.
const LOST: i32 = 125;

/// The name of a step byte of this module, for [`crate::ns_setup_step`].
pub(crate) fn step_name(byte: u8) -> Option<&'static str> {
    match byte {
        NS_PID_STEP => Some("clone(CLONE_NEWPID)"),
        NS_PID_USER_STEP => Some("clone(CLONE_NEWUSER | CLONE_NEWPID)"),
        NS_PID_FORK_STEP => Some("forking the payload under the init of the PID namespace"),
        _ => crate::ns_join::pid_step_name(byte),
    }
}

/// The PID namespace of one fork, decided before it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pid {
    /// The namespace flags of the clone of the child.
    pub(crate) flags: u64,
    /// The child is pid 1 of a PID namespace of its own.
    pub(crate) own: bool,
    /// The child forks the payload and stays as its init or relay.
    pub(crate) relay: bool,
    /// A pidfd of the parent, for the check after `PR_SET_PDEATHSIG`:
    /// `getppid` gives 0 in a PID namespace of its own.
    pub(crate) parent: Option<i64>,
}

impl Pid {
    /// The PID namespace of a fork of `plan` on the entry `ns`. A plan
    /// with a PID namespace on an entry with no mount namespace of its own
    /// is refused: `entry` names that entry. A join follows the run, whatever
    /// the plan.
    pub(crate) fn of(plan: &Plan, ns: Option<&NsEntry<'_>>, entry: Option<&str>) -> Result<Pid> {
        match ns {
            Some(NsEntry::Join(j, _)) => Ok(Pid { relay: j.pid, ..Pid::default() }),
            Some(NsEntry::Own(m)) if plan.pid_namespace => {
                let user = if m.user.is_some() { sys::CLONE_NEWUSER } else { 0 };
                Ok(Pid { flags: sys::CLONE_NEWPID | user, own: true, relay: true, parent: None })
            }
            _ if plan.pid_namespace => Err(refused(entry.unwrap_or("this entry"))),
            _ => Ok(Pid::default()),
        }
    }

    /// With a pidfd of this process where the child is pid 1 and must end
    /// with its parent: the last step before the clone, so that no early
    /// return leaves the descriptor open.
    pub(crate) fn with_parent(self, die_with_parent: bool) -> Result<Pid> {
        if !(self.own && die_with_parent) {
            return Ok(self);
        }
        let fd = sys::pidfd_open(sys::getpid())
            .map_err(|e| crate::errno_failure("pidfd_open of the entry for its PID namespace", e))?;
        Ok(Pid { parent: Some(fd), ..self })
    }

    /// The step byte of a refused clone.
    pub(crate) fn clone_step(&self) -> u8 {
        if self.flags & sys::CLONE_NEWUSER != 0 { NS_PID_USER_STEP } else { NS_PID_STEP }
    }

    /// Close the pidfd of the parent, in the parent after the fork.
    pub(crate) fn close(&self) {
        if let Some(fd) = self.parent {
            let _ = sys::close(fd);
        }
    }

    /// In the child, after `PR_SET_PDEATHSIG`: did the parent `parent` end
    /// before it? Async-signal-safe.
    pub(crate) fn parent_ended(&self, parent: i64) -> bool {
        let Some(fd) = self.parent else { return sys::getppid() != parent };
        let mut fds = [sys::PollFd { fd: fd as i32, events: sys::POLLIN, revents: 0 }];
        let ended = !matches!(sys::ppoll(&mut fds, 0), Ok(0));
        let _ = sys::close(fd);
        ended
    }

    /// In the child, just before the exec: fork the payload and stay as its
    /// init or relay. Returns in the payload, or where the fork has no
    /// place. Async-signal-safe.
    ///
    /// The init and the relay never exec, so they would keep each
    /// descriptor of the caller, also each one with close-on-exec: a pipe
    /// whose end of file a caller waits for (the ready pipe of
    /// `pod-supervise`, PodUtils/podvm#460), a lock, a socket. They close
    /// each one, the standard ones too: they read and write none.
    pub(crate) fn split(&self) -> std::result::Result<(), (u8, Errno)> {
        if !self.relay {
            return Ok(());
        }
        match unsafe { sys::clone_fork(sys::SIGCHLD) } {
            Ok(0) => {
                // The payload ends with its relay: a relay of a join is
                // not pid 1, and a signal before its mask can end it.
                let _ = sys::set_pdeathsig(9);
                Ok(())
            }
            Ok(payload) => {
                let _ = sys::close_from(0);
                relay(payload)
            }
            Err(e) => Err((NS_PID_FORK_STEP, e)),
        }
    }
}

/// The refusal of a plan with a PID namespace on `entry`.
pub(crate) fn refused(entry: &str) -> Error {
    Error::runtime(Class::Unsupported, format!("the PID namespace needs the namespace rung; {entry} has none"))
}

/// `si_code` of a signal of the kernel: a terminal sends SIGINT, SIGQUIT
/// and SIGWINCH so to each process of its foreground group.
const SI_KERNEL: i32 = 0x80;

/// Does the init give the signal `signo` of the code `code` to the
/// payload? Not SIGCHLD, and not a signal of the terminal: the payload is
/// in the same foreground group and got it already.
pub(crate) fn forwards(signo: u32, code: i32) -> bool {
    u64::from(signo) != sys::SIGCHLD && code != SI_KERNEL
}

/// The loop of the init or the relay of `payload`: give each signal of
/// [`FORWARDED`] to it, reap each child, and exit with the code of the
/// payload. With no signalfd it reaps with no forward. Async-signal-safe.
fn relay(payload: i64) -> ! {
    let mut old = 0u64;
    let mut status = 0i32;
    if let Ok(fd) = sys::sigblock(MASK, &mut old).and_then(|_| sys::signalfd(MASK)) {
        loop {
            // Each ended child first: a SIGCHLD before the mask is gone.
            loop {
                match sys::reap_any(&mut status) {
                    Ok(Some(p)) if p == payload => sys::exit_group(crate::exit_status(status)),
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(e) if e == sys::EINTR => {}
                    Err(_) => sys::exit_group(LOST),
                }
            }
            let mut buf = [0u8; 128];
            match sys::read(fd, &mut buf) {
                Ok(n) if n >= 12 => {
                    // `ssi_signo` and `ssi_code` of `signalfd_siginfo`.
                    let signo = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
                    let code = i32::from_ne_bytes([buf[8], buf[9], buf[10], buf[11]]);
                    if forwards(signo, code) {
                        let _ = sys::kill(payload, i64::from(signo));
                    }
                }
                Err(e) if e != sys::EINTR => break,
                _ => {}
            }
        }
    }
    loop {
        match sys::wait4(-1, &mut status) {
            Ok(p) if p == payload => sys::exit_group(crate::exit_status(status)),
            Ok(_) => {}
            Err(e) if e == sys::EINTR => {}
            Err(_) => sys::exit_group(LOST),
        }
    }
}

#[cfg(test)]
mod tests;
