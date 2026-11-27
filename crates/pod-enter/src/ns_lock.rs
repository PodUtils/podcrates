//! The nested user and mount namespace of the payload (PodUtils/podvm#695,
//! PodUtils/podrunner#378).
//!
//! The kernel locks the flags of a mount (`MNT_LOCK_READONLY`, the locks of
//! `nosuid`, `nodev`, `noexec` and of the atime flags) and each mount under
//! the root (`MNT_LOCKED`) only when it copies a mount namespace into a user
//! namespace with less privilege. A payload that is uid 0 of the user
//! namespace that made the mounts of the rung holds `CAP_SYS_ADMIN` over
//! them: it can remount a `:ro` volume writable, clear `nosuid` of `/proc`,
//! give a tmpfs of the plan another size, and unmount a volume. So on each
//! entry of the namespace rung where the payload is uid 0 (the user
//! namespace of the rung, uid 0 of a user namespace of the caller, and host
//! root), the child makes a nested user namespace after the change of root,
//! then a mount namespace in it: the copy locks each mount, and the payload
//! holds no capability over the namespace of the mounts or over the
//! superblock of a tmpfs.
//!
//! The maps are the identity of each line of the maps of the outer
//! namespace, so each id stays the same and the payload stays uid 0 of its
//! namespace. The kernel takes a map of one id from the process itself
//! (with `deny` in `setgroups`); a map of more ids needs `CAP_SETUID` in the
//! outer namespace, so the parent of the fork writes it, which stays there.
//! `setgroups` then stays as it is. With ids of the plan
//! ([`Plan::userns_ids`]), a caller that is uid 0 of a user namespace that
//! is not the initial one gets the map `<uid> 0 1` instead (`ns_user`).
//!
//! For host root the payload loses each capability of the initial user
//! namespace: it cannot load a module, set the clock, open a raw socket or
//! bind a port under 1024 in the network namespace of the host (call C990
//! of `docs/decisions.md`).

use std::io::Write as _;

use pod_core::Class;
use pod_hostprobe::sys::{self, Errno};
use pod_hostprobe::userns::{Maps, Step};

use crate::entry::NsEntry;
use crate::ns_user::{
    NS_NESTED_GID_MAP_STEP, NS_NESTED_SETGROUPS_STEP, NS_NESTED_UID_MAP_STEP, NS_NESTED_USER_STEP, nested_ids,
    userns_ids,
};
use crate::{Error, Plan, Result};

/// `unshare(CLONE_NEWUSER)` of the nested namespace of host root: a setup
/// step of the rung. A host that gives root no user namespace keeps the
/// rung's fallback to chroot, which refuses a plan with a mount.
pub const NS_LOCK_USER_STEP: u8 = 58;
/// `unshare(CLONE_NEWNS)` in the nested user namespace.
pub const NS_NESTED_NS_STEP: u8 = 59;

/// The nested namespace of one fork, built before it.
pub(crate) struct Nested {
    /// The maps that the child writes itself, or `None` where the parent
    /// writes them ([`Nested::parent_lines`]).
    child: Option<Maps>,
    /// The lines of `uid_map` and of `gid_map` that the parent writes.
    pub(crate) parent_lines: Option<(Vec<u8>, Vec<u8>)>,
    /// A mount namespace after the maps: each entry of the namespace rung.
    mount: bool,
    /// The caller is host root: a refused unshare is a setup step.
    host_root: bool,
    /// The maps are the ids of the plan, not the identity of the lock.
    ids: bool,
}

/// The step of the nested user namespace that a byte names.
pub(crate) fn nested_step(byte: u8) -> Option<Step> {
    match byte {
        NS_NESTED_USER_STEP | NS_LOCK_USER_STEP => Some(Step::Unshare),
        NS_NESTED_SETGROUPS_STEP => Some(Step::Setgroups),
        NS_NESTED_UID_MAP_STEP => Some(Step::UidMap),
        NS_NESTED_GID_MAP_STEP => Some(Step::GidMap),
        _ => None,
    }
}

