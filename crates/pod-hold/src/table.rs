//! The table of the sockets of the payload, and the flow budget.
//!
//! The key of an entry is the inode number of the socket of the program
//! end (the link `/proc/PID/fd/N` is `socket:[INODE]`). A `fork`, a `dup`,
//! a `dup2`, a `dup3`, an `fcntl(F_DUPFD)` and a descriptor sent with
//! `SCM_RIGHTS` give the same socket, thus the same entry, with no call to
//! stop. Each process of the payload and each thread finds the entry of its
//! descriptor with one `readlink`.
//!
//! A `close` removes the entry only when no other descriptor of the payload
//! refers to the socket. Some descriptors close with no `close` call (the
//! exit of a process, `execve` with close-on-exec, `close_range`). A sweep
//! removes the entries of these sockets: it runs when the table doubles,
//! and before the table refuses a socket at its ceiling.
//!
//! The budget: the ceiling is the caller's, else it comes from
//! `RLIMIT_NOFILE` ([`derived_ceiling`]). Each refused `socket` call is
//! counted by its class ([`Refusal`]).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use pod_core::Failure;

use crate::reply::{Answer, proceed};

mod proc;

pub use proc::{inode_of, nofile, parse_socket_link, raise_nofile, socket_inode, sockets_of};

/// The key of an entry: the inode number of the socket of the program end.
/// 0 is not a key: the sockets file system gives no inode 0.
pub type Key = u64;

/// The table size of the first sweep.
const FIRST_SWEEP: usize = 64;

/// The supervisor descriptors of one tracked socket: its end of the pair
/// and the upstream (a datagram socket: its end, a copy and the relay).
const FDS_PER_SOCKET: u64 = 2;
/// The descriptors of the supervisor that are not tracked sockets.
const RESERVE: u64 = 64;
/// The smallest and the largest ceiling.
pub const MIN_CEILING: usize = 64;
pub const MAX_CEILING: usize = 1 << 20;
/// The ceiling of the derived budget.
const MAX_DERIVED: usize = 65_536;

/// The ceiling for a descriptor limit `limit` of the supervisor: the
/// descriptors above a reserve, two for each socket, from 64 to 65536.
pub fn derived_ceiling(limit: u64) -> usize {
    let n = limit.saturating_sub(RESERVE) / FDS_PER_SOCKET;
    (n.min(MAX_DERIVED as u64) as usize).max(MIN_CEILING)
}

/// The class of a refused `socket` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The table is at its ceiling (`EMFILE`).
    TableFull,
    /// The supervisor could not make the socket pair (its own descriptor
    /// limit; `EMFILE`).
    SocketPair,
    /// A handler failed before it answered (`EIO`, or `ECONNREFUSED` for
    /// `connect`).
    Other,
}

impl Refusal {
    pub const ALL: [Refusal; 3] = [Refusal::TableFull, Refusal::SocketPair, Refusal::Other];

    /// The name of the class in the log and in the record.
    pub fn name(self) -> &'static str {
        match self {
            Refusal::TableFull => "table full",
            Refusal::SocketPair => "socketpair failed",
            Refusal::Other => "other",
        }
    }
}

/// The counters of the refusals, one for each class.
#[derive(Default, Debug)]
pub struct Refusals([AtomicU64; 3]);

impl Refusals {
    /// No refusal; usable in a `static` of the consumer.
    pub const fn new() -> Refusals {
        Refusals([const { AtomicU64::new(0) }; 3])
    }

    /// Count one refusal of `class`. Return the new count of the class.
    pub fn count(&self, class: Refusal) -> u64 {
        self.0[class as usize].fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The count of each class, in the order of [`Refusal::ALL`].
    pub fn counts(&self) -> [(Refusal, u64); 3] {
        Refusal::ALL.map(|class| (class, self.0[class as usize].load(Ordering::Relaxed)))
    }

    /// One line with each count and the flow budget `ceiling`, or `None`
    /// when no call was refused.
    pub fn summary(&self, ceiling: usize) -> Option<String> {
        let counts = self.counts();
        let total: u64 = counts.iter().map(|(_, n)| n).sum();
        if total == 0 {
            return None;
        }
        let parts: Vec<String> = counts.iter().map(|(class, n)| format!("{} {n}", class.name())).collect();
        Some(format!("{total} socket calls refused ({}); the flow budget is {ceiling}", parts.join(", ")))
    }
}

/// The table of the sockets of a payload, with an entry of type `T` for
/// each socket.
#[derive(Debug)]
pub struct Table<T> {
    socks: HashMap<Key, T>,
    /// The first process of the payload.
    root: i32,
    /// Each task that made a stopped call, with its thread group id.
    tasks: HashMap<u32, i32>,
    /// The table size at which the next sweep runs.
    sweep_at: usize,
    /// The most entries at one time.
    ceiling: usize,
}

impl<T> std::ops::Deref for Table<T> {
    type Target = HashMap<Key, T>;
    fn deref(&self) -> &Self::Target {
        &self.socks
    }
}

impl<T> std::ops::DerefMut for Table<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.socks
    }
}

