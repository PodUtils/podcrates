//! The detached supervisor: one process per running container.
//!
//! [`TODO/supervise.md`](../../../TODO/supervise.md) T-0601, T-0602, T-0603,
//! T-0605.
//!
//! ⭐ **One launcher per container, and it is the only thing that knows the
//! container is running.** It holds the payload's pidfd, holds the container's
//! lock for its whole life, owns the control socket, and is what writes
//! `exited` into the table. Nothing else infers any of that.
//!
//! ⛔ **Nothing here sleeps.** T-0602: a fixed sleep is a scheduling assumption,
//! and the prior art's own capture of this lifecycle contains a failed run for
//! exactly that reason. Readiness is an `O_CLOEXEC` pipe that the `execve`
//! closes; the wait for the payload to end is `ppoll` on its pidfd; the wait for
//! a control message is the same `ppoll` on the listener. Every one of them has
//! an upper bound and a distinct outcome for reaching it.
//!
//! ⛔ **The log is opened BEFORE the root changes.** T-0605: a sink opened after
//! the chroot cannot reach the store, which is outside the new root, and that is
//! step 2 of `TOOL.md` section 6.5 made in a different place.
//!
//! ⚠ **A pidfd addresses ONE process.** It does not contain descendants and it
//! is not a PID namespace: a grandchild that reparents is outside podvm's
//! reach, `inspect` says so, and nothing here implies otherwise.

use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};

use podvm_enter::{Fds, Plan, RootDir};
use podvm_image::store::Lock;
use podvm_image::Store;
use podvm_probe::sys;

use crate::table::{self, State};
use crate::{Error, Result};

/// The control protocol, one line each way. ⚠ Deliberately tiny: it exists so
/// `stop`, `kill` and `wait` address the LAUNCHER rather than a pid, which a
/// second process cannot do safely because a pid is reused.
pub const CTL_SIGNAL: &str = "SIG";
pub const CTL_WAIT: &str = "WAIT";

/// Ask a running container's launcher to signal its payload.
pub fn signal(store: &Store, id: &str, sig: i32) -> Result<()> {
    let mut s = connect(store, id)?;
    writeln!(s, "{CTL_SIGNAL} {sig}").map_err(|e| Error(format!("the launcher: {e}")))?;
    let mut line = String::new();
    BufReader::new(&s)
        .read_line(&mut line)
        .map_err(|e| Error(format!("the launcher: {e}")))?;
    if line.trim() == "ok" {
        Ok(())
    } else {
        Err(Error(format!("the launcher answered {:?}", line.trim())))
    }
}

/// Why a wait on a launcher failed.
///
/// ⛔ T-1691 (issue 215): only `Gone` means the container already ended. A
/// failure of the wait itself is passed on, never read as a stop.
#[derive(Debug)]
pub enum WaitError {
    /// No launcher answers: it ended and closed or removed its socket.
    Gone(Error),
    /// A launcher listens, and the wait failed for another reason.
    Failed(Error),
}

/// Wait for a running container to end, bounded.
///
/// ⛔ T-0602 and `TODO/RULES.md` section 8: `Ok(None)` is the bound being
/// reached and it is a distinct outcome, never a guess at the exit code.
/// A zero bound waits for nothing and answers `Ok(None)` at once: `std`
/// refuses a zero read timeout (T-1691).
pub fn wait(
    store: &Store,
    id: &str,
    timeout_ms: u64,
) -> std::result::Result<Option<i32>, WaitError> {
    let s = connect(store, id).map_err(WaitError::Gone)?;
    if timeout_ms == 0 {
        return Ok(None);
    }
    s.set_read_timeout(Some(std::time::Duration::from_millis(timeout_ms)))
        .map_err(|e| WaitError::Failed(Error(format!("bounding the wait: {e}"))))?;
    let mut w = &s;
    // ⚠ A launcher that closes the connection between the connect and the
    // write or the read has ended: that is `Gone`, as a failed connect is.
    writeln!(w, "{CTL_WAIT}").map_err(|e| WaitError::Gone(Error(format!("the launcher: {e}"))))?;
    let mut line = String::new();
    match BufReader::new(&s).read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => match line.trim().strip_prefix("exit ") {
            Some(n) => n.parse::<i32>().map(Some).map_err(|_| {
                WaitError::Failed(Error(format!("the launcher answered {:?}", line.trim())))
            }),
            None => Ok(None),
        },
        Err(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
        {
            Ok(None)
        }
        Err(e) => Err(WaitError::Gone(Error(format!("the launcher: {e}")))),
    }
}

fn connect(store: &Store, id: &str) -> Result<UnixStream> {
    // ⚠ Through `/proc/self/fd`, for the same reason the bind is: the real path
    // does not fit in `sun_path`. The directory handle is held for the length of
    // the connect and dropped after it.
    let (_dir, path) = table::control_via_fd(store, id)?;
    UnixStream::connect(&path).map_err(|e| {
        Error(format!(
            "no launcher is listening for {id}: {e}. The container is not running"
        ))
    })
}

