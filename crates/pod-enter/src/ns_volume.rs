//! The volumes of a plan on the namespace rung ([`crate::Plan::volumes`],
//! PodUtils/podvm#440), as root and through a user namespace (`ns_user`).
//!
//! Before the fork, the entry checks each volume: the guest path is an
//! absolute normal path that is not `/`, the host path is absolute and
//! exists. A refusal names the volume, before the banner and the fork.
//!
//! In the child, after the private remount of `/`, the tmpfs on `/tmp`,
//! the optional mounts and the device binds, before the chroot: the root
//! opens by its path (a descriptor of the mount namespace of the caller
//! does not take a mount in the new one), and each component of the guest
//! path opens under it with `openat2(RESOLVE_IN_ROOT |
//! RESOLVE_NO_SYMLINKS)`. A missing directory is made; a missing last
//! component is an empty file where the host side is not a directory. A
//! symbolic link of the image gives `ELOOP` (or `EXDEV`): a bind through it
//! would land on a path that the image chose. Then `mount(host,
//! /proc/self/fd/N, MS_BIND | MS_REC)`. A read-only volume opens again
//! (the new walk crosses into the bind), `fstatfs` reads the flags of that
//! mount, and `MS_REMOUNT | MS_BIND | MS_RDONLY` gives each flag that a
//! user namespace locks (`nosuid`, `nodev`, `noexec`, the atime flags) and
//! `nosymfollow` again: the kernel refuses a remount that clears one.
//!
//! A refused step stops the payload with its step byte and the index of
//! the volume, and the parent names the volume: a payload never runs with
//! a volume missing or writable where the plan says read-only. No fallback
//! to chroot either: chroot has no mount namespace.

use pod_beneath::Refusal;
use pod_core::Class;
use pod_hostprobe::sys::{self, CBuf, Errno, OpenHow};

use crate::entry::ns_setup_step;
use crate::ns_dev_bind::mount_on_fd;
use crate::{Error, Plan, Result, RootDir, VolumeBind, errno_class, errno_failure};

/// The step byte of the mount point of a volume (a component that does not
/// open, or that the child cannot make).
pub const NS_VOLUME_POINT_STEP: u8 = 33;
/// The step byte of a guest path through a symbolic link of the image.
pub const NS_VOLUME_LINK_STEP: u8 = 34;
/// The step byte of the bind of the host path.
pub const NS_VOLUME_BIND_STEP: u8 = 35;
/// The step byte of the read-only remount of the bind.
pub const NS_VOLUME_REMOUNT_STEP: u8 = 36;
/// The step byte of the open of a host path under its base with no link
/// (`VolumeBind::host_base`).
pub const NS_VOLUME_SOURCE_STEP: u8 = 48;

/// The buffer of the NUL-ended copy of a host path or its base.
const PATH_BUF: usize = 4096;

/// The most volumes of a plan: the child names a refused volume by its
/// index in one byte.
pub const MOST: usize = 255;

/// Each walk of a guest path: no link, and nothing out of the directory.
const RESOLVE: u64 = sys::RESOLVE_IN_ROOT | sys::RESOLVE_NO_SYMLINKS;

/// One volume, built before the fork.
pub(crate) struct Prepared {
    host: CBuf,
    /// The components of the guest path, in order; at least one.
    parts: Vec<CBuf>,
    /// The host side is a directory: the mount point is a directory.
    dir: bool,
    read_only: bool,
    /// The base and the host path, where the host path opens under the
    /// base with no link (`VolumeBind::host_base`).
    beneath: Option<(Vec<u8>, Vec<u8>)>,
}

/// The volumes and the tmpfs mounts of a plan for the child, built before
/// the fork.
pub(crate) struct Volumes {
    /// The root path: the child opens it in its own mount namespace.
    root: CBuf,
    list: Vec<Prepared>,
    empty: CBuf,
    /// The tmpfs mounts, before the volumes (`ns_tmpfs`).
    tmpfs: crate::ns_tmpfs::Tmpfs,
}

