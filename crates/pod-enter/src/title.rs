//! The command line, the environment, the name and the dump flag of a
//! process of the caller in the PID namespace of a payload (T-1112,
//! PodUtils/podvm#646).
//!
//! The init of [`crate::Plan::pid_namespace`] and the relay of a join of
//! such a run ([`crate::spawn_joined`]) are forks of the caller with no
//! exec. With no title, the payload reads the whole command line of the
//! caller in `/proc/PID/cmdline` (the values of its options) and its whole
//! environment in `/proc/PID/environ` (a token too), and a payload of the
//! same user can trace them and read their memory.
//!
//! [`hide`] points the command line of the process at a short title and
//! its environment at nothing, with `prctl(PR_SET_MM, PR_SET_MM_MAP)`, which
//! needs no capability where `exe_fd` is -1 (Linux 3.18 and later, with
//! `CONFIG_CHECKPOINT_RESTORE`). The strings that Rust and the C library
//! read (`std::env::args`, `environ`) do not move: the kernel reads
//! `/proc/PID/cmdline` and `/proc/PID/environ` through `arg_start` to
//! `env_end` only. The name of the process (`/proc/PID/comm`) takes the
//! title too. A fork takes the map and the name with it, so a child of a
//! hidden process is hidden until its exec. [`restore`] gives both back.
//! [`blank_in_place`] is the second path, for a process that reads no
//! argument and no variable again.
//!
//! The title lives in memory of the heap: the kernel reads those two files
//! with `FOLL_ANON`, so a title in `.data` of the binary reads empty.
//!
//! The entry does the same in its child (`Shown`), which has one thread
//! whatever the threads of the caller: the parent reads the map and makes
//! the title before the fork; the child sets them after the maps of each
//! user namespace and before the limits, the confinement and the fork of
//! the payload. Then the child writes NUL over the old command line and
//! environment in its memory, where the map took and where the kernel
//! refused it. The init and the relay also become not dumpable: a payload
//! with no capability over their memory (the user namespace of the caller)
//! cannot trace them or read `/proc/PID/environ` and `/proc/PID/mem`
//! (T-1113). The kernel opens the namespaces of such an init only for a
//! caller with `CAP_SYS_PTRACE`, so a join of a caller with no
//! capabilities opens the payload of the run ([`crate::joinable`]).

use pod_core::Class;
use pod_hostprobe::sys::{self, CBuf};

use crate::ns_pid::Pid;
use crate::{Error, Plan, Result};

/// `struct prctl_mm_map` of `linux/prctl.h`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Map {
    start_code: u64,
    end_code: u64,
    start_data: u64,
    end_data: u64,
    start_brk: u64,
    brk: u64,
    start_stack: u64,
    arg_start: u64,
    arg_end: u64,
    env_start: u64,
    env_end: u64,
    auxv: u64,
    auxv_size: u32,
    exe_fd: u32,
}

/// The map of this process before [`hide`], for [`restore`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Saved(Map, [u8; 16]);

/// Why the title did not take: the errno of the call that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused {
    pub call: &'static str,
    pub errno: i32,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} failed with errno {}", self.call, self.errno)
    }
}

/// The fields of the map in the text of `/proc/self/stat`: 26 to 28 and 45
/// to 51 of `proc(5)`, counted after the last `)` of the command name.
pub fn map_of(stat: &str, brk: u64) -> Option<Map> {
    map_and_threads(stat, brk).map(|(m, _)| m)
}

/// [`map_of`] and the number of threads, field 20 of `proc(5)`, from the
/// same read: the count is of the moment of the map.
fn map_and_threads(stat: &str, brk: u64) -> Option<(Map, u64)> {
    let rest = &stat[stat.rfind(')')? + 1..];
    // Field 3 (the state) is the first after the name.
    let f: Vec<&str> = rest.split_whitespace().collect();
    let at = |n: usize| -> Option<u64> { f.get(n.checked_sub(3)?)?.parse().ok() };
    let threads = at(20)?;
    let map = Map {
        start_code: at(26)?,
        end_code: at(27)?,
        start_stack: at(28)?,
        start_data: at(45)?,
        end_data: at(46)?,
        start_brk: at(47)?,
        brk,
        arg_start: at(48)?,
        arg_end: at(49)?,
        env_start: at(50)?,
        env_end: at(51)?,
        auxv: 0,
        auxv_size: 0,
        exe_fd: u32::MAX,
    };
    Some((map, threads))
}

/// `map` with the command line at `title` (its bytes and a NUL, at
/// `at`) and an empty environment just after it.
pub fn titled(map: Map, at: u64, len: u64) -> Map {
    Map { arg_start: at, arg_end: at + len, env_start: at + len, env_end: at + len, ..map }
}

/// The map of this process. With `alone`, a refusal where it has more
/// than one thread: another thread can move the break of the heap between
/// its read and the call, and the call would set a stale one.
fn current(alone: bool) -> std::result::Result<Map, Refused> {
    let stat = std::fs::read_to_string("/proc/self/stat")
        .map_err(|e| Refused { call: "read(/proc/self/stat)", errno: e.raw_os_error().unwrap_or(0) })?;
    match map_and_threads(&stat, sys::brk_now()) {
        Some((m, n)) if n == 1 || !alone => Ok(m),
        Some(_) => Err(Refused { call: "a title with more than one thread", errno: 0 }),
        None => Err(Refused { call: "the parse of /proc/self/stat", errno: 0 }),
    }
}

