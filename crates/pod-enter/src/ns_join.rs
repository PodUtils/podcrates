//! The join of the mounts of a running payload ([`crate::spawn_joined`]).
//!
//! A payload of the namespace rung in a user namespace of its own (a
//! caller with no capabilities, `ns_user`) has a mount namespace that this
//! user namespace owns. The kernel gives `setns(CLONE_NEWNS)` only to a
//! holder of `CAP_SYS_ADMIN` over that owner and in the user namespace of
//! the caller, so the caller with no capabilities gets `EPERM`. The owner
//! of a user namespace holds each capability in it: the child joins the
//! user namespace and the mount namespace in one call,
//! `setns(pidfd, CLONE_NEWUSER | CLONE_NEWNS)`. Where the payload is in
//! the user namespace of the caller (a run as root), the join is
//! `CLONE_NEWNS` alone: the kernel refuses a join of the own user
//! namespace with `EINVAL`.
//!
//! A payload in a PID namespace of its own (`ns_pid`) adds `CLONE_NEWPID`
//! to the same call. It sets the PID namespace of the children of the
//! child, not of the child: the child forks the payload of the exec into
//! it and stays as its relay (`ns_pid`).
//!
//! The kernel checks `CAP_SYS_ADMIN` over the owner of the mount namespace
//! with the credentials from before the call. Where the mount namespace of
//! the run belongs to a user namespace that is neither the one of the
//! payload nor the one of this process (a payload in a nested user
//! namespace whose mounts its parent namespace made, PodUtils/podvm#695),
//! the child joins that owner first ([`mount_owner`]), then the payload.
//!
//! The init of such a namespace is not dumpable (`title`, T-1113), and the
//! kernel gives its namespaces only to a holder of `CAP_SYS_PTRACE` over
//! the user namespace of the caller of the run. A caller with no such
//! capability joins the payload of the run in its place ([`joinable`]).

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;

use pod_hostprobe::sys::{self, Errno};

use crate::entry::NS_JOIN_STEP;

/// The step byte of `setns(pidfd, CLONE_NEWUSER | CLONE_NEWNS)`.
pub const NS_JOIN_USER_STEP: u8 = 27;

/// The step bytes of the two joins with `CLONE_NEWPID`.
pub const NS_JOIN_PID_STEP: u8 = 43;
pub const NS_JOIN_USER_PID_STEP: u8 = 44;

/// The step byte of the join of the owner of the mount namespace of the run.
pub const NS_JOIN_OWNER_STEP: u8 = 60;

/// `NS_GET_USERNS` of `<linux/nsfs.h>`: `_IO(0xb7, 0x1)`, the owner of a
/// namespace.
const NS_GET_USERNS: u64 = 0xb701;

/// The join, decided before the fork.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Join {
    pub(crate) pidfd: i64,
    /// The root of the run is an overlay: the landing check asks for one.
    pub(crate) overlay: bool,
    /// The payload is in a user namespace that is not the one of the
    /// caller: the child joins it too.
    pub(crate) user: bool,
    /// The payload is in a PID namespace that is not the one of the
    /// caller: the child joins it for its children.
    pub(crate) pid: bool,
    /// A descriptor of the owner of the mount namespace of the run, which
    /// the child joins first ([`mount_owner`]); the caller holds it open.
    pub(crate) owner: Option<i64>,
}

impl Join {
    /// The join of the payload of `pidfd`. It reads the user and the PID
    /// namespaces here, before the fork.
    pub(crate) fn of(pidfd: i64, overlay: bool) -> Join {
        let (user, pid) = (other_user_namespace(pidfd), other_namespace(pidfd, "pid"));
        Join { pidfd, overlay, user, pid, owner: None }
    }

    /// The namespaces of the call.
    pub(crate) fn flags(&self) -> u64 {
        let user = if self.user { sys::CLONE_NEWUSER } else { 0 };
        let pid = if self.pid { sys::CLONE_NEWPID } else { 0 };
        user | sys::CLONE_NEWNS | pid
    }

    /// The step byte of the call.
    pub(crate) fn step(&self) -> u8 {
        match (self.user, self.pid) {
            (false, false) => NS_JOIN_STEP,
            (true, false) => NS_JOIN_USER_STEP,
            (false, true) => NS_JOIN_PID_STEP,
            (true, true) => NS_JOIN_USER_PID_STEP,
        }
    }