/// A volume as `HOST:GUEST`, with `:ro` where it is read-only, for a
/// refusal.
pub fn volume_label(v: &VolumeBind) -> String {
    format!("{}:{}{}", v.host, v.guest, if v.read_only { ":ro" } else { "" })
}

/// The first volume of `plan` (`the volume HOST:GUEST`), else its first
/// tmpfs (`the tmpfs GUEST:size=N`), else its read-only root, for a refusal
/// of an entry with no mount namespace of its own.
pub(crate) fn first_mount(plan: &Plan) -> Option<String> {
    let volume = plan.volumes.first().map(|v| format!("the volume {}", volume_label(v)));
    let tmpfs = || plan.tmpfs.first().map(|t| format!("the tmpfs {}", crate::tmpfs_label(t)));
    volume.or_else(tmpfs).or_else(|| plan.read_only_root.then(|| "the read-only root".to_string()))
}

/// The components of a guest path, or why it is not one: absolute, not
/// `/`, no `.`, `..` or empty component (a trailing `/` goes).
pub fn guest_parts(guest: &str) -> std::result::Result<Vec<&str>, &'static str> {
    let Some(rest) = guest.strip_prefix('/') else { return Err("is not an absolute path") };
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() {
        return Err("is the guest root `/`, and a mount cannot replace the root");
    }
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.iter().any(|c| c.is_empty() || *c == "." || *c == "..") {
        return Err("is not a normal path: it has a `.`, `..` or empty component");
    }
    Ok(parts)
}

/// Check the volumes and the tmpfs mounts of `plan` and build them for
/// the child. `without` is the entry with no mount namespace of its own
/// (the chroot rung, a rung of the ladder, the entry with no chroot), which
/// refuses each one. Before the banner and the fork.
pub(crate) fn prepare(plan: &Plan, root: &RootDir, without: Option<&str>) -> Result<Volumes> {
    let usage =
        |v: &VolumeBind, why: &str| Error::runtime(Class::Usage, format!("the volume {} {why}", volume_label(v)));
    let tmpfs = crate::ns_tmpfs::Tmpfs::default();
    let mut out = Volumes { root: sys::cempty(), list: Vec::new(), empty: sys::cempty(), tmpfs };
    let Some(first) = first_mount(plan) else { return Ok(out) };
    if let Some(entry) = without {
        return Err(Error::runtime(
            Class::Unsupported,
            format!("{first} needs the namespace rung; {entry} has no mount namespace of its own"),
        ));
    }
    if plan.volumes.len() > MOST {
        return Err(Error::runtime(
            Class::Usage,
            format!("{} volumes; a plan takes at most {MOST}", plan.volumes.len()),
        ));
    }
    out.root = CBuf::new(&root.path)
        .ok_or_else(|| Error::runtime(Class::Usage, format!("{first} has a root path with a NUL byte")))?;
    out.tmpfs = crate::ns_tmpfs::prepare(plan)?;
    let mut seen: Vec<Vec<&str>> = Vec::new();
    for v in &plan.volumes {
        let parts = guest_parts(&v.guest).map_err(|why| usage(v, &format!("has a guest path that {why}")))?;
        if seen.contains(&parts) {
            return Err(usage(v, "repeats the guest path of another volume"));
        }
        if !v.host.starts_with('/') {
            return Err(usage(v, "has a host path that is not an absolute path"));
        }
        let nul = || usage(v, "has a NUL byte");
        let host = CBuf::new(&v.host).ok_or_else(nul)?;
        let parts_c = parts.iter().map(|p| CBuf::new(p).ok_or_else(nul)).collect::<Result<Vec<_>>>()?;
        let st = sys::stat(&host).map_err(|e| {
            let what = if e == sys::ENOENT { "does not exist" } else { "cannot be read" };
            errno_failure(&format!("the host path of the volume {} {what}", volume_label(v)), e)
        })?;
        let dir = st.st_mode & sys::S_IFMT == sys::S_IFDIR;
        let beneath = match &v.host_base {
            None => None,
            Some(base) => Some(beneath(v, base).map_err(|why| usage(v, &why))?),
        };
        if let Some((base, host)) = &beneath {
            // The check before the fork, for a refusal that names the
            // volume; the child opens it again.
            let fd = open_beneath(base, host).map_err(|r| {
                let e = Errno(r.errno());
                let (class, what) = match r {
                    Refusal::Link => (Class::Policy, "holds a symbolic link"),
                    Refusal::DotDot => (Class::Policy, "goes above its base by `..`"),
                    Refusal::NoOpenat2 => {
                        (Class::Unsupported, "cannot open with no link: the kernel has no openat2(2)")
                    }
                    _ => (errno_class(e), "does not open"),
                };
                Error::runtime(
                    class,
                    format!(
                        "the volume {} is refused: its host path {what} under its base ({} ({}))",
                        volume_label(v),
                        e.name(),
                        e.0
                    ),
                )
            })?;
            let _ = sys::close(fd);
        }
        out.list.push(Prepared { host, parts: parts_c, dir, read_only: v.read_only, beneath });
        seen.push(parts);
    }
    Ok(out)
}

