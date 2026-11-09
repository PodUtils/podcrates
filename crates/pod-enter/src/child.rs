//! The waits on a started payload.

use std::io::Write;

use pod_core::Class;
use pod_hostprobe::sys;

use crate::status::{exit_status, signal_name};
use crate::{Child, Result, errno_failure};

/// How a bounded wait ended. There are three outcomes, not two: a bound
/// that the wait reached is not a failure and not an exit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bounded {
    /// docker's exit code for the end of the payload, as [`Child::wait`]
    /// gives it.
    Exited(i32),
    /// The wait reached its bound and killed the payload. It carries the
    /// bound and no code: the payload did not exit by itself.
    TimedOut { after_ms: i64 },
}

impl Child {
    /// Reap the payload, and give docker's exit code for its end.
    pub fn wait(&self) -> Result<i32> {
        let mut status = 0i32;
        loop {
            match sys::wait4(self.pid, &mut status) {
                Ok(_) => break,
                // A signal to the waiter is not the end of the payload.
                Err(e) if e == sys::EINTR => continue,
                Err(e) => return Err(errno_failure("wait4 on the payload failed", e)),
            }
        }
        Ok(exit_status(status))
    }

    /// Reap the payload, and forward SIGINT and SIGTERM of this process to
    /// it first.
    ///
    /// A waiter in the foreground holds the terminal and the payload holds
    /// the work: a Ctrl-C that kills only the waiter leaves the payload
    /// with no parent. The two signals are blocked and read from a
    /// signalfd beside the pidfd of the payload in one `ppoll`. So a
    /// signal cannot kill this process between the read and the forward,
    /// and a PID that the kernel used again never gets the signal. Each
    /// forward and an end by a signal are named on `err`; a clean exit
    /// prints nothing.
    ///
    /// The wait has no thread, and must not get one: a thread would make
    /// the next fork of this process the fork of a process with threads.
    ///
    /// A payload that ignores the signal is waited on, not killed: a kill
    /// is the decision of the caller. SIGINT is forwarded, never raised
    /// again, so the waiter stays alive to reap the payload and gives the
    /// code of the payload. A payload that dies on the default action
    /// still ends 130.
    pub fn wait_forwarding(&self, err: &mut dyn Write) -> Result<i32> {
        let mut old = 0u64;
        if let Err(e) = sys::sigblock_shutdown(&mut old) {
            return self.wait_unforwarded(err, &format!("the shutdown mask refused: {} ({})", e.name(), e.0));
        }
        let sfd = match sys::signalfd_shutdown() {
            Ok(f) => f,
            Err(e) => {
                let _ = sys::sigrestore(old);
                return self.wait_unforwarded(err, &format!("signalfd refused: {} ({})", e.name(), e.0));
            }
        };
        let pidfd = match sys::pidfd_open(self.pid) {
            Ok(f) => f,
            Err(e) => {
                let _ = sys::close(sfd);
                let _ = sys::sigrestore(old);
                return self.wait_unforwarded(err, &format!("pidfd_open refused: {} ({})", e.name(), e.0));
            }
        };
        let code = self.forward_loop(pidfd, sfd, err);
        let _ = sys::close(pidfd);
        let _ = sys::close(sfd);
        let _ = sys::sigrestore(old);
        code
    }

    /// The fallback: the forward could not start, so the wait reaps the
    /// payload with no forward, and says so.
    fn wait_unforwarded(&self, err: &mut dyn Write, why: &str) -> Result<i32> {
        let _ = writeln!(
            err,
            "waiting without shutdown forwarding ({why}); a SIGINT or SIGTERM \
             to this process will not reach the payload"
        );
        self.wait()
    }

