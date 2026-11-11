//! The detached launcher: one process for each running container.
//!
//! One launcher for each container, and only it knows that the container
//! runs. It holds the pidfd of the payload, holds the lock of the container
//! for its whole life, owns the control socket, and writes `exited` into the
//! table.
//!
//! Nothing here sleeps: a fixed sleep is a guess about the scheduler.
//! Readiness is an `O_CLOEXEC` pipe that the `execve` closes; the wait for
//! the end of the payload is `ppoll` on its pidfd; the wait for a control
//! message is the same `ppoll` on the listener. Each has a bound and its own
//! result for the bound.
//!
//! The log opens before the root changes: a file opened after a `chroot`
//! cannot reach the state directory, which is outside the new root.
//!
//! A pidfd addresses one process. It does not hold the descendants and it
//! is not a PID namespace. The launcher is the child subreaper of the tree
//! instead ([`tree`]): a descendant whose parent ends comes to the
//! launcher, which reaps it while the payload runs and kills it at the end.

use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Instant;

use pod_core::{Class, Failure, Stage};
use pod_hostprobe::sys;
use serde::{Serialize, de::DeserializeOwned};

use crate::end::Reason;
use crate::table::{self, Container, State};
use crate::{Result, Settings, failure};

/// The control protocol: one line each way. It lets `stop`, `kill` and
/// `wait` address the launcher, not a pid, which a second process cannot do
/// safely because a pid is used again.
pub const CTL_SIGNAL: &str = "SIG";
pub const CTL_WAIT: &str = "WAIT";

/// What the entry of the caller gets in the launcher.
pub struct Launch<'a> {
    /// The descriptors for the payload, as (the number in the payload, the
    /// descriptor in the launcher): 0 is `/dev/null`, 1 and 2 are the log.
    /// The entry adds its own and passes the list to the child.
    pub pass: Vec<(i64, i64)>,
    /// The log of the container, for a line of the entry (a note of a
    /// fallback). The stderr of the launcher is `/dev/null`, and the ready
    /// pipe carries only the start protocol.
    pub log: &'a mut dyn Write,
}

/// The payload that the entry started.
pub struct Payload {
    /// The pid of the payload, a child of the launcher.
    pub pid: i64,
    /// What the payload needs while it runs (an open file, a held lock).
    /// The launcher drops it after the end of the payload.
    pub keep: Box<dyn std::any::Any>,
}

/// Ask the launcher of a running container to signal its payload.
pub fn signal(s: &Settings, id: &str, sig: i32) -> Result<()> {
    let mut stream = connect(s, id)?;
    writeln!(stream, "{CTL_SIGNAL} {sig}").map_err(launcher_io)?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).map_err(launcher_io)?;
    if line.trim() == "ok" {
        Ok(())
    } else {
        Err(failure(Class::Protocol, format!("the launcher answered {:?}", line.trim())))
    }
}

fn launcher_io(e: std::io::Error) -> Failure {
    Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("the launcher: {e}"), e)
}

/// Why a wait on a launcher failed. Only `Gone` means that the container
/// ended. A failure of the wait itself goes to the caller, never as a
/// stop.
#[derive(Debug)]
pub enum WaitError {
    /// No launcher answers: it ended and closed or removed its socket.
    Gone(Failure),
    /// A launcher listens, and the wait failed for another reason.
    Failed(Failure),
}