    /// In the child, first after the death signal: not dumpable, then the
    /// owner of the mount namespace where it is another one, then one call:
    /// a pidfd with several flags joins all of them, or none.
    /// Async-signal-safe.
    ///
    /// The child holds the descriptors of the caller (the root of the
    /// entry is a directory of the mount namespace of the caller) until
    /// its `execve`. After the join the payload of the run holds each
    /// capability of the child in its user namespace: only the dumpable
    /// flag keeps it out of `/proc/PID/fd` of the child. The `execve` of
    /// the payload sets the flag again.
    pub(crate) fn enter(&self) -> std::result::Result<(), (u8, Errno)> {
        sys::set_dumpable(false).map_err(|e| (self.step(), e))?;
        if let Some(owner) = self.owner {
            sys::setns(owner, sys::CLONE_NEWUSER).map_err(|e| (NS_JOIN_OWNER_STEP, e))?;
        }
        sys::setns(self.pidfd, self.flags()).map(drop).map_err(|e| (self.step(), e))
    }
}

/// The pid in a `/proc/self/fdinfo` text of a pidfd, where it is a pid of
/// the PID namespace of the reader: the kernel writes 0 for a process
/// outside it and -1 for a process that ended.
pub fn fdinfo_pid(text: &str) -> Option<i64> {
    let line = text.lines().find_map(|l| l.strip_prefix("Pid:"))?;
    line.trim().parse::<i64>().ok().filter(|&p| p > 0)
}

/// The numbers of the line `key:` of a text of `/proc` (`Pid`, `PPid`,
/// `NSpid`); `None` where the line is absent or holds no number.
pub fn ids_of(text: &str, key: &str) -> Option<Vec<i64>> {
    let line = text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?;
    let ids: Option<Vec<i64>> = line.split_whitespace().map(|w| w.parse().ok()).collect();
    ids.filter(|v| !v.is_empty())
}

/// The pid and the pids of each PID namespace of the process of `pidfd`,
/// from its fdinfo: `None` for a process that ended.
fn pidfd_ids(pidfd: i64) -> Option<(i64, Vec<i64>)> {
    let text = std::fs::read_to_string(format!("/proc/self/fdinfo/{pidfd}")).ok()?;
    Some((fdinfo_pid(&text)?, ids_of(&text, "NSpid")?))
}

/// The parent and the pids of each PID namespace of the process `pid` of
/// `/proc` (`self` for this process).
fn status_ids(pid: &str) -> Option<(i64, Vec<i64>)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    Some((*ids_of(&text, "PPid")?.first()?, ids_of(&text, "NSpid")?))
}

/// The payload of a run, which a join opens in place of its init
/// ([`joinable`]).
#[derive(Debug)]
pub struct Member {
    /// Its pid in the PID namespace of `/proc`.
    pub pid: i64,
    /// Its pidfd.
    pub pidfd: OwnedFd,
}

/// The payload of the init of `pidfd`, where the process of `pidfd` is pid
/// 1 of a PID namespace under the one of `/proc` and this process cannot
/// open the namespaces of that init. `None` where a join opens the process
/// of `pidfd`.
///
/// The init of `Plan::pid_namespace` is not dumpable (T-1113): the kernel
/// opens its namespaces (`setns` on its pidfd, `/proc/PID/ns/*`,
/// `/proc/PID/root`) only for a holder of `CAP_SYS_PTRACE` over the user
/// namespace of its memory, the one of the caller of the run. A caller
/// with no capabilities opens the payload: pid 2 of the namespace, the
/// child of the init, in the namespaces of the run while it does not change
/// them. A payload that is not dumpable refuses such a join too.
pub fn joinable(pidfd: i64) -> Option<Member> {
    let (init, ns) = pidfd_ids(pidfd).filter(|(_, ns)| ns.len() > 1 && ns.last() == Some(&1))?;
    // A caller with the right opens the init: the payload can change its
    // own namespaces, the init cannot.
    if std::fs::metadata(format!("/proc/{init}/ns/pid")).is_ok() {
        return None;
    }
    let pids: Vec<i64> = match std::fs::read_to_string(format!("/proc/{init}/task/{init}/children")) {
        Ok(text) => text.split_whitespace().filter_map(|w| w.parse().ok()).collect(),
        // A kernel with no `CONFIG_PROC_CHILDREN`: each process of `/proc`.
        Err(_) => std::fs::read_dir("/proc")
            .ok()?
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse().ok()))
            .collect(),
    };
    // `pidfd_open` takes a pid of the PID namespace of this process, which
    // is under the one of `/proc` after a join of the PID namespace of the
    // run.
    let depth = status_ids("self")?.1.len();
    let payload = |c: i64| {
        let (parent, of_c) = status_ids(&c.to_string())?;
        (parent == init && of_c.len() == ns.len() && of_c.last() == Some(&2)).then_some(of_c)
    };
    pids.into_iter().find_map(|c| {
        let fd = sys::pidfd_open(*payload(c)?.get(depth.checked_sub(1)?)?).ok()?;
        // SAFETY: `pidfd_open` gave the descriptor, and nothing else owns it.
        let member = Member { pid: c, pidfd: unsafe { OwnedFd::from_raw_fd(fd as i32) } };
        // After the open: the descriptor names `c` before and after a new
        // read of `c`, and the init lives, so no other process took either
        // pid between the reads.
        let names = |fd: i64, pid: i64| pidfd_ids(fd).is_some_and(|(p, _)| p == pid);
        (names(fd, c) && payload(c).is_some() && names(fd, c) && names(pidfd, init)).then_some(member)
    })
}

