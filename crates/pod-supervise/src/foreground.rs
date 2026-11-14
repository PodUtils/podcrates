//! A foreground hold: the record of a payload that the caller runs and
//! waits for itself, with no launcher.
//!
//! The hold does for the record what a launcher does. It holds the lock of
//! the container for its life, so [`crate::table::reconcile`] sees a live
//! holder, and a holder that is killed reads as `Dead`. It serves the
//! control socket on one thread: a `SIG` request goes to the payload, and
//! a `WAIT` request gets the code that [`Foreground::finish`] gives. So
//! `kill`, `stop`, `wait`, `remove`, `list` and `reconcile` work on a
//! foreground payload with no change.
//!
//! The caller reaps the payload, not the hold. A signal by pid after that
//! reap can reach another process that got the pid. So the hold signals
//! through a pidfd that it opens at the start, and a reaped payload answers
//! `ESRCH`, which the hold ignores. The hold signals the payload only, not
//! its process group, as the launcher does.
//!
//! The hold does not apply `wall_ms`, does not kill the tree of the payload
//! and writes no `end`: the caller runs the payload and measures it.
//!
//! The thread of the hold makes a fork unsafe (a lock of the allocator can
//! be held at the fork). While a hold lives in this process,
//! [`crate::start`] refuses. Start the payload first, then the hold.

use std::io::Write;
use std::marker::PhantomData;
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

use pod_core::{Class, Failure, Stage};
use pod_hostprobe::sys;
use serde::{Serialize, de::DeserializeOwned};

use crate::launcher;
use crate::lock::Lock;
use crate::table::{self, State};
use crate::{Result, Settings, failure};

/// The holds that live in this process.
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// The count of the holds that live in this process.
pub(crate) fn live() -> usize {
    LIVE.load(Ordering::Acquire)
}

/// A raw descriptor that closes at the drop.
struct Fd(i64);

impl Drop for Fd {
    fn drop(&mut self) {
        let _ = sys::close(self.0);
    }
}

/// The hold of a running foreground payload. [`Foreground::finish`] writes
/// the end; a drop with no `finish` writes `Dead`.
pub struct Foreground<X>
where
    X: Clone + Serialize + DeserializeOwned,
{
    s: Settings,
    id: String,
    live: Option<Live>,
    of: PhantomData<fn() -> X>,
}

struct Live {
    /// The statement "a holder is alive". It goes last, after the table
    /// write and the stop of the thread.
    lock: Lock,
    /// The write end of the stop pipe: `exit N`, then its close.
    stop: Fd,
    server: JoinHandle<()>,
}

/// Hold the record `want` for a payload that the caller started and waits
/// for: take the lock of the container, serve its control socket, and
/// write `Running` with `payload_pid` and the start time.
///
/// `payload_pid` must be a process that the caller did not reap yet.
pub fn foreground<X>(s: &Settings, want: &str, payload_pid: i64) -> Result<Foreground<X>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let table = table::reconcile::<X>(s)?;
    let c = table::find(s, &table, want)?;
    if c.state == State::Running {
        return Err(failure(Class::Usage, format!("{} is already running", c.name)));
    }
    if payload_pid <= 0 {
        return Err(failure(Class::Usage, format!("the pid {payload_pid} names no payload")));
    }
    let pidfd = sys::pidfd_open(payload_pid).map(Fd).map_err(|e| {
        failure(
            Class::Unavailable,
            format!(
                "pidfd_open on the payload {payload_pid} failed: {} ({}). The hold does not signal a \
                 payload by pid",
                e.name(),
                e.0
            ),
        )
    })?;
    let dir = table::dir(s, &c.id);
    crate::private_dir(&dir).map_err(|e| crate::io_at(&dir, e))?;
    // The lock before the write of `Running`, as in the launcher.
    let lock_path = table::lock_path(s, &c.id);
    let Some(lock) = Lock::try_exclusive(&lock_path)? else {
        return Err(failure(
            Class::Usage,
            format!("{} is already running: another process holds {}", c.name, lock_path.display()),
        ));
    };
    let mut ends = [0i32; 2];
    sys::pipe2(&mut ends, sys::O_CLOEXEC)
        .map_err(|e| failure(Class::Unavailable, format!("pipe2: {} ({})", e.name(), e.0)))?;
    let (stop_r, stop_w) = (Fd(ends[0] as i64), Fd(ends[1] as i64));
    let listener = launcher::listen(s, &c.id).map_err(|e| failure(Class::Unavailable, e))?;
    let server = match std::thread::Builder::new()
        .name("pod-supervise-hold".to_string())
        .spawn(move || serve(listener, pidfd, stop_r))
    {
        Ok(h) => h,
        Err(e) => {
            let _ = std::fs::remove_file(table::control_path(s, &c.id));
            return Err(Failure::caused(
                Stage::Host,
                Class::Unavailable,
                format!("starting the thread of the hold: {e}"),
                e,
            ));
        }
    };
    LIVE.fetch_add(1, Ordering::AcqRel);
    let mut hold =
        Foreground { s: s.clone(), id: c.id.clone(), live: Some(Live { lock, stop: stop_w, server }), of: PhantomData };
    let holder = sys::getpid();
    let wrote = launcher::record::<X>(s, &c.id, |c| c.mark_running(payload_pid, holder));
    if let Err(e) = wrote {
        // The record never said `Running`: no `Dead` either.
        hold.close(None);
        return Err(e);
    }
    Ok(hold)
}