/// Set `map`. Async-signal-safe.
fn set(map: &Map) -> std::result::Result<(), Refused> {
    // SAFETY: the map lives for the call, and the size is its size.
    unsafe { sys::set_mm_map(std::ptr::from_ref(map).cast(), std::mem::size_of::<Map>()) }
        .map(drop)
        .map_err(|e| Refused { call: "prctl(PR_SET_MM_MAP)", errno: e.0 })
}

/// Show `title` as the command line of this process and no environment.
/// With one thread: the break of the heap must not move between its read
/// and the call.
pub fn hide(title: &str) -> std::result::Result<Saved, Refused> {
    hide_as(title, true)
}

/// [`hide`]; `alone` false takes a process of more threads (a test whose
/// other threads wait). The name is of the calling thread.
fn hide_as(title: &str, alone: bool) -> std::result::Result<Saved, Refused> {
    let old = current(alone)?;
    let bytes = CBuf::new(title).ok_or(Refused { call: "a title with a NUL byte", errno: 0 })?;
    let new = titled(old, bytes.ptr(), title.len() as u64 + 1);
    set(&new)?;
    let mut comm = [0u8; 16];
    // The name of a thread is no secret: a failure of either call leaves
    // the title as it is.
    let _ = sys::name_of_thread(&mut comm);
    let _ = sys::set_name(&bytes);
    // The kernel reads the title for the life of the process.
    std::mem::forget(bytes);
    Ok(Saved(old, comm))
}

/// Give back the command line and the environment of [`Saved`].
pub fn restore(saved: &Saved) -> std::result::Result<(), Refused> {
    restore_as(saved, true)
}

fn restore_as(saved: &Saved, alone: bool) -> std::result::Result<(), Refused> {
    let now = current(alone)?;
    let Map { arg_start, arg_end, env_start, env_end, .. } = saved.0;
    set(&Map { arg_start, arg_end, env_start, env_end, ..now })?;
    if saved.1[0] != 0 {
        let len = saved.1.iter().position(|&b| b == 0).unwrap_or(saved.1.len());
        if let Some(name) = CBuf::from_bytes(&saved.1[..len]) {
            let _ = sys::set_name(&name);
        }
    }
    Ok(())
}

/// The second path where [`hide`] is refused, for a process that reads no
/// argument and no variable again: the bytes of its command line and of
/// its environment become NUL in place.
///
/// # Safety
/// No code of this process reads `argv` or `environ` after the call.
pub unsafe fn blank_in_place() -> std::result::Result<(), Refused> {
    let m = current(false)?;
    // SAFETY: the caller reads neither again.
    unsafe { blank(&m) };
    Ok(())
}

/// Write NUL over the command line and the environment of `m`.
/// Async-signal-safe.
///
/// # Safety
/// `m` is the map of this process, and no code reads `argv` or `environ`
/// after the call.
unsafe fn blank(m: &Map) {
    for (start, end) in [(m.arg_start, m.arg_end), (m.env_start, m.env_end)] {
        if start != 0 && end > start {
            // SAFETY: the kernel gives these bounds of this process's own
            // memory, which is mapped and writable.
            unsafe { std::ptr::write_bytes(start as *mut u8, 0, (end - start) as usize) };
        }
    }
}

/// The title of the init or the relay of one fork, made before it.
pub(crate) struct Shown {
    /// The map of the caller; `None` where `/proc/self/stat` does not read.
    /// The child reads its own break.
    map: Option<Map>,
    title: CBuf,
    len: u64,
}

impl Shown {
    /// The title of the fork of `plan` with the PID namespace `pid`:
    /// [`Plan::relay_title`], else the product of [`Plan::names`]. `None`
    /// where the child stays no init and no relay.
    pub(crate) fn of(plan: &Plan, pid: &Pid) -> Result<Option<Shown>> {
        if !pid.relay {
            return Ok(None);
        }
        let text = plan.relay_title.as_deref().unwrap_or(plan.names.product);
        let title = CBuf::new(text)
            .ok_or_else(|| Error::runtime(Class::Usage, format!("the title {text:?} contains a NUL byte")))?;
        let map = std::fs::read_to_string("/proc/self/stat").ok().and_then(|s| map_of(&s, 0));
        Ok(Some(Shown { map, title, len: text.len() as u64 + 1 }))
    }

    /// In the child, with one thread: the title, no environment, the name,
    /// NUL over the old command line and environment, and not dumpable.
    /// Where the kernel refuses the map, `/proc/PID` shows the NUL bytes.
    /// The init and the relay read neither, and the payload execs with its
    /// own. Async-signal-safe.
    pub(crate) fn apply(&self) {
        if let Some(m) = self.map {
            let new = titled(Map { brk: sys::brk_now(), ..m }, self.title.ptr(), self.len);
            // A refused map leaves the old strings in `/proc/PID`; a map
            // that took leaves them in the memory. The NUL bytes go over
            // both.
            let _ = set(&new);
            // SAFETY: `m` is the map of the caller, which the fork copied;
            // the title is on the heap, out of it; this process reads
            // neither again.
            unsafe { blank(&m) };
        }
        let _ = sys::set_name(&self.title);
        let _ = sys::set_dumpable(false);
    }
}

#[cfg(test)]
mod tests;