/// The base and the host path of a volume with `host_base`, or why not.
fn beneath(v: &VolumeBind, base: &str) -> std::result::Result<(Vec<u8>, Vec<u8>), String> {
    match pod_beneath::split(base.as_bytes(), v.host.as_bytes()) {
        Err(Refusal::BaseNotAbsolute) => Err("has a host base that is not an absolute path".to_string()),
        Err(_) => Err("has a host path that is not under its host base".to_string()),
        Ok(_) if base.contains('\0') || v.host.contains('\0') => Err("has a NUL byte".to_string()),
        Ok(_) => Ok((base.as_bytes().to_vec(), v.host.as_bytes().to_vec())),
    }
}

/// An `O_PATH` descriptor of `host` under `base`, with no link on the way
/// and nothing above the base (`pod-beneath`). No allocation.
fn open_beneath(base: &[u8], host: &[u8]) -> std::result::Result<i64, Refusal> {
    let mut buf = [0u8; PATH_BUF];
    pod_beneath::open_under_base(&sys::Beneath, base, host, pod_beneath::O_PATH, &mut buf).map(i64::from)
}

/// The flags of the read-only remount of a bind whose mount has the
/// `f_flags` of `statfs`: `MS_RDONLY`, and each flag of the mount that a
/// user namespace locks, or that a remount would clear.
pub fn remount_flags(f_flags: u64) -> u64 {
    let base = sys::MS_REMOUNT | sys::MS_BIND | sys::MS_RDONLY;
    if f_flags & sys::ST_VALID == 0 {
        return base;
    }
    [
        (sys::ST_NOSUID, sys::MS_NOSUID),
        (sys::ST_NODEV, sys::MS_NODEV),
        (sys::ST_NOEXEC, sys::MS_NOEXEC),
        (sys::ST_NOATIME, sys::MS_NOATIME),
        (sys::ST_NODIRATIME, sys::MS_NODIRATIME),
        (sys::ST_RELATIME, sys::MS_RELATIME),
        (sys::ST_NOSYMFOLLOW, sys::MS_NOSYMFOLLOW),
    ]
    .iter()
    .filter(|(st, _)| f_flags & st != 0)
    .fold(base, |flags, (_, ms)| flags | ms)
}

/// The step of a refused open: a link of the image, or the mount point.
fn open_step(e: Errno) -> u8 {
    if e == sys::ELOOP || e == sys::EXDEV { NS_VOLUME_LINK_STEP } else { NS_VOLUME_POINT_STEP }
}

fn open_in(dir: i64, name: &CBuf, flags: u64, mode: u64) -> sys::Sysres {
    sys::openat2(dir, name, &OpenHow { flags, mode, resolve: RESOLVE })
}

