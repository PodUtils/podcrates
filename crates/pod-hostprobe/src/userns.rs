//! The user namespace of a caller with no capabilities, as `unshare -Ur`
//! makes it: `unshare(CLONE_NEWUSER)`, then `deny` into
//! `/proc/self/setgroups`, then a map of one line in `uid_map` and in
//! `gid_map` (the id of the caller to 0, or to the ids that the caller
//! names: [`Maps::with_inside`]). In that namespace the process holds each
//! capability, whatever its id inside, so a mount namespace that it makes
//! after this takes mounts.
//!
//! The namespace rung of `pod-enter` and the probe leg
//! [`crate::probes::USERNS_LEG`] call [`enter`], so the two cannot drift
//! into different sequences. [`Maps`] is built before a fork, and [`enter`]
//! does only system calls on it: it is async-signal-safe.

use crate::sys::{self, CBuf, Errno};

/// The call of the sequence that the kernel refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Unshare,
    Setgroups,
    UidMap,
    GidMap,
}

impl Step {
    /// The name of the call, for a fallback line and for a probe row.
    pub fn name(self) -> &'static str {
        match self {
            Step::Unshare => "unshare(CLONE_NEWUSER)",
            Step::Setgroups => "writing deny to /proc/self/setgroups",
            Step::UidMap => "writing /proc/self/uid_map",
            Step::GidMap => "writing /proc/self/gid_map",
        }
    }
}

/// The paths and the lines of the maps, built before the fork.
pub struct Maps {
    setgroups: CBuf,
    uid_map: CBuf,
    gid_map: CBuf,
    /// The same three files by name, under a descriptor of `/proc/self`
    /// ([`write_maps_at`]).
    names: [CBuf; 3],
    uid_line: Vec<u8>,
    gid_line: Vec<u8>,
}

/// One line of a map: `<inside> <id> 1`, the id `id` of the parent
/// namespace to the id `inside`. The kernel takes one line with no
/// capability in the parent namespace only where `id` is the effective id
/// of the writer; `inside` can be each id but `u32::MAX`.
pub fn map_line(inside: u32, id: u32) -> String {
    format!("{inside} {id} 1\n")
}

impl Maps {
    /// The maps of `uid` and `gid` to 0.
    pub fn new(uid: u32, gid: u32) -> Maps {
        Maps::with_inside((0, 0), uid, gid)
    }

    /// The maps of `uid` and `gid` to the uid and the gid of `inside`.
    pub fn with_inside(inside: (u32, u32), uid: u32, gid: u32) -> Maps {
        let c = |s: &str| CBuf::new(s).expect("a literal path has no NUL");
        Maps {
            setgroups: c("/proc/self/setgroups"),
            uid_map: c("/proc/self/uid_map"),
            gid_map: c("/proc/self/gid_map"),
            names: [c("setgroups"), c("uid_map"), c("gid_map")],
            uid_line: map_line(inside.0, uid).into_bytes(),
            gid_line: map_line(inside.1, gid).into_bytes(),
        }
    }

    /// The maps of the effective ids of this process to 0: the probe legs.
    pub fn of_caller() -> Maps {
        Maps::of_caller_inside((0, 0))
    }

    /// The maps of the effective ids of this process to the uid and the gid
    /// of `inside`: the namespace rung of `pod-enter`.
    pub fn of_caller_inside(inside: (u32, u32)) -> Maps {
        let id = |nr| unsafe { sys::sys(nr, [0; 6]) }.map_or(0, |n| n as u32);
        Maps::with_inside(inside, id(sys::SYS_GETEUID), id(sys::SYS_GETEGID))
    }

    /// The line of `uid_map`.
    pub fn uid_line(&self) -> &[u8] {
        &self.uid_line
    }

    /// The line of `gid_map`.
    pub fn gid_line(&self) -> &[u8] {
        &self.gid_line
    }
}

/// Is `uid_map` (the text of `/proc/self/uid_map`) the map of the initial
/// user namespace: one line `0 0 4294967295`? Each other map is the map of
/// a user namespace that a process made, also a full map that a runtime
/// wrote: the kernel gives the initial namespace alone each id.
pub fn is_initial_map(uid_map: &str) -> bool {
    let mut lines = uid_map.lines().filter(|l| !l.trim().is_empty());
    let first = lines.next().map(|l| l.split_whitespace().collect::<Vec<_>>());
    first.as_deref() == Some(&["0", "0", "4294967295"][..]) && lines.next().is_none()
}

/// Write `bytes` into the file at `path` under the directory `dir` in one
/// `write(2)`: the kernel takes a map in one write only. No allocation.
fn write_file_at(dir: i64, path: &CBuf, bytes: &[u8]) -> Result<(), Errno> {
    let fd = sys::openat(dir, path, sys::O_WRONLY | sys::O_CLOEXEC, 0)?;
    let n = sys::write(fd, bytes);
    let _ = sys::close(fd);
    match n {
        Ok(n) if n as usize == bytes.len() => Ok(()),
        Ok(_) => Err(sys::EIO),
        Err(e) => Err(e),
    }
}