/// Fork a detached launcher for `container`, and return once its payload has
/// reached its `execve` or failed to.
///
/// ⛔ **The readiness answer comes back over a pipe, not from a timer.** This
/// function returns `Ok(pid)` only after the launcher has said the payload is
/// running, so a caller never has to decide whether a container it cannot see
/// yet is starting or already dead.
pub fn start(
    store: &Store,
    container: &table::Container,
    record: &podvm_image::Record,
) -> Result<(i64, i64)> {
    let dir = table::dir(store, &container.id);
    std::fs::create_dir_all(&dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;

    // ⭐ T-0605. The log sink is opened HERE, before anything changes root, and
    // its descriptors are handed to the payload. A sink opened after the chroot
    // cannot reach the store.
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(table::log_path(store, &container.id))
        .map_err(|e| Error(format!("opening the container log: {e}")))?;

    // The readiness channel from the LAUNCHER back to this process. Separate
    // from the one `podvm_enter::spawn` uses between the launcher and the
    // payload: this one carries the launcher's own verdict.
    // ⛔ `O_CLOEXEC`, and it is not tidiness. Measured on 2026-09-09: without it
    // the PAYLOAD inherits the write end through its `execve` and holds it for
    // its whole life, so the caller's read never sees EOF and `podvm run -d`
    // blocks for exactly as long as the container runs. A fork keeps the fd,
    // which is what the launcher needs; an exec drops it, which is what the
    // payload must do.
    let mut ready = [0i32; 2];
    sys::pipe2(&mut ready, sys::O_CLOEXEC)
        .map_err(|e| Error(format!("pipe2: {} ({})", e.name(), e.0)))?;
    let (ready_r, ready_w) = (ready[0] as i64, ready[1] as i64);

    // ⛔ SINGLE-THREADED ON THIS PATH. T-0603: `PR_SET_PDEATHSIG` fires on the
    // creating THREAD's exit, and podvm keeps the path that clones, chroots and
    // execs free of threads so the creating thread is the process. Nothing here
    // spawns one, and `the_spawn_path_is_single_threaded` asserts it.
    let pid = match unsafe { sys::clone_fork(sys::SIGCHLD) } {
        Ok(0) => {
            // ⛔ A DOUBLE FORK, and it is what makes `-d` detached. This
            // intermediate child forks the real supervisor and exits at once, so
            // the caller's `wait4` below returns immediately instead of blocking
            // for the container's whole life, and the supervisor is reparented
            // to init rather than left as a child of a process that has gone.
            let _ = sys::close(ready_r);
            match unsafe { sys::clone_fork(sys::SIGCHLD) } {
                Ok(0) => {
                    let code = supervise(store, container, record, log, ready_w);
                    sys::exit_group(code)
                }
                // ⚠ The intermediate keeps NOTHING open: its copy of the
                // readiness pipe closing is what would otherwise hold the
                // caller's read open forever.
                Ok(_) => sys::exit_group(0),
                Err(e) => {
                    let msg = format!("err clone(2) for the supervisor failed: {}", e.name());
                    let _ = sys::write(ready_w, msg.as_bytes());
                    sys::exit_group(1)
                }
            }
        }
        Ok(pid) => pid,
        Err(e) => {
            let _ = sys::close(ready_r);
            let _ = sys::close(ready_w);
            return Err(Error(format!(
                "clone(2) for the launcher failed: {} ({})",
                e.name(),
                e.0
            )));
        }
    };
    let _ = sys::close(ready_w);

    // ⚠ The launcher is reaped HERE and immediately: it forks its own child and
    // exits, so this wait is for the intermediate process and returns at once.
    // Leaving it unreaped would make every `podvm run -d` leave a zombie.
    let mut line = Vec::new();
    let mut buf = [0u8; 256];
    loop {
        match sys::read(ready_r, &mut buf) {
            Ok(0) => break,
            Ok(n) => line.extend_from_slice(&buf[..n as usize]),
            Err(e) if e == sys::EINTR => continue,
            Err(_) => break,
        }
    }
    let _ = sys::close(ready_r);
    let mut status = 0i32;
    let _ = sys::wait4(pid, &mut status);

    let text = String::from_utf8_lossy(&line).trim().to_string();
    match text.strip_prefix("ok ") {
        Some(rest) => {
            let mut parts = rest.split_whitespace();
            let launcher = parts.next().and_then(|v| v.parse::<i64>().ok());
            let payload = parts.next().and_then(|v| v.parse::<i64>().ok());
            match (launcher, payload) {
                (Some(l), Some(p)) => Ok((l, p)),
                _ => Err(Error(format!("the launcher said {text:?}"))),
            }
        }
        None if text.is_empty() => Err(Error(
            "the launcher exited without saying whether the payload started".into(),
        )),
        None => Err(Error(text.trim_start_matches("err ").to_string())),
    }
}

mod supervise;
use supervise::*;

/// Wait for the payload to end, answering control messages meanwhile.
///
/// ⛔ **One `ppoll` over both, and no interval anywhere.** T-0602. The bound is
/// long rather than absent, and reaching it is a re-poll rather than a verdict:
/// a container that runs for a week is not a failure, and a poll that returned
/// zero has learned nothing.
fn serve(listener: &UnixListener, pidfd: i64, child: &podvm_enter::Child) -> i32 {
    let mut waiters: Vec<UnixStream> = Vec::new();
    // ⭐ TODO/supervise.md T-1335: the launcher's own SIGINT/SIGTERM are
    // blocked and read from a signalfd beside the payload's pidfd, so a
    // signal to the launcher reaches the payload instead of orphaning it.
    // No naming here: this process's stdio is /dev/null (T-0608), so the
    // signaled exit code in the table record is the record. The mask stays
    // blocked through this process's exit; there is nothing left that waits
    // on those signals.
    let mut old = 0u64;
    let sfd = sys::sigblock_shutdown(&mut old)
        .and_then(|_| sys::signalfd_shutdown())
        .unwrap_or(-1);
    loop {
        let mut fds = [
            sys::PollFd {
                fd: pidfd as i32,
                events: sys::POLLIN,
                revents: 0,
            },
            sys::PollFd {
                fd: listener.as_raw_fd(),
                events: sys::POLLIN,
                revents: 0,
            },
            // ⚠ A negative fd is ignored by the kernel, which is what keeps
            // one shape here when the signalfd above refused: that launcher
            // waits unforwarded rather than not at all.
            sys::PollFd {
                fd: sfd as i32,
                events: if sfd >= 0 { sys::POLLIN } else { 0 },
                revents: 0,
            },
        ];
        // 60 s, and reaching it means nothing happened rather than anything
        // being wrong.
        match sys::ppoll(&mut fds, 60_000) {
            Ok(_) => {}
            Err(e) if e == sys::EINTR => continue,
            Err(_) => {
                // ⚠ The poll itself failed. Fall back to a blocking reap rather
                // than spinning: the payload is still this process's child.
                if sfd >= 0 {
                    let _ = sys::close(sfd);
                }
                return child.wait().unwrap_or(125);
            }
        }
        if sfd >= 0 && fds[2].revents & (sys::POLLIN | sys::POLLHUP) != 0 {
            let mut buf = [0u8; 128];
            if let Ok(n) = sys::read(sfd, &mut buf)
                && n >= 4
            {
                let signo = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as i64;
                if signo == sys::SIGINT as i64 || signo == sys::SIGTERM as i64 {
                    // The `kill`-and-not-`pidfd_send_signal` reasoning in
                    // `handle` applies unchanged: parent, unreaped.
                    let _ = sys::kill(child.pid, signo);
                }
            }
        }
        if fds[1].revents & (sys::POLLIN | sys::POLLHUP) != 0
            && let Ok((stream, _)) = listener.accept()
        {
            handle(stream, child, &mut waiters);
        }
        if fds[0].revents & (sys::POLLIN | sys::POLLHUP) != 0 {
            // ⭐ T-0601: the status comes through the pidfd, so it cannot be
            // redirected by pid reuse between the readiness and the reap.
            let code = match sys::waitid_pidfd(pidfd, false) {
                Ok(Some(x)) if x.code == sys::CLD_EXITED => x.status,
                Ok(Some(x)) => 128 + x.status,
                _ => child.wait().unwrap_or(125),
            };
            for mut w in waiters {
                let _ = writeln!(w, "exit {code}");
            }
            if sfd >= 0 {
                let _ = sys::close(sfd);
            }
            return code;
        }
    }
}

fn handle(stream: UnixStream, child: &podvm_enter::Child, waiters: &mut Vec<UnixStream>) {
    let mut line = String::new();
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut w = &stream;
    let text = line.trim();
    if let Some(n) = text.strip_prefix(CTL_SIGNAL) {
        let sig: i64 = n.trim().parse().unwrap_or(15);
        // ⚠ `kill` and not `pidfd_send_signal`, and it is safe HERE and only
        // here: this process is the payload's parent and has not reaped it, so
        // the pid still names it. A second process could not say that.
        let _ = sys::kill(child.pid, sig);
        let _ = writeln!(w, "ok");
        return;
    }
    if text.starts_with(CTL_WAIT) {
        // ⛔ Held, not answered. The waiter is told the exit code when there is
        // one, which is the only honest moment to say anything.
        waiters.push(stream);
        return;
    }
    let _ = writeln!(w, "err unknown request");
}

#[cfg(test)]
mod tests;
