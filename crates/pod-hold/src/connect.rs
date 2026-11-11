//! The dial state of a held `connect`, from podnet's `connect.rs` (the cut
//! of PodUtils/podnet#414; podnet's answer PodUtils/podnet#424).
//!
//! The supervisor does not answer the notification of a blocking `connect`
//! while it dials: the kernel keeps the caller blocked, and the supervisor
//! serves the other calls of the payload. More than one call can wait for
//! one dial: a second thread that connects the same socket, or the same
//! thread when the kernel starts its call again after a signal. The kernel
//! does the same for a blocking connect on a socket in `SYN_SENT`.
//!
//! A nonblocking `connect` gets `EINPROGRESS` at once, as in the kernel,
//! and the dial goes on; a second one gets `EALREADY`. A
//! `getsockopt(SO_ERROR)` during the dial waits for its end and gets its
//! result. At the end, each call that still waits gets the result: a
//! `connect` takes the error, else the first `SO_ERROR` takes it. A
//! blocking dial with no waiting call is dropped (the caller exited, or a
//! signal stopped the call); a nonblocking one stays.
//!
//! The dial itself, the entry of the socket, the hand-off of the upstream
//! and the relay are the consumer's: it keys this state by its own key of a
//! socket.

use std::collections::HashMap;
use std::hash::Hash;
use std::os::fd::RawFd;

use crate::mem::{Access, Memory};
use crate::reply::{self, Answer};

/// A call of the payload: its notification, its task and its descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller {
    pub id: u64,
    pub pid: u32,
    pub fd: i32,
}

/// A `getsockopt(SO_ERROR)` that waits for a nonblocking dial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorCall {
    pub id: u64,
    pub pid: u32,
    pub fd: i32,
    /// The addresses of `optval` and `optlen` in the task, and the length
    /// that the task gave in `optlen`.
    pub optval: u64,
    pub optlen: u64,
    pub len: usize,
}

/// The calls that wait for the dial of one socket.
#[derive(Debug, Default)]
struct Waiting {
    connects: Vec<u64>,
    errors: Vec<ErrorCall>,
    /// The first `connect` was nonblocking: it got `EINPROGRESS`.
    nonblocking: bool,
}

/// A `connect` on a socket whose dial is in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joined {
    /// A blocking call: it waits for the dial. Give it no answer now.
    Waits,
    /// A nonblocking call: answer `EALREADY`.
    Already,
}

/// A new dial of a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    /// A blocking call: it waits for the dial. Give it no answer now.
    Waits,
    /// A nonblocking call: answer `EINPROGRESS` now.
    InProgress,
}

/// The dials in progress, keyed by the consumer's key of a socket.
#[derive(Debug)]
pub struct Dials<K> {
    waiting: HashMap<K, Waiting>,
}

impl<K: Hash + Eq + Copy> Default for Dials<K> {
    fn default() -> Self {
        Dials { waiting: HashMap::new() }
    }
}