/// Wait for a running container to end, with a bound.
///
/// `Ok(None)` is the bound, a result of its own, never a guess of the exit
/// code. A zero bound waits for nothing and answers `Ok(None)` at once:
/// `std` refuses a zero read timeout.
pub fn wait(s: &Settings, id: &str, timeout_ms: u64) -> std::result::Result<Option<i32>, WaitError> {
    let stream = connect(s, id).map_err(WaitError::Gone)?;
    if timeout_ms == 0 {
        return Ok(None);
    }
    stream.set_read_timeout(Some(std::time::Duration::from_millis(timeout_ms))).map_err(|e| {
        WaitError::Failed(Failure::caused(Stage::Host, Class::Unavailable, format!("bounding the wait: {e}"), e))
    })?;
    let mut w = &stream;
    // A launcher that closes the connection between the connect and the
    // write or the read ended: that is `Gone`, as a failed connect is.
    writeln!(w, "{CTL_WAIT}").map_err(|e| WaitError::Gone(launcher_io(e)))?;
    let mut line = String::new();
    match BufReader::new(&stream).read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => match line.trim().strip_prefix("exit ") {
            Some(n) => n.parse::<i32>().map(Some).map_err(|_| {
                WaitError::Failed(failure(Class::Protocol, format!("the launcher answered {:?}", line.trim())))
            }),
            None => Ok(None),
        },
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => Ok(None),
        Err(e) => Err(WaitError::Gone(launcher_io(e))),
    }
}

fn connect(s: &Settings, id: &str) -> Result<UnixStream> {
    // Through `/proc/self/fd`, for the reason of the bind: the real path
    // does not fit in `sun_path`. The directory stays open for the connect.
    let (_dir, path) = table::control_via_fd(s, id)?;
    UnixStream::connect(&path).map_err(|e| {
        Failure::caused(
            Stage::Host,
            Class::Unavailable,
            format!("no launcher is listening for {id}: {e}. The container is not running"),
            e,
        )
    })
}

/// Fork a detached launcher for `container`, and return when its payload
/// reached its `execve` or failed to. The answer is the pids of the
/// launcher and of the payload.
///
/// `enter` runs in the launcher, after the lock, the control socket and the
/// stdio of the payload are ready, and starts the payload: a child of the
/// launcher. Its `Err` text goes back to the caller as the failure.
///
/// The ready answer comes on a pipe, not from a timer: this returns `Ok`
/// only after the launcher said that the payload runs, so a caller never
/// has to guess whether a container that it cannot see yet is starting or
/// dead.
pub fn start<X, F>(s: &Settings, container: &Container<X>, enter: F) -> Result<(i64, i64)>
where
    X: Clone + Serialize + DeserializeOwned,
    F: FnOnce(&Container<X>, Launch<'_>) -> std::result::Result<Payload, String>,
{
    let dir = table::dir(s, &container.id);
    std::fs::create_dir_all(&dir).map_err(|e| crate::io_at(&dir, e))?;

    // The log opens here, before a root changes, and its descriptors go to
    // the payload.
    let log_path = table::log_path(s, &container.id);
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| {
        Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("opening the container log: {e}"), e)
    })?;

    // The ready pipe from the launcher to this process. `O_CLOEXEC` is
    // necessary: without it the payload gets the write end through its
    // `execve` and holds it for its whole life, so the read here sees no
    // end of file until the container ends. A fork keeps the descriptor,
    // which the launcher needs; an exec drops it, which the payload must do.
    let mut ready = [0i32; 2];
    sys::pipe2(&mut ready, sys::O_CLOEXEC)
        .map_err(|e| failure(Class::Unavailable, format!("pipe2: {} ({})", e.name(), e.0)))?;
    let (ready_r, ready_w) = (ready[0] as i64, ready[1] as i64);

    // One thread on this path: `PR_SET_PDEATHSIG` fires at the exit of the
    // thread that made the child, so the path that clones, enters and execs
    // has no thread (`nothing_on_the_spawn_path_can_spawn_a_thread`).
    // SAFETY: the process has one thread on this path, and each child ends
    // with `exit_group`.
    let pid = match unsafe { sys::clone_fork(sys::SIGCHLD) } {
        Ok(0) => {
            // A double fork makes the launcher detached: this middle child
            // forks the launcher and exits at once, so the `wait4` below
            // returns at once, and init becomes the parent of the launcher.
            let _ = sys::close(ready_r);
            // SAFETY: as above.
            match unsafe { sys::clone_fork(sys::SIGCHLD) } {
                Ok(0) => {
                    let code = supervise(s, container, enter, log, ready_w);
                    sys::exit_group(code)
                }
                // The middle child keeps nothing open: its copy of the ready
                // pipe would hold the read of the caller open.
                Ok(_) => sys::exit_group(0),
                Err(e) => {
                    let msg = format!("err clone(2) for the launcher failed: {}", e.name());
                    let _ = sys::write(ready_w, msg.as_bytes());
                    sys::exit_group(1)
                }
            }
        }
        Ok(pid) => pid,
        Err(e) => {
            let _ = sys::close(ready_r);
            let _ = sys::close(ready_w);
            return Err(failure(
                Class::Unavailable,
                format!("clone(2) for the launcher failed: {} ({})", e.name(), e.0),
            ));
        }
    };
    let _ = sys::close(ready_w);

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
    // The middle child is reaped here: it exits at once. Without the reap,
    // each start leaves a zombie.
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
                _ => Err(failure(Class::Protocol, format!("the launcher said {text:?}"))),
            }
        }
        None if text.is_empty() => {
            Err(failure(Class::Unavailable, "the launcher exited and did not say whether the payload started"))
        }
        None => Err(failure(Class::Unavailable, text.trim_start_matches("err ").to_string())),
    }
}

