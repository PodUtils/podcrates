use super::*;

/// How a bounded wait ended. ⛔ Three outcomes and not two: `TimedOut` is
/// `TODO/RULES.md` section 8's own rule, and a bound reached is neither a
/// failure nor an exit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bounded {
    /// docker's exit code for however it ended, as [`Child::wait`] reports it.
    Exited(i32),
    /// The bound was reached and the payload was killed. ⚠ It carries the
    /// bound rather than a code, because there is no code: nothing exited on
    /// its own.
    TimedOut { after_ms: i64 },
}

impl Child {
    /// Reap it, and return docker's exit code for however it ended.
    pub fn wait(&self) -> Result<i32> {
        let mut status = 0i32;
        loop {
            match sys::wait4(self.pid, &mut status) {
                Ok(_) => break,
                // ⚠ `EINTR` is not a failure. A signal delivered to podvm while
                // it waits must not be reported as the payload having died.
                Err(e) if e == sys::EINTR => continue,
                Err(e) => {
                    return Err(Error::Runtime(format!(
                        "wait4 on the payload failed: {} ({})",
                        e.name(),
                        e.0
                    )))
                }
            }
        }
        Ok(exit_status(status))
    }

    /// Reap it, forwarding this process's own SIGINT/SIGTERM to it first.
    ///
    /// TODO/supervise.md T-1335: a foreground waiter holds the terminal but
    /// the payload holds the work, so a Ctrl-C that kills only the waiter
    /// orphans the payload. The two signals are blocked and read from a
    /// signalfd beside the payload's pidfd in one `ppoll`, so the signal
    /// never kills this process between the decision and the forward, and a
    /// reused pid can never be signalled instead. Each forward and a
    /// signaled end are named on `err`; a clean exit prints nothing.
    ///
    /// ⚠ No threads, and none can be added here: the T-0603 assertion scans
    /// this crate, and a thread would make the clone below fork a threaded
    /// process. The wait is single-threaded by construction.
    ///
    /// ⚠ A payload that ignores the signal is waited on, not killed: killing
    /// it would be `stop`'s decision made inside `run`, and the caller that
    /// wants it dead already has `stop` and `kill`.
    ///
    /// ⚠ SIGINT is forwarded, never re-raised: the waiter stays alive to
    /// reap the payload, and the code it returns is the payload's own. A
    /// payload that dies on the default disposition still ends 130; one
    /// that traps or ignores SIGINT outlives it, and the waiter with it.
    pub fn wait_forwarding(&self, err: &mut dyn Write) -> Result<i32> {
        let mut old = 0u64;
        if let Err(e) = sys::sigblock_shutdown(&mut old) {
            return self.wait_unforwarded(
                err,
                &format!("the shutdown mask refused: {} ({})", e.name(), e.0),
            );
        }
        let sfd = match sys::signalfd_shutdown() {
            Ok(f) => f,
            Err(e) => {
                let _ = sys::sigrestore(old);
                return self
                    .wait_unforwarded(err, &format!("signalfd refused: {} ({})", e.name(), e.0));
            }
        };
        let pidfd = match sys::pidfd_open(self.pid) {
            Ok(f) => f,
            Err(e) => {
                let _ = sys::close(sfd);
                let _ = sys::sigrestore(old);
                return self
                    .wait_unforwarded(err, &format!("pidfd_open refused: {} ({})", e.name(), e.0));
            }
        };
        let code = self.forward_loop(pidfd, sfd, err);
        let _ = sys::close(pidfd);
        let _ = sys::close(sfd);
        let _ = sys::sigrestore(old);
        code
    }

    /// The old wait, named as the fallback: the forward could not be armed,
    /// so the payload is reaped with signals unforwarded rather than left.
    fn wait_unforwarded(&self, err: &mut dyn Write, why: &str) -> Result<i32> {
        let _ = writeln!(
            err,
            "waiting without shutdown forwarding ({why}); a SIGINT or SIGTERM \
             to this process will not reach the payload"
        );
        self.wait()
    }