impl<T> Table<T> {
    /// The table of the payload whose first process is `root`, with at
    /// most `ceiling` entries (clamped to 64 to 1048576).
    pub fn new(root: i32, ceiling: usize) -> Self {
        Table {
            socks: HashMap::new(),
            root,
            tasks: HashMap::new(),
            sweep_at: FIRST_SWEEP,
            ceiling: ceiling.clamp(MIN_CEILING, MAX_CEILING),
        }
    }

    /// The flow budget: the most sockets at one time.
    pub fn ceiling(&self) -> usize {
        self.ceiling
    }

    /// The key of the descriptor `fd` of the task `pid` (0 when it is not a
    /// socket). Record the process of the task for the sweep.
    pub fn key_of(&mut self, pid: u32, fd: i32) -> Key {
        if !self.tasks.contains_key(&pid) {
            if self.tasks.len() >= 4 * self.ceiling {
                self.tasks.retain(|tid, _| proc::alive(*tid as i32));
            }
            self.tasks.insert(pid, proc::tgid_of(pid));
        }
        socket_inode(pid as i32, fd)
    }

    /// Whether the table has room for one more entry. Sweep first when
    /// the sweep is due.
    pub fn has_room(&mut self) -> bool {
        if self.socks.len() >= self.sweep_at {
            self.sweep();
            self.sweep_at = (self.socks.len() * 2).clamp(FIRST_SWEEP, self.ceiling);
        }
        self.socks.len() < self.ceiling
    }

    /// Remove each entry whose socket no process of the payload holds.
    fn sweep(&mut self) {
        self.tasks.retain(|tid, _| proc::alive(*tid as i32));
        let mut live = HashSet::new();
        let mut read = 0;
        for process in self.processes() {
            if let Some(sockets) = sockets_of(process) {
                read += 1;
                live.extend(sockets.into_iter().map(|(_, key)| key));
            }
        }
        // No table could be read: keep each entry.
        if read > 0 {
            self.socks.retain(|key, _| live.contains(key));
        }
    }

    /// The processes of the payload: the root and its descendants, and
    /// each process that made a stopped call (a process whose parent
    /// exited).
    fn processes(&self) -> HashSet<i32> {
        let mut todo: Vec<i32> = vec![self.root];
        todo.extend(self.tasks.values());
        proc::descendants(todo)
    }

    /// Whether a descriptor of the payload other than `fd` of the task
    /// `pid` refers to the socket `key`.
    pub fn other_copy(&self, pid: u32, fd: i32, key: Key) -> bool {
        let own = sockets_of(pid as i32).unwrap_or_default();
        if own.iter().any(|&(n, k)| k == key && n != fd) {
            return true;
        }
        let caller = self.tasks.get(&pid).copied().unwrap_or(pid as i32);
        self.processes()
            .into_iter()
            .filter(|&process| process != caller)
            .any(|process| sockets_of(process).is_some_and(|s| s.iter().any(|&(_, k)| k == key)))
    }
}

/// `close` of the descriptor `fd` (the socket `key`) by the task `pid`:
/// drop the entry when this is the last descriptor of the socket in the
/// payload. The kernel then does the close.
pub fn handle_close<T>(
    lfd: impl Answer,
    id: u64,
    fd: i32,
    key: Key,
    pid: u32,
    map: &mut Table<T>,
) -> Result<(), Failure> {
    if key != 0 && map.contains_key(&key) && !map.other_copy(pid, fd, key) {
        map.remove(&key);
    }
    proceed(lfd, id)
}

#[cfg(test)]
mod tests;