mod supervise;
pub mod tree;
use supervise::supervise;

/// docker's exit code for a `wait4` status: the code of an exit, or 128 and
/// the number of the signal.
pub fn exit_status(status: i32) -> i32 {
    if status & 0x7f == 0 { (status >> 8) & 0xff } else { 128 + (status & 0x7f) }
}

/// Reap the child `pid` and give its exit code. `EINTR` is not a failure.
fn reap(pid: i64) -> Option<i32> {
    let mut status = 0i32;
    loop {
        match sys::wait4(pid, &mut status) {
            Ok(_) => return Some(exit_status(status)),
            Err(e) if e == sys::EINTR => continue,
            Err(_) => return None,
        }
    }
}

/// The end of the payload, as `serve` saw it. The waiters get the code
/// after the kill of the tree and the write of the record.
struct Served {
    code: i32,
    reason: Reason,
    waiters: Vec<UnixStream>,
}

fn reason(walled: bool, signaled: bool) -> Reason {
    if walled {
        Reason::Wall
    } else if signaled {
        Reason::Signal
    } else {
        Reason::Exit
    }
}

/// Wait for the payload to end, and answer control messages meanwhile.
///
/// One `ppoll` over both, and no interval. The bound is long, not absent,
/// and its end is a new poll, not a result: a container that runs for a week
/// is not a failure. At each wake the descendants that ended are reaped.
/// At `wall` the payload gets SIGKILL, and its end comes as any end.
fn serve(listener: &UnixListener, pidfd: i64, pid: i64, failed_code: i32, wall: Option<Instant>) -> Served {
    let mut waiters: Vec<UnixStream> = Vec::new();
    let mut walled = false;
    // SIGINT and SIGTERM of the launcher are blocked and read from a
    // signalfd beside the pidfd, so a signal to the launcher goes to the
    // payload and does not leave it with no parent. The mask stays blocked
    // until the launcher exits: nothing else waits for those signals.
    let mut old = 0u64;
    let sfd = sys::sigblock_shutdown(&mut old).and_then(|_| sys::signalfd_shutdown()).unwrap_or(-1);
    let done = |code: i32, signaled: bool, walled: bool, waiters: Vec<UnixStream>| {
        if sfd >= 0 {
            let _ = sys::close(sfd);
        }
        Served { code, reason: reason(walled, signaled), waiters }
    };
    loop {
        // 60 s, or less up to the wall bound; the end of the bound means
        // that nothing happened.
        let mut bound_ms = 60_000i64;
        if let Some(at) = wall
            && !walled
        {
            let left = at.saturating_duration_since(Instant::now());
            if left.is_zero() {
                // `kill` is safe here, as in `handle`: the parent, before the
                // reap.
                let _ = sys::kill(pid, 9);
                walled = true;
            } else {
                bound_ms = bound_ms.min(left.as_millis() as i64 + 1);
            }
        }
        let mut fds = [
            sys::PollFd { fd: pidfd as i32, events: sys::POLLIN, revents: 0 },
            sys::PollFd { fd: listener.as_raw_fd(), events: sys::POLLIN, revents: 0 },
            // The kernel ignores a negative descriptor, so the shape stays
            // the same when the signalfd failed: that launcher waits with no
            // forward.
            sys::PollFd { fd: sfd as i32, events: if sfd >= 0 { sys::POLLIN } else { 0 }, revents: 0 },
        ];
        match sys::ppoll(&mut fds, bound_ms) {
            Ok(_) => {}
            Err(e) if e == sys::EINTR => continue,
            // The poll failed. Reap with a block, not a spin: the payload
            // is still a child of this process.
            Err(_) => return done(reap(pid).unwrap_or(failed_code), false, walled, waiters),
        }
        if sfd >= 0 && fds[2].revents & (sys::POLLIN | sys::POLLHUP) != 0 {
            let mut buf = [0u8; 128];
            if let Ok(n) = sys::read(sfd, &mut buf)
                && n >= 4
            {
                let signo = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as i64;
                if signo == sys::SIGINT as i64 || signo == sys::SIGTERM as i64 {
                    let _ = sys::kill(pid, signo);
                }
            }
        }
        if fds[1].revents & (sys::POLLIN | sys::POLLHUP) != 0
            && let Ok((stream, _)) = listener.accept()
        {
            handle(stream, pid, &mut waiters);
        }
        // The reap of the descendants that ended. It can take the payload
        // too: then its status comes from here.
        if let Some(status) = tree::sweep(pid).payload {
            return done(exit_status(status), status & 0x7f != 0, walled, waiters);
        }
        if fds[0].revents & (sys::POLLIN | sys::POLLHUP) != 0 {
            // The status comes through the pidfd, so a pid used again
            // between the readiness and the reap cannot redirect it.
            return match sys::waitid_pidfd(pidfd, false) {
                Ok(Some(x)) if x.code == sys::CLD_EXITED => done(x.status, false, walled, waiters),
                Ok(Some(x)) => done(128 + x.status, true, walled, waiters),
                _ => done(reap(pid).unwrap_or(failed_code), false, walled, waiters),
            };
        }
    }
}

