//! The capability sets of this thread: `capget`, `capset`, the drop of a
//! capability of the bounding set, and the clear of the ambient set. Each
//! is one system call on data on the stack, so a child between its fork
//! and its exec can call it.

use super::{Errno, SYS_PRCTL, Sysres, sys};

pub const SYS_CAPGET: i64 = nr!(capget, __NR_capget);
pub const SYS_CAPSET: i64 = nr!(capset, __NR_capset);

/// `_LINUX_CAPABILITY_VERSION_3`: two 32-bit words for each set.
pub const CAPABILITY_VERSION_3: u32 = 0x2008_0522;
/// `prctl(PR_CAPBSET_DROP, cap)`.
pub const PR_CAPBSET_DROP: u64 = 24;
/// `prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL)`.
pub const PR_CAP_AMBIENT: u64 = 47;
pub const PR_CAP_AMBIENT_CLEAR_ALL: u64 = 4;
/// `CAP_SETPCAP`: a drop from the bounding set needs it.
pub const CAP_SETPCAP: u32 = 8;
/// `CAP_SYS_PTRACE`: it passes the ptrace check of a Landlock domain.
pub const CAP_SYS_PTRACE: u32 = 19;

/// The three sets of one thread, as 64-bit masks (bit `n` is capability `n`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CapSets {
    pub effective: u64,
    pub permitted: u64,
    pub inheritable: u64,
}

#[repr(C)]
struct Header {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Data {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

fn join(d: [Data; 2]) -> CapSets {
    let w = |lo: u32, hi: u32| u64::from(lo) | (u64::from(hi) << 32);
    CapSets {
        effective: w(d[0].effective, d[1].effective),
        permitted: w(d[0].permitted, d[1].permitted),
        inheritable: w(d[0].inheritable, d[1].inheritable),
    }
}

fn split(s: CapSets) -> [Data; 2] {
    let lo = |v: u64| v as u32;
    let hi = |v: u64| (v >> 32) as u32;
    [
        Data { effective: lo(s.effective), permitted: lo(s.permitted), inheritable: lo(s.inheritable) },
        Data { effective: hi(s.effective), permitted: hi(s.permitted), inheritable: hi(s.inheritable) },
    ]
}

/// `capget` of this thread.
pub fn capget() -> Result<CapSets, Errno> {
    let mut header = Header { version: CAPABILITY_VERSION_3, pid: 0 };
    let mut data = [Data::default(); 2];
    let h = &mut header as *mut Header as u64;
    let d = data.as_mut_ptr() as u64;
    unsafe { sys(SYS_CAPGET, [h, d, 0, 0, 0, 0]) }?;
    Ok(join(data))
}

/// `capset` of this thread. The kernel refuses a bit of `permitted` that
/// the thread does not hold (`EPERM`).
pub fn capset(sets: CapSets) -> Sysres {
    let mut header = Header { version: CAPABILITY_VERSION_3, pid: 0 };
    let data = split(sets);
    let h = &mut header as *mut Header as u64;
    let d = data.as_ptr() as u64;
    unsafe { sys(SYS_CAPSET, [h, d, 0, 0, 0, 0]) }
}

/// `prctl(PR_CAPBSET_DROP, cap)`: `EINVAL` for a number past the last
/// capability of the kernel, `EPERM` with no `CAP_SETPCAP`.
pub fn capbset_drop(cap: u32) -> Sysres {
    unsafe { sys(SYS_PRCTL, [PR_CAPBSET_DROP, u64::from(cap), 0, 0, 0, 0]) }
}

/// `prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL)`: `EINVAL` on a kernel
/// older than 4.3, which has no ambient set.
pub fn ambient_clear() -> Sysres {
    unsafe { sys(SYS_PRCTL, [PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0, 0]) }
}