/// The name of a step byte of this module, for [`crate::ns_setup_step`]:
/// only the setup step.
pub(crate) fn step_name(byte: u8) -> Option<&'static str> {
    (byte == NS_LOCK_USER_STEP).then_some("unshare(CLONE_NEWUSER) of the nested user namespace of host root")
}

/// The fields of each line of a map (`inside outside count`), or `None`
/// where a line is not three numbers.
pub(crate) fn map_lines(map: &str) -> Option<Vec<(u32, u32, u32)>> {
    let line = |l: &str| {
        let w: Vec<u32> = l.split_whitespace().map(|w| w.parse().ok()).collect::<Option<_>>()?;
        <[u32; 3]>::try_from(w).ok().map(|[a, b, c]| (a, b, c))
    };
    map.lines().filter(|l| !l.trim().is_empty()).map(line).collect::<Option<Vec<_>>>().filter(|v| !v.is_empty())
}

/// The identity of each line of `map`: `inside inside count`.
pub(crate) fn identity(lines: &[(u32, u32, u32)]) -> Vec<u8> {
    lines.iter().map(|(a, _, c)| format!("{a} {a} {c}\n")).collect::<String>().into_bytes()
}

/// The ids inside of a map of one line of one id, or `None`.
fn one_id(lines: &[(u32, u32, u32)]) -> Option<u32> {
    match lines {
        [(a, _, 1)] => Some(*a),
        _ => None,
    }
}

/// The lock of a caller of the effective ids `ids` with the maps `uid_map`
/// and `gid_map` of its user namespace: the child writes the map of one
/// id where it holds that id, else the parent writes the identity.
pub(crate) fn lock(ids: (u32, u32), uid_map: &str, gid_map: &str) -> Result<Nested> {
    let unread = |what: &str| {
        Error::runtime(
            Class::Unavailable,
            format!("the lock of the mounts of the rung: /proc/self/{what} does not read as a map"),
        )
    };
    let uids = map_lines(uid_map).ok_or_else(|| unread("uid_map"))?;
    let gids = map_lines(gid_map).ok_or_else(|| unread("gid_map"))?;
    let host_root = pod_hostprobe::userns::is_initial_map(uid_map);
    let child = match (one_id(&uids), one_id(&gids)) {
        (Some(u), Some(g)) if (u, g) == ids => Some(Maps::with_inside(ids, ids.0, ids.1)),
        _ => None,
    };
    let parent_lines = child.is_none().then(|| (identity(&uids), identity(&gids)));
    Ok(Nested { child, parent_lines, mount: true, host_root, ids: false })
}

/// The nested namespace of `plan` on an entry, built before the fork, or
/// `None`. A join takes none (the namespaces of the run hold), nor the
/// entry with no chroot. The namespace rung takes the lock where its
/// payload is uid 0; a chroot rung of uid 0 of a user namespace that is
/// not the initial one takes the ids of the plan, with no mount namespace.
pub(crate) fn nested_for(plan: &Plan, ns: Option<&NsEntry<'_>>, userland: bool) -> Result<Option<Nested>> {
    if userland || matches!(ns, Some(NsEntry::Join(..))) {
        return Ok(None);
    }
    let read = |name: &str| std::fs::read_to_string(format!("/proc/self/{name}"));
    let own = matches!(ns, Some(NsEntry::Own(_)));
    // The user namespace of the rung maps the caller to the ids of the
    // plan: a payload of uid 0 there takes the lock with a map of one id.
    if let Some(NsEntry::Own(m)) = ns
        && m.user.is_some()
    {
        let ids = userns_ids(plan)?;
        let child = Some(Maps::with_inside(ids, ids.0, ids.1));
        let lock = Nested { child, parent_lines: None, mount: true, host_root: false, ids: false };
        return Ok((ids.0 == 0).then_some(lock));
    }
    let euid = unsafe { sys::syscall6(sys::SYS_GETEUID, 0, 0, 0, 0, 0, 0) } as u32;
    if euid != 0 {
        return Ok(None);
    }
    let ids = plan.userns_ids.map(|_| userns_ids(plan)).transpose()?;
    // The ids of the plan for uid 0 of a user namespace that is not the
    // initial one; host root keeps its ids (T-791).
    if let Some(ids) = nested_ids(ids, euid, || read("uid_map"))? {
        let maps = Maps::of_caller_inside(ids);
        return Ok(Some(Nested { child: Some(maps), parent_lines: None, mount: own, host_root: false, ids: true }));
    }
    if !own {
        return Ok(None);
    }
    let egid = unsafe { sys::syscall6(sys::SYS_GETEGID, 0, 0, 0, 0, 0, 0) } as u32;
    let unread = |e: std::io::Error| {
        let what = format!("the lock of the mounts of the rung: the maps of this process cannot be read ({e})");
        Error::runtime(Class::Unavailable, what)
    };
    let uid_map = read("uid_map").map_err(unread)?;
    let gid_map = read("gid_map").map_err(unread)?;
    lock((0, egid), &uid_map, &gid_map).map(Some)
}