fn handle(stream: UnixStream, pid: i64, waiters: &mut Vec<UnixStream>) {
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
        // `kill`, not `pidfd_send_signal`, and safe here only: this process
        // is the parent of the payload and did not reap it, so the pid still
        // names it. A second process could not say that.
        let _ = sys::kill(pid, sig);
        let _ = writeln!(w, "ok");
        return;
    }
    if text.starts_with(CTL_WAIT) {
        // Held, not answered: the waiter gets the exit code when there is
        // one.
        waiters.push(stream);
        return;
    }
    let _ = writeln!(w, "err unknown request");
}

/// The state that the launcher writes into the record of `id`.
fn record<X>(s: &Settings, id: &str, f: impl FnOnce(&mut Container<X>)) -> Result<()>
where
    X: Clone + Serialize + DeserializeOwned,
{
    table::update(s, |t: &mut table::Table<X>| {
        if let Some(c) = t.containers.iter_mut().find(|c| c.id == id) {
            f(c);
        }
        Ok::<_, Failure>(())
    })
}

/// The record of a payload that ended with `code`.
fn ended<X>(c: &mut Container<X>, code: i32) {
    c.state = State::Exited;
    c.exit_code = Some(code);
    c.finished_at = Some(table::now());
    c.pid = None;
    c.launcher_pid = None;
}

#[cfg(test)]
mod tests;