/// One component of a guest path under `dir`: an `O_PATH` descriptor, made
/// first where it is missing. No allocation.
fn component(dir: i64, name: &CBuf, as_dir: bool) -> std::result::Result<i64, (u8, Errno)> {
    let flags = sys::O_PATH | sys::O_CLOEXEC | if as_dir { sys::O_DIRECTORY } else { 0 };
    match open_in(dir, name, flags, 0) {
        Err(e) if e == sys::ENOENT => {}
        r => return r.map_err(|e| (open_step(e), e)),
    }
    let made = if as_dir {
        sys::mkdirat(dir, name, 0o755).map(drop)
    } else {
        let create = sys::O_CREAT | sys::O_EXCL | sys::O_WRONLY | sys::O_CLOEXEC;
        open_in(dir, name, create, 0o644).map(|fd| {
            let _ = sys::close(fd);
        })
    };
    match made {
        // `EEXIST`: a link, or a file that came after the open; the open
        // below decides.
        Err(e) if e != sys::EEXIST => return Err((NS_VOLUME_POINT_STEP, e)),
        _ => {}
    }
    open_in(dir, name, flags, 0).map_err(|e| (open_step(e), e))
}

/// The mount point of `v` under `root`, one component at a time.
fn point(root: i64, v: &Prepared) -> std::result::Result<i64, (u8, Errno)> {
    walk(root, &v.parts, v.dir)
}

/// The mount point of the guest path `parts` (at least one) under `root`:
/// each missing directory is made, and a missing last component is a
/// directory where `dir_last`, else an empty file. The step of a refusal is a
/// step of a volume. No allocation.
pub(crate) fn walk(root: i64, parts: &[CBuf], dir_last: bool) -> std::result::Result<i64, (u8, Errno)> {
    let last = parts.len() - 1;
    let mut dir = root;
    for (at, name) in parts.iter().enumerate() {
        let next = component(dir, name, at < last || dir_last);
        if dir != root {
            let _ = sys::close(dir);
        }
        dir = next?;
    }
    Ok(dir)
}

/// The bind of one volume, and its remount where it is read-only.
fn bind_one(root: i64, v: &Prepared, empty: &CBuf) -> std::result::Result<(), (u8, Errno)> {
    let fd = point(root, v)?;
    let bound = match &v.beneath {
        None => mount_on_fd(&v.host, fd, sys::MS_BIND | sys::MS_REC, empty),
        // The source by its descriptor: the path can change after the open.
        Some((base, host)) => match open_beneath(base, host) {
            Ok(src) => {
                let r = crate::ns_dev_bind::mount_fd_on_fd(src, fd, sys::MS_BIND | sys::MS_REC, empty);
                let _ = sys::close(src);
                r
            }
            Err(r) => {
                let _ = sys::close(fd);
                return Err((NS_VOLUME_SOURCE_STEP, Errno(r.errno())));
            }
        },
    };
    let _ = sys::close(fd);
    bound.map_err(|e| (NS_VOLUME_BIND_STEP, e))?;
    if !v.read_only {
        return Ok(());
    }
    // A new walk crosses into the bind: the remount needs its root.
    let top = point(root, v).map_err(|(_, e)| (NS_VOLUME_REMOUNT_STEP, e))?;
    let remounted = sys::fstatfs(top).and_then(|fs| mount_on_fd(empty, top, remount_flags(fs.f_flags), empty));
    let _ = sys::close(top);
    remounted.map(drop).map_err(|e| (NS_VOLUME_REMOUNT_STEP, e))
}

/// In the child, after the mounts of the rung and before the chroot: each
/// tmpfs mount (`ns_tmpfs`), then each volume in its order. A volume can
/// be under a tmpfs; a tmpfs under a volume is refused before the fork.
/// `Err` gives the step, the errno and the index of the volume or the
/// tmpfs. No allocation.
pub(crate) fn bind_each(v: &Volumes) -> std::result::Result<(), (u8, Errno, u8)> {
    if v.list.is_empty() && v.tmpfs.is_empty() {
        return Ok(());
    }
    let flags = sys::O_PATH | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
    let root = sys::open(&v.root, flags, 0).map_err(|e| (NS_VOLUME_POINT_STEP, e, 0))?;
    let mut out = crate::ns_tmpfs::mount_each(root, &v.tmpfs);
    for (at, vol) in v.list.iter().enumerate() {
        if out.is_err() {
            break;
        }
        if let Err((step, e)) = bind_one(root, vol, &v.empty) {
            out = Err((step, e, at as u8));
            break;
        }
    }
    let _ = sys::close(root);
    out
}