impl Nested {
    /// The byte after the step and the errno of a refusal: 0 for the ids
    /// of the plan, 1 for the lock ([`failure`]).
    pub(crate) fn at(&self) -> u8 {
        u8::from(!self.ids)
    }

    /// In the child, after the change of root and before the working
    /// directory: the user namespace, its maps, then the mount namespace.
    /// `proc_self` is a descriptor of `/proc/self` opened before the change
    /// of root; `pipes` go to the parent where it writes the maps.
    /// Async-signal-safe.
    pub(crate) fn enter(&self, proc_self: i64, pipes: &Pipes) -> std::result::Result<(), (u8, Errno)> {
        let (ask, answer) = pipes.0.map_or((-1, -1), |[_, ask, answer, _]| (ask, answer));
        let unshare = if self.host_root { NS_LOCK_USER_STEP } else { NS_NESTED_USER_STEP };
        sys::unshare(sys::CLONE_NEWUSER).map_err(|e| (unshare, e))?;
        match &self.child {
            Some(maps) => pod_hostprobe::userns::write_maps_at(proc_self, maps).map_err(|(step, e)| {
                let byte = match step {
                    Step::Unshare => NS_NESTED_USER_STEP,
                    Step::Setgroups => NS_NESTED_SETGROUPS_STEP,
                    Step::UidMap => NS_NESTED_UID_MAP_STEP,
                    Step::GidMap => NS_NESTED_GID_MAP_STEP,
                };
                (byte, e)
            })?,
            None => {
                // One byte asks the parent; three come back: the step and
                // the errno, or a zero step.
                sys::write(ask, &[1]).map_err(|e| (NS_NESTED_UID_MAP_STEP, e))?;
                let mut got = [0u8; 3];
                match sys::read(answer, &mut got) {
                    Ok(3) if got[0] == 0 => {}
                    Ok(3) => return Err((got[0], Errno(i32::from(got[1]) | (i32::from(got[2]) << 8)))),
                    Ok(_) => return Err((NS_NESTED_UID_MAP_STEP, sys::EIO)),
                    Err(e) => return Err((NS_NESTED_UID_MAP_STEP, e)),
                }
            }
        }
        if self.mount {
            sys::unshare(sys::CLONE_NEWNS).map_err(|e| (NS_NESTED_NS_STEP, e))?;
        }
        Ok(())
    }
}

/// The two pipes between the child and the parent where the parent writes
/// the maps: the read end of the ask, its write end, the read end of the
/// answer and its write end. `None` where the child writes them.
pub(crate) struct Pipes(Option<[i64; 4]>);

impl Pipes {
    /// The pipes for `nested`, made before the fork, clear of the passed
    /// descriptors.
    pub(crate) fn of(plan: &Plan, nested: Option<&Nested>) -> Result<Pipes> {
        if nested.is_none_or(|n| n.parent_lines.is_none()) {
            return Ok(Pipes(None));
        }
        let mut fds = [-1i64; 4];
        let mut out = Ok(());
        for at in [0, 2] {
            let mut p = [0i32; 2];
            if let Err(e) = sys::pipe2(&mut p, sys::O_CLOEXEC) {
                out = Err(crate::errno_failure("pipe2 for the maps of the nested user namespace failed", e));
                break;
            }
            for (i, fd) in p.into_iter().enumerate() {
                match crate::pass_clear::clear_of_passes(plan, i64::from(fd)) {
                    Ok(fd) => fds[at + i] = fd,
                    Err(e) => out = Err(e),
                }
            }
        }
        if out.is_err() {
            for fd in fds.into_iter().filter(|fd| *fd >= 0) {
                let _ = sys::close(fd);
            }
        }
        out.map(|()| Pipes(Some(fds)))
    }