    /// One `ppoll` over the payload's exit and this process's shutdown
    /// signals, re-polled on the bound: reaching it means nothing happened
    /// rather than anything being wrong (T-0602's shape, not a second one).
    fn forward_loop(&self, pidfd: i64, sfd: i64, err: &mut dyn Write) -> Result<i32> {
        loop {
            // ⛔ Fresh set every poll: `ppoll` leaves old `revents` standing,
            // and a stale one would reap or forward twice.
            let mut fds = [
                sys::PollFd {
                    fd: pidfd as i32,
                    events: sys::POLLIN,
                    revents: 0,
                },
                sys::PollFd {
                    fd: sfd as i32,
                    events: sys::POLLIN,
                    revents: 0,
                },
            ];
            match sys::ppoll(&mut fds, 60_000) {
                Ok(_) => {}
                Err(e) if e == sys::EINTR => continue,
                Err(_) => return self.wait(),
            }
            if fds[1].revents & (sys::POLLIN | sys::POLLHUP) != 0 {
                let mut buf = [0u8; 128];
                // ⛔ `ssi_signo` is the first word. A short read is not a
                // signal, and anything but SIGINT/SIGTERM cannot arrive: the
                // mask holds exactly those two bits.
                if let Ok(n) = sys::read(sfd, &mut buf)
                    && n >= 4
                {
                    let signo = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as i64;
                    if signo == sys::SIGINT as i64 || signo == sys::SIGTERM as i64 {
                        // ⚠ ESRCH is not a failure: the payload exited
                        // between the read and this kill, and the pidfd
                        // half reaps it below.
                        let _ = sys::kill(self.pid, signo);
                        let _ = writeln!(
                            err,
                            "forwarded {} to the payload (pid {})",
                            signal_name(signo),
                            self.pid
                        );
                    }
                }
            }
            if fds[0].revents & (sys::POLLIN | sys::POLLHUP) != 0 {
                match sys::waitid_pidfd(pidfd, false) {
                    Ok(Some(x)) if x.code == sys::CLD_EXITED => return Ok(x.status),
                    Ok(Some(x)) => {
                        let code = 128 + x.status;
                        let _ = writeln!(
                            err,
                            "the payload died on {}: exit {code}",
                            signal_name(x.status as i64)
                        );
                        return Ok(code);
                    }
                    // Spurious readiness: re-poll rather than verdict.
                    Ok(None) => {}
                    Err(_) => return self.wait(),
                }
            }
        }
    }

    /// Reap it, but never wait longer than `ms`.
    ///
    /// ⭐ **`ppoll` on a pidfd, which is `TODO/supervise.md` T-0601 and T-0602's
    /// mechanism rather than a second one.** A pid plus a sleep is a heuristic;
    /// a pidfd names one process and `ppoll` waits on the CONDITION with an
    /// upper bound, so there is no interval to be wrong about and no window in
    /// which a reused pid could be signalled instead.
    ///
    /// ⛔ **On the bound the child is `SIGKILL`ed and then reaped.** A step
    /// podvm started inside somebody else's image and then walked away from
    /// would keep the rootfs busy and hold the image lock, and `AGENTS.md`
    /// makes bounded waits a requirement of podvm itself and not only of the
    /// agent working on it.
    pub fn wait_bounded(&self, ms: i64) -> Result<Bounded> {
        let pidfd = sys::pidfd_open(self.pid).map_err(|e| {
            Error::Runtime(format!(
                "pidfd_open on pid {} failed: {} ({}); podvm will not wait on a \
                 bare pid",
                self.pid,
                e.name(),
                e.0
            ))
        })?;
        let mut fds = [sys::PollFd {
            fd: pidfd as i32,
            events: sys::POLLIN,
            revents: 0,
        }];
        let ready = loop {
            match sys::ppoll(&mut fds, ms) {
                Ok(n) => break n,
                // ⚠ `EINTR` is not the bound. A signal delivered to podvm while
                // it waits must not read as the step having hung.
                Err(e) if e == sys::EINTR => continue,
                Err(e) => {
                    let _ = sys::close(pidfd);
                    return Err(Error::Runtime(format!(
                        "ppoll on the step's pidfd failed: {} ({})",
                        e.name(),
                        e.0
                    )));
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
            // ⛔ 128+signal, which is `exit_status`'s own rule and docker's: a
            // killed step reporting 0 or 1 is the lie T-0802 is about.
            Ok(Some(e)) => Ok(Bounded::Exited(128 + e.status)),
            Ok(None) => Err(Error::Runtime(
                "waitid reported no status for a pidfd that was ready".into(),
            )),
            Err(e) => Err(Error::Runtime(format!(
                "waitid on the step failed: {} ({})",
                e.name(),
                e.0
            ))),
        }
    }
}