    /// One `ppoll` over the exit of the payload and the shutdown signals of
    /// this process, done again at the bound: the bound only means that
    /// nothing happened.
    fn forward_loop(&self, pidfd: i64, sfd: i64, err: &mut dyn Write) -> Result<i32> {
        loop {
            // A new set for each poll: an old `revents` would reap or
            // forward two times.
            let mut fds = [
                sys::PollFd { fd: pidfd as i32, events: sys::POLLIN, revents: 0 },
                sys::PollFd { fd: sfd as i32, events: sys::POLLIN, revents: 0 },
            ];
            match sys::ppoll(&mut fds, 60_000) {
                Ok(_) => {}
                Err(e) if e == sys::EINTR => continue,
                Err(_) => return self.wait(),
            }
            if fds[1].revents & (sys::POLLIN | sys::POLLHUP) != 0 {
                let mut buf = [0u8; 128];
                // `ssi_signo` is the first word. A short read is not a
                // signal, and only SIGINT and SIGTERM can arrive: the mask
                // holds those two bits only.
                if let Ok(n) = sys::read(sfd, &mut buf)
                    && n >= 4
                {
                    let signo = i64::from(u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]));
                    if signo == sys::SIGINT as i64 || signo == sys::SIGTERM as i64 {
                        // ESRCH is not a failure: the payload exited
                        // between the read and the kill, and the pidfd
                        // half reaps it below.
                        let _ = sys::kill(self.pid, signo);
                        let _ = writeln!(err, "forwarded {} to the payload (pid {})", signal_name(signo), self.pid);
                    }
                }
            }
            if fds[0].revents & (sys::POLLIN | sys::POLLHUP) != 0 {
                match sys::waitid_pidfd(pidfd, false) {
                    Ok(Some(x)) if x.code == sys::CLD_EXITED => return Ok(x.status),
                    Ok(Some(x)) => {
                        let code = 128 + x.status;
                        let _ = writeln!(err, "the payload died on {}: exit {code}", signal_name(i64::from(x.status)));
                        return Ok(code);
                    }
                    // A readiness with no status: poll again.
                    Ok(None) => {}
                    Err(_) => return self.wait(),
                }
            }
        }
    }

    /// Reap the payload, but never wait longer than `ms`.
    ///
    /// `ppoll` on a pidfd: a pidfd names one process, and `ppoll` waits on
    /// the condition with a bound. There is no interval to guess and no
    /// window in which a PID that the kernel used again gets the signal.
    ///
    /// At the bound the payload gets SIGKILL and is then reaped: a payload
    /// that the caller left would keep the root file system busy and hold
    /// the image lock.
    pub fn wait_bounded(&self, ms: i64) -> Result<Bounded> {
        let pidfd = sys::pidfd_open(self.pid).map_err(|e| {
            errno_failure(
                &format!("pidfd_open on pid {} failed, and the entry does not wait on a bare pid", self.pid),
                e,
            )
        })?;
        let mut fds = [sys::PollFd { fd: pidfd as i32, events: sys::POLLIN, revents: 0 }];
        let ready = loop {
            match sys::ppoll(&mut fds, ms) {
                Ok(n) => break n,
                // A signal to the waiter is not the bound.
                Err(e) if e == sys::EINTR => continue,
                Err(e) => {
                    let _ = sys::close(pidfd);
                    return Err(errno_failure("ppoll on the pidfd of the payload failed", e));
                }
            }
        };
        if ready == 0 {
            let _ = sys::kill(self.pid, 9);
        }
        let ended = sys::waitid_pidfd(pidfd, false);
        let _ = sys::close(pidfd);
        if ready == 0 {
            return Ok(Bounded::TimedOut { after_ms: ms });
        }
        match ended {
            Ok(Some(e)) if e.code == sys::CLD_EXITED => Ok(Bounded::Exited(e.status)),
            // 128 + the signal, the rule of `exit_status` and of docker.
            Ok(Some(e)) => Ok(Bounded::Exited(128 + e.status)),
            Ok(None) => {
                Err(crate::Error::runtime(Class::Unavailable, "waitid gave no status for a pidfd that was ready"))
            }
            Err(e) => Err(errno_failure("waitid on the payload failed", e)),
        }
    }
}