/// The refusal of a volume that the child reported, or `None` where the
/// step is not a step of a volume.
pub(crate) fn failure(plan: &Plan, step: u8, e: Errno, at: u8) -> Option<Error> {
    if !(NS_VOLUME_POINT_STEP..=NS_VOLUME_REMOUNT_STEP).contains(&step) && step != NS_VOLUME_SOURCE_STEP {
        return crate::ns_tmpfs::failure(plan, step, e, at);
    }
    let words = ns_setup_step(step)?;
    let label = plan.volumes.get(usize::from(at)).map_or_else(|| format!("number {at}"), volume_label);
    let (class, text) = if step == NS_VOLUME_LINK_STEP
        || (step == NS_VOLUME_SOURCE_STEP && (e == sys::ELOOP || e == sys::EXDEV))
    {
        (Class::Policy, format!("the volume {label} is refused: {words} ({} ({}))", e.name(), e.0))
    } else {
        (errno_class(e), format!("the volume {label} could not be bound: {words} failed with {} ({})", e.name(), e.0))
    };
    Some(Error::runtime(class, format!("{text}; the payload did not run")))
}

/// The components of a working directory under `/tmp` or in a tmpfs of
/// the plan on the namespace rung, for [`make_workdir`]: the tmpfs hides a
/// directory that the caller made in the image (PodUtils/podvm#569). Empty
/// elsewhere.
pub(crate) fn tmp_workdir_parts(plan: &Plan) -> Vec<CBuf> {
    let w = plan.working_dir.as_str();
    if !w.starts_with("/tmp/") && !crate::ns_tmpfs::holds(plan, w) {
        return Vec::new();
    }
    guest_parts(w).map(|parts| parts.iter().filter_map(|p| CBuf::new(p)).collect()).unwrap_or_default()
}

/// In the child, after the chroot: make each missing component of `parts`
/// from `/` with no link on the way, and `fchdir` into the last one. No
/// allocation.
pub(crate) fn make_workdir(slash: &CBuf, parts: &[CBuf]) -> std::result::Result<(), Errno> {
    let flags = sys::O_PATH | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
    let mut dir = sys::open(slash, flags, 0)?;
    for p in parts {
        if let Err(e) = sys::mkdirat(dir, p, 0o755)
            && e != sys::EEXIST
        {
            let _ = sys::close(dir);
            return Err(e);
        }
        let next = sys::openat(dir, p, flags, 0);
        let _ = sys::close(dir);
        dir = next?;
    }
    let r = sys::fchdir(dir);
    let _ = sys::close(dir);
    r.map(drop)
}

/// Check the working directory before the fork, on a rung with a chroot: a
/// directory of the image, of the upper directory of an overlay root over
/// it (`upper`, [`crate::ns_upper`]), or of the host side of a volume that
/// serves it. A working directory in a tmpfs of the plan passes: the child
/// makes it there.
pub(crate) fn check_workdir(root: &RootDir, plan: &Plan, upper: Option<crate::ns_upper::UpperDir<'_>>) -> Result<()> {
    if crate::ns_tmpfs::holds(plan, &plan.working_dir) {
        return Ok(());
    }
    let refuse = |kind, label: Option<String>| {
        let mut context = format!("in the image's rootfs {:?}", root.path);
        if let Some(u) = upper {
            context.push_str(&format!(" under the upper layer {:?}", u.path));
        }
        crate::workdir::refuse_workdir(&plan.working_dir, &context, kind, label.as_deref(), None)
    };
    if plan.volumes.is_empty() && upper.is_none() {
        return crate::entry::workdir_kind(root.path.as_str(), &plan.working_dir).map_err(|k| refuse(k, None));
    }
    let view = crate::ns_upper::View::new(root, plan, upper);
    let label = || crate::abi::GuestRoot::volume_label(&view, &plan.working_dir);
    crate::entry::workdir_kind(&view, &plan.working_dir).map_err(|k| refuse(k, label()))
}

#[cfg(test)]
mod tests;