/// The device and inode of a namespace link.
fn ns_id(path: &str) -> Option<(u64, u64)> {
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// Is the process of `pidfd` in a user namespace that is not the one of
/// this process? False where `/proc` does not tell: the join is then the
/// mount namespace alone, as before, and a refusal falls back.
pub fn other_user_namespace(pidfd: i64) -> bool {
    other_namespace(pidfd, "user")
}

/// The name of the step of a join with `CLONE_NEWPID` or of the owner, for
/// [`crate::ns_setup_step`].
pub(crate) fn pid_step_name(byte: u8) -> Option<&'static str> {
    match byte {
        NS_JOIN_PID_STEP => Some("setns(pidfd, CLONE_NEWNS | CLONE_NEWPID)"),
        NS_JOIN_USER_PID_STEP => Some("setns(pidfd, CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWPID)"),
        NS_JOIN_OWNER_STEP => Some("setns(the owner of the mount namespace of the run, CLONE_NEWUSER)"),
        _ => None,
    }
}

/// The owner of the mount namespace of the process of `pidfd`, where it is
/// a user namespace that is neither the one of that process nor the one
/// of this process: the join enters it first. `None` where `/proc` or the
/// kernel (`NS_GET_USERNS`, Linux 4.9) does not tell: the join then goes
/// as before, and a refusal falls back.
pub fn mount_owner(pidfd: i64) -> Option<OwnedFd> {
    let fdinfo = format!("/proc/self/fdinfo/{pidfd}");
    let pid = || std::fs::read_to_string(&fdinfo).ok().as_deref().and_then(fdinfo_pid);
    let p = pid()?;
    let mnt = std::fs::File::open(format!("/proc/{p}/ns/mnt")).ok()?;
    let theirs = ns_id(&format!("/proc/{p}/ns/user"));
    // SAFETY: an open descriptor of a namespace and a request with no
    // argument; the kernel gives a new descriptor or an errno.
    let fd = unsafe { sys::sys(sys::SYS_IOCTL, [mnt.as_raw_fd() as u64, NS_GET_USERNS, 0, 0, 0, 0]) }.ok()?;
    // SAFETY: the ioctl gave the descriptor, and nothing else owns it.
    let owner = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    let of_owner = std::fs::File::from(owner.try_clone().ok()?).metadata().ok().map(|m| (m.dev(), m.ino()));
    // The pid again: a process that ended between the reads gave its
    // number free.
    let other = of_owner.is_some() && of_owner != theirs && of_owner != ns_id("/proc/self/ns/user");
    (other && pid() == Some(p)).then_some(owner)
}

/// Is the process of `pidfd` in a namespace of the kind `kind` (a name
/// under `/proc/PID/ns`) that is not the one of this process? False where
/// `/proc` does not tell.
pub fn other_namespace(pidfd: i64, kind: &str) -> bool {
    let fdinfo = format!("/proc/self/fdinfo/{pidfd}");
    let pid = || std::fs::read_to_string(&fdinfo).ok().as_deref().and_then(fdinfo_pid);
    let Some(p) = pid() else { return false };
    let (own, theirs) = (ns_id(&format!("/proc/self/ns/{kind}")), ns_id(&format!("/proc/{p}/ns/{kind}")));
    // The pid again: a process that ended between the two reads gave its
    // number free, and the link above can be the one of another process.
    let same = pid() == Some(p);
    matches!((own, theirs), (Some(a), Some(b)) if same && a != b)
}

#[cfg(test)]
mod tests;