    /// Close the ends of the parent: in the child first, and in the parent
    /// after its answer.
    pub(crate) fn close_parent_ends(&self) {
        if let Some([ask, _, _, answer]) = self.0 {
            let _ = (sys::close(ask), sys::close(answer));
        }
    }

    /// Close the ends of the child: in the parent after the fork, and in
    /// the child after the maps.
    pub(crate) fn close_child_ends(&self) {
        if let Some([_, ask, answer, _]) = self.0 {
            let _ = (sys::close(ask), sys::close(answer));
        }
    }

    /// In the parent, after the fork: the maps of the child `pid` where the
    /// parent writes them ([`serve`]).
    pub(crate) fn serve(&self, nested: Option<&Nested>, pid: i64) {
        if let (Some([ask, _, _, answer]), Some(lines)) = (self.0, nested.and_then(|n| n.parent_lines.as_ref())) {
            serve(lines, pid, ask, answer);
        }
    }
}

/// In the parent, after the fork, where it writes the maps: wait for the
/// ask of the child `pid` on `ask`, write the lines into its maps, and
/// give the answer on `answer`. An end of file on `ask` is a child that
/// stopped before it asked: nothing to write.
fn serve(lines: &(Vec<u8>, Vec<u8>), pid: i64, ask: i64, answer: i64) {
    let mut b = [0u8; 1];
    let asked = loop {
        match sys::read(ask, &mut b) {
            Ok(n) => break n == 1,
            Err(e) if e == sys::EINTR => continue,
            Err(_) => break false,
        }
    };
    if !asked {
        return;
    }
    let write = |name: &str, bytes: &[u8]| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().write(true).open(format!("/proc/{pid}/{name}"))?;
        // The kernel takes a map in one write only.
        match f.write(bytes)? {
            n if n == bytes.len() => Ok(()),
            _ => Err(std::io::Error::from_raw_os_error(sys::EIO.0)),
        }
    };
    let errno = |e: std::io::Error| Errno(e.raw_os_error().unwrap_or(sys::EIO.0));
    let (step, e) = match write("uid_map", &lines.0) {
        Err(e) => (NS_NESTED_UID_MAP_STEP, errno(e)),
        Ok(()) => match write("gid_map", &lines.1) {
            Err(e) => (NS_NESTED_GID_MAP_STEP, errno(e)),
            Ok(()) => (0, Errno(0)),
        },
    };
    let _ = sys::write(answer, &[step, (e.0 & 0xff) as u8, ((e.0 >> 8) & 0xff) as u8]);
}

/// The refusal of a step of the nested namespace, or `None` where `byte`
/// is another step or the setup step of host root. `at` is
/// [`Nested::at`]: the ids of the plan, or the lock.
pub(crate) fn failure(plan: &Plan, byte: u8, errno: Errno, at: u8) -> Option<Error> {
    let call = match byte {
        NS_NESTED_NS_STEP => "unshare(CLONE_NEWNS)",
        NS_LOCK_USER_STEP => return None,
        _ => nested_step(byte)?.name(),
    };
    let need = match plan.userns_ids {
        Some((u, g)) if at == 0 => format!(
            "the ids {u}:{g} of the plan need a nested user namespace (the caller is uid 0 of a user namespace \
             that is not the initial one)"
        ),
        _ => {
            "the payload of uid 0 needs a nested user namespace that locks the mounts of the namespace rung".to_string()
        }
    };
    Some(Error::runtime(
        crate::errno_class(errno),
        format!("{need}, and {call} failed with {} ({}); the payload did not run", errno.name(), errno.0),
    ))
}

#[cfg(test)]
mod tests;