impl<K: Hash + Eq + Copy> Dials<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// True while a dial of the socket `key` is in progress.
    pub fn in_progress(&self, key: K) -> bool {
        self.waiting.contains_key(&key)
    }

    /// True while a dial of `key` that a nonblocking `connect` started is
    /// in progress.
    pub fn nonblocking(&self, key: K) -> bool {
        self.waiting.get(&key).is_some_and(|w| w.nonblocking)
    }

    /// The `connect` `id` on `key`, first: `None` when no dial of `key` is
    /// in progress (the consumer reads the address and decides), else what
    /// the call gets. `nonblocking` is the `O_NONBLOCK` of the descriptor of
    /// the task ([`reply::child_fd_flags`]).
    pub fn join(&mut self, key: K, id: u64, nonblocking: bool) -> Option<Joined> {
        let waiting = self.waiting.get_mut(&key)?;
        if nonblocking {
            return Some(Joined::Already);
        }
        waiting.connects.push(id);
        Some(Joined::Waits)
    }

    /// The start of a dial of `key` by the `connect` `id`, after the
    /// consumer accepted the address. The consumer dials and calls
    /// [`Dials::finish`] at the end.
    pub fn start(&mut self, key: K, id: u64, nonblocking: bool) -> Started {
        let connects = if nonblocking { Vec::new() } else { vec![id] };
        self.waiting.insert(key, Waiting { connects, errors: Vec::new(), nonblocking });
        if nonblocking { Started::InProgress } else { Started::Waits }
    }

    /// Add a `getsockopt(SO_ERROR)` to the calls that wait for the dial of
    /// `key`. False: no dial is in progress, and the consumer answers the
    /// call from the socket.
    pub fn wait_error(&mut self, key: K, call: ErrorCall) -> bool {
        match self.waiting.get_mut(&key) {
            Some(waiting) => {
                waiting.errors.push(call);
                true
            }
            None => false,
        }
    }

    /// The end of the dial of `key`. Each call that `valid` refuses left
    /// (`reply::id_valid` of the notification descriptor). `None`: the
    /// dial was blocking and no call waits for it; drop the connection.
    pub fn finish(&mut self, key: K, valid: impl Fn(u64) -> bool) -> Option<Finished> {
        let mut waiting = self.waiting.remove(&key).unwrap_or_default();
        waiting.connects.retain(|&id| valid(id));
        waiting.errors.retain(|call| valid(call.id));
        if !waiting.nonblocking && waiting.connects.is_empty() {
            return None;
        }
        Some(Finished { connects: waiting.connects, errors: waiting.errors, nonblocking: waiting.nonblocking })
    }
}

/// The calls that wait for one dial at its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub connects: Vec<u64>,
    pub errors: Vec<ErrorCall>,
    pub nonblocking: bool,
}

impl Finished {
    /// The one call that can take the socket of the dial (a hand-off of
    /// the upstream to the payload): the blocking `connect` that started it
    /// (`first`), or the one `SO_ERROR` of a nonblocking dial; with the ids
    /// that its answer covers.
    pub fn taker(&self, first: Caller) -> (Caller, Vec<u64>) {
        match (&self.connects[..], &self.errors[..]) {
            ([], [call]) => (Caller { id: call.id, pid: call.pid, fd: call.fd }, vec![call.id]),
            _ => (first, self.connects.clone()),
        }
    }

    /// Answer each call with the result `errno` (0: connected). A `connect`
    /// takes the error, else the first `SO_ERROR` takes it, as in the
    /// kernel; `write` puts the value of an `SO_ERROR` into the memory of
    /// its task ([`write_error`]), and a failed write answers `EFAULT`.
    /// True when a call took the result: else the consumer keeps the error
    /// on its socket for a later `SO_ERROR` (the kernel clears it when
    /// `connect` returns it).
    pub fn answer(&self, lfd: impl Answer, errno: i32, mut write: impl FnMut(&ErrorCall, i32) -> bool) -> bool {
        for &id in &self.connects {
            let _ = reply::respond(lfd, id, if errno == 0 { 0 } else { -1 }, errno);
        }
        let mut pending = if self.connects.is_empty() { errno } else { 0 };
        for call in &self.errors {
            let value = std::mem::take(&mut pending);
            let _ = match write(call, value) {
                true => reply::respond(lfd, call.id, 0, 0),
                false => reply::respond(lfd, call.id, -1, libc::EFAULT),
            };
        }
        !self.connects.is_empty() || !self.errors.is_empty()
    }
}

/// Write `errno` as the option of `call`: at most the length that the task
/// gave, and the length written into `optlen`.
pub fn write_error(memory: &Memory, call: &ErrorCall, errno: i32) -> bool {
    let value = errno.to_ne_bytes();
    let out = call.len.min(value.len());
    memory.write(call.optval, &value[..out]).is_ok() && memory.write(call.optlen, &(out as u32).to_ne_bytes()).is_ok()
}

/// [`write_error`] into the memory of the task of `call`, opened now, when
/// the call still waits after the open: a pid used again cannot take the
/// value.
pub fn write_error_to_task(lfd: RawFd, access: Access, call: &ErrorCall, errno: i32) -> bool {
    let memory = Memory::open(call.pid, access);
    reply::id_valid(lfd, call.id) && write_error(&memory, call, errno)
}

#[cfg(test)]
mod tests;