/// Enter a new user namespace and map the caller in it by `m`. The process
/// must have one thread: the kernel refuses `CLONE_NEWUSER` to a process
/// with more (`EINVAL`). Async-signal-safe.
pub fn enter(m: &Maps) -> Result<(), (Step, Errno)> {
    sys::unshare(sys::CLONE_NEWUSER).map_err(|e| (Step::Unshare, e))?;
    write_maps(m)
}

/// Map the caller by `m` in the user namespace that it is in: the second
/// half of [`enter`], for a child that `clone(CLONE_NEWUSER)` made. The
/// clone makes the user namespace first, so it owns each other namespace
/// of the same call. Async-signal-safe.
pub fn write_maps(m: &Maps) -> Result<(), (Step, Errno)> {
    write_three(sys::AT_FDCWD as i64, [&m.setgroups, &m.uid_map, &m.gid_map], m)
}

/// [`write_maps`] through `proc_self`, a descriptor of `/proc/self` that
/// the process opened before it changed its root: a process whose root
/// holds no `/proc` writes its maps through it. The kernel opens a map of
/// the user namespace that the process is in at the time of the open, so
/// a descriptor of the directory serves a user namespace made after it.
/// Async-signal-safe.
pub fn write_maps_at(proc_self: i64, m: &Maps) -> Result<(), (Step, Errno)> {
    let [setgroups, uid_map, gid_map] = &m.names;
    write_three(proc_self, [setgroups, uid_map, gid_map], m)
}

/// `deny` into `setgroups`, then the two maps, each under `dir`.
fn write_three(dir: i64, [setgroups, uid_map, gid_map]: [&CBuf; 3], m: &Maps) -> Result<(), (Step, Errno)> {
    // A kernel before 3.19 has no `setgroups` file, and takes the map of
    // the group with no `deny`.
    match write_file_at(dir, setgroups, b"deny") {
        Ok(()) | Err(sys::ENOENT) => {}
        Err(e) => return Err((Step::Setgroups, e)),
    }
    write_file_at(dir, uid_map, &m.uid_line).map_err(|e| (Step::UidMap, e))?;
    write_file_at(dir, gid_map, &m.gid_line).map_err(|e| (Step::GidMap, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_is_one_line_of_the_id_to_0() {
        assert_eq!(map_line(0, 1000), "0 1000 1\n");
        let m = Maps::new(1000, 100);
        assert_eq!(m.uid_line(), b"0 1000 1\n");
        assert_eq!(m.gid_line(), b"0 100 1\n");
    }

    /// The ids inside come first, then the ids of the caller, one each.
    #[test]
    fn a_map_with_ids_inside_names_them_first() {
        assert_eq!(map_line(1001, 1000), "1001 1000 1\n");
        let m = Maps::with_inside((1001, 1002), 1000, 100);
        assert_eq!(m.uid_line(), b"1001 1000 1\n");
        assert_eq!(m.gid_line(), b"1002 100 1\n");
        let zero = Maps::with_inside((0, 0), 1000, 100);
        let new = Maps::new(1000, 100);
        assert_eq!((zero.uid_line(), zero.gid_line()), (new.uid_line(), new.gid_line()));
    }

    #[test]
    fn the_maps_of_the_caller_are_its_effective_ids() {
        let euid = unsafe { sys::sys(sys::SYS_GETEUID, [0; 6]) }.expect("geteuid") as u32;
        let egid = unsafe { sys::sys(sys::SYS_GETEGID, [0; 6]) }.expect("getegid") as u32;
        let m = Maps::of_caller();
        assert_eq!(m.uid_line(), map_line(0, euid).as_bytes());
        assert_eq!(m.gid_line(), map_line(0, egid).as_bytes());
        let m = Maps::of_caller_inside((1001, 1001));
        assert_eq!(m.uid_line(), map_line(1001, euid).as_bytes());
        assert_eq!(m.gid_line(), map_line(1001, egid).as_bytes());
    }

    /// The kernel writes each field in a width of 10; only one line of the
    /// full range from 0 is the initial namespace.
    #[test]
    fn only_the_full_map_from_0_is_the_initial_namespace() {
        assert!(is_initial_map("         0          0 4294967295\n"));
        assert!(is_initial_map("0 0 4294967295"));
        for other in
            ["         0       1000          1\n", "0 0 4294967294\n", "0 0 4294967295\n1 1 1\n", "1001 0 1\n", ""]
        {
            assert!(!is_initial_map(other), "{other:?}");
        }
    }

    #[test]
    fn each_step_has_a_name() {
        let names: Vec<_> = [Step::Unshare, Step::Setgroups, Step::UidMap, Step::GidMap].map(Step::name).into();
        assert_eq!(names[0], "unshare(CLONE_NEWUSER)");
        assert!(names.iter().all(|n| !n.is_empty()));
        let mut d = names.clone();
        d.dedup();
        assert_eq!(d.len(), 4);
    }
}