impl<X> Foreground<X>
where
    X: Clone + Serialize + DeserializeOwned,
{
    /// The id of the container.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The payload ended with `code` (docker's code: 128 and the signal for
    /// a signal, [`launcher::exit_status`]). Write `Exited`, the code and
    /// the time, then give the code to each waiter, then release the lock.
    pub fn finish(mut self, code: i32) -> Result<()> {
        let wrote = launcher::record::<X>(&self.s, &self.id, |c| launcher::ended(c, code));
        // A waiter gets the code only when the record holds it.
        self.close(wrote.as_ref().ok().map(|_| code));
        wrote
    }

    /// The end of the hold, after the table write: no new client, the code
    /// (or no answer) to each waiter, the stop of the thread, then the
    /// release of the lock.
    fn close(&mut self, code: Option<i32>) {
        let Some(Live { lock, stop, server }) = self.live.take() else {
            return;
        };
        // A client that connects from here fails, and reads the record.
        let _ = std::fs::remove_file(table::control_path(&self.s, &self.id));
        if let Some(code) = code {
            let _ = sys::write(stop.0, format!("exit {code}").as_bytes());
        }
        // The end of the pipe stops the thread.
        drop(stop);
        let _ = server.join();
        LIVE.fetch_sub(1, Ordering::AcqRel);
        drop(lock);
    }
}

impl<X> Drop for Foreground<X>
where
    X: Clone + Serialize + DeserializeOwned,
{
    /// No `finish`: the end of the payload is not known. The record reads
    /// `Dead`, as the reconcile of a killed launcher writes it.
    fn drop(&mut self) {
        if self.live.is_none() {
            return;
        }
        let now = table::now();
        let _ = launcher::record::<X>(&self.s, &self.id, |c| {
            if c.state == State::Running {
                c.mark_dead(&now);
            }
        });
        self.close(None);
    }
}

/// The thread of a hold: answer each request until the stop pipe ends.
fn serve(listener: UnixListener, pidfd: Fd, stop: Fd) {
    let mut waiters: Vec<UnixStream> = Vec::new();
    let code = loop {
        let mut fds = [
            sys::PollFd { fd: listener.as_raw_fd(), events: sys::POLLIN, revents: 0 },
            sys::PollFd { fd: stop.0 as i32, events: sys::POLLIN, revents: 0 },
        ];
        // No bound: the stop pipe ends each hold.
        match sys::ppoll(&mut fds, -1) {
            Ok(_) => {}
            Err(e) if e == sys::EINTR => continue,
            // The poll failed: no more requests, and the waiters get the end.
            Err(_) => break read_end(&stop),
        }
        if fds[1].revents & (sys::POLLIN | sys::POLLHUP) != 0 {
            break read_end(&stop);
        }
        if fds[0].revents & (sys::POLLIN | sys::POLLHUP) != 0
            && let Ok((stream, _)) = listener.accept()
        {
            // Through the pidfd: the caller can reap the payload at any time.
            launcher::handle(
                stream,
                |sig| {
                    let _ = sys::pidfd_send_signal(pidfd.0, sig);
                },
                &mut waiters,
            );
        }
    };
    // With no code, each waiter gets the end of the stream, and `wait`
    // reads the record again.
    if let Some(code) = code {
        for mut w in waiters {
            let _ = writeln!(w, "exit {code}");
        }
    }
}

/// The text of the stop pipe, to its end: `exit N`, or nothing.
fn read_end(stop: &Fd) -> Option<i32> {
    let mut text = Vec::new();
    let mut buf = [0u8; 64];
    loop {
        match sys::read(stop.0, &mut buf) {
            Ok(0) => break,
            Ok(n) => text.extend_from_slice(&buf[..n as usize]),
            Err(e) if e == sys::EINTR => continue,
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&text).trim().strip_prefix("exit ")?.parse().ok()
}
