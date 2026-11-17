//! The device nodes of the host on the namespace rung in a user namespace
//! (`ns_user`, PodUtils/podvm#414).
//!
//! In a user namespace the kernel refuses `mknod`, so an image that
//! `pod-rootfs` completed there holds regular files at `/dev/null`,
//! `/dev/urandom` and the others: a read of `/dev/urandom` ends after the
//! bytes of the file. The child binds the host's character device on each
//! of [`DEVICES`] in the image, after the private remount of `/` and before
//! the chroot, then remounts the bind `nosuid` and `noexec`. `nodev` stays
//! off: the node must work.
//!
//! The kind of each path in the image comes from the descriptor of the
//! root with `O_NOFOLLOW`, before the fork ([`choose`]). The child binds
//! onto a descriptor (`/proc/self/fd/N` of an `O_PATH | O_NOFOLLOW` open
//! through the descriptor of `/dev`), never onto a path that a link can
//! redirect: a link in the image never sends a mount out of the image. A
//! refused step writes one line, and the rung goes on (as `ns_optional`).
//!
//! Where the image has a `/dev/pts` directory, `/dev/ptmx` takes a bind
//! too: of the `ptmx` of the new devpts instance (`ns_pts`), or of the
//! host's node where the kernel refused that instance.

use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::entry::{decimal, ns_setup_step};
use crate::names::Names;
use crate::ns_optional::refused_line;

/// The names under `/dev` that the child binds, in their order.
pub const DEVICES: [&str; 6] = ["null", "zero", "full", "random", "urandom", "tty"];

/// The step byte of the mount point of a device. It names its step but
/// never goes to the readiness pipe, as [`crate::NS_PROC_STEP`].
pub const NS_DEV_POINT_STEP: u8 = 30;
/// The step byte of the bind of a device, as [`NS_DEV_POINT_STEP`].
pub const NS_DEV_BIND_STEP: u8 = 31;
/// The step byte of the remount of that bind, as [`NS_DEV_POINT_STEP`].
pub const NS_DEV_REMOUNT_STEP: u8 = 32;

/// What the image holds at a path of [`DEVICES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Missing,
    Regular,
    CharDevice,
    Link,
    Directory,
    /// A fifo, a socket or a block device.
    Other,
    /// The stat failed with another errno than `ENOENT`.
    Unreadable,
}

/// The kind of a mode of `stat`.
pub fn kind_of(mode: u32) -> Kind {
    match mode & sys::S_IFMT {
        sys::S_IFREG => Kind::Regular,
        m if u64::from(m) == sys::S_IFCHR => Kind::CharDevice,
        sys::S_IFLNK => Kind::Link,
        sys::S_IFDIR => Kind::Directory,
        _ => Kind::Other,
    }
}

/// What the child does at one path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A bind of the host's node; `create`: first an empty file as the
    /// mount point.
    Bind { create: bool },
    /// No bind; why, for the line.
    Skip(&'static str),
}

/// The choice for a path whose kind in the image is `image`, where the
/// host's node is a character device or not. A link is never a mount
/// point: a mount follows it out of the image. A character device of the
/// image takes the host's node too (C668).
pub fn choose(image: Kind, host_char: bool) -> Choice {
    let why = match image {
        Kind::Missing | Kind::Regular | Kind::CharDevice if !host_char => "the host's node is not a character device",
        Kind::Missing => return Choice::Bind { create: true },
        Kind::Regular | Kind::CharDevice => return Choice::Bind { create: false },
        Kind::Link => "the image holds a symbolic link there, and the entry binds nothing over a link",
        Kind::Directory => "the image holds a directory there",
        Kind::Other => "the image holds a file of another kind there",
        Kind::Unreadable => "the entry cannot read that path of the image",
    };
    Choice::Skip(why)
}

/// The flags of the remount of a bind: no set-user-ID and no program, and
/// read-only where the host's node is on a read-only mount (the kernel
/// refuses to clear a flag of a locked mount). No `MS_NODEV`.
pub fn remount_flags(read_only: bool) -> u64 {
    let ro = if read_only { sys::MS_RDONLY } else { 0 };
    sys::MS_REMOUNT | sys::MS_BIND | sys::MS_NOSUID | sys::MS_NOEXEC | ro
}

/// One bind, built before the fork.
pub(crate) struct Device {
    /// The name of [`DEVICES`], for the tests.
    #[cfg_attr(not(test), expect(dead_code))]
    label: &'static str,
    source: CBuf,
    name: CBuf,
    create: bool,
    remount: u64,
    point_refused: Vec<u8>,
    bind_refused: Vec<u8>,
    remount_refused: Vec<u8>,
}

/// The binds of the devices, built before the fork, used only in a user
/// namespace of the rung.
pub(crate) struct Devices {
    /// The `/dev` of the image by path: the child opens it with
    /// `O_NOFOLLOW` after the overlay root, where there is one.
    dir: CBuf,
    /// The empty path of `AT_EMPTY_PATH` and of a remount.
    empty: CBuf,
    binds: Vec<Device>,
    /// The lines of the paths that the child does not bind.
    skipped: Vec<Vec<u8>>,
    /// The bind of `/dev/ptmx`, where the image has a `/dev/pts` directory.
    ptmx: Option<Ptmx>,
}

/// The two binds of `/dev/ptmx`, built before the fork: the child takes
/// one. `Err` holds the line of a path that the child does not bind.
pub(crate) struct Ptmx {
    /// The `ptmx` of the new devpts instance.
    own: Result<Device, Vec<u8>>,
    /// The host's `/dev/ptmx`, where the kernel refused the new instance.
    host: Result<Device, Vec<u8>>,
}

/// The start of the line of a refused step at `/dev/NAME`.
pub fn refused_start(names: &Names, name: &str, step: u8) -> String {
    let step = ns_setup_step(step).unwrap_or("a bind of a device");
    format!("{}: the namespace rung keeps the image's /dev/{name}: {step} failed with errno ", names.product)
}

/// The line of a path that the child does not bind.
pub fn skipped_line(names: &Names, name: &str, why: &str) -> String {
    format!("{}: the namespace rung keeps the image's /dev/{name}: {why}\n", names.product)
}

/// The line where the image has no `/dev` directory (missing or a link).
pub fn no_dev_line(names: &Names) -> String {
    format!(
        "{}: the namespace rung binds no device of the host: the image holds no /dev directory (a link is not one)\n",
        names.product
    )
}

/// The host's node at `/dev/NAME`: a character device, and on a read-only
/// mount or not. `None`: not a character device.
fn host_node(path: &CBuf) -> Option<bool> {
    let st = sys::stat(path).ok()?;
    (kind_of(st.st_mode) == Kind::CharDevice)
        .then(|| sys::statfs(path).is_ok_and(|f| f.flag(sys::ST_RDONLY) == Some(true)))
}

/// Read the image through the descriptor of its root, and the host, and
/// build each bind and each line. Before the fork.
pub(crate) fn devices(root: &crate::RootDir, names: &Names) -> Devices {
    let lit = |s: &str| CBuf::new(s).expect("a literal");
    let dir = CBuf::new(&format!("{}/dev", root.path.trim_end_matches('/'))).unwrap_or_else(sys::cempty);
    let mut out = Devices { dir, empty: sys::cempty(), binds: Vec::new(), skipped: Vec::new(), ptmx: None };
    let flags = sys::O_PATH | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
    let Ok(dev) = sys::openat(root.fd(), &lit("dev"), flags, 0) else {
        out.skipped.push(no_dev_line(names).into_bytes());
        return out;
    };
    for name in DEVICES {
        let source = lit(&format!("/dev/{name}"));
        let host = host_node(&source);
        match plan_one(names, name, source, kind_at(dev, name), host, NS_DEV_BIND_STEP) {
            Ok(d) => out.binds.push(d),
            Err(line) => out.skipped.push(line),
        }
    }
    if let Some(pts) = crate::ns_optional::ns_pts_target(&root.path) {
        let kind = kind_at(dev, "ptmx");
        let source = lit(&crate::ns_pts::ptmx_of(&pts));
        let own = plan_one(names, "ptmx", source, kind, Some(false), crate::ns_pts::NS_PTMX_BIND_STEP);
        let source = lit("/dev/ptmx");
        let host = host_node(&source);
        let host = plan_one(names, "ptmx", source, kind, host, NS_DEV_BIND_STEP);
        out.ptmx = Some(Ptmx { own, host });
    }
    let _ = sys::close(dev);
    out
}

/// The kind of `name` in the directory `dev` of the image, not followed.
fn kind_at(dev: i64, name: &str) -> Kind {
    match sys::fstatat(dev, &CBuf::new(name).expect("a literal"), sys::AT_SYMLINK_NOFOLLOW) {
        Ok(st) => kind_of(st.st_mode),
        Err(e) if e == sys::ENOENT => Kind::Missing,
        Err(_) => Kind::Unreadable,
    }
}

/// The bind of `source` on `/dev/NAME`, or the line of no bind. `host`:
/// the source is a character device, on a read-only mount or not.
fn plan_one(
    names: &Names,
    name: &'static str,
    source: CBuf,
    kind: Kind,
    host: Option<bool>,
    bind_step: u8,
) -> Result<Device, Vec<u8>> {
    match choose(kind, host.is_some()) {
        Choice::Bind { create } => Ok(Device {
            label: name,
            source,
            name: CBuf::new(name).expect("a literal"),
            create,
            remount: remount_flags(host == Some(true)),
            point_refused: refused_start(names, name, NS_DEV_POINT_STEP).into_bytes(),
            bind_refused: refused_start(names, name, bind_step).into_bytes(),
            remount_refused: refused_start(names, name, NS_DEV_REMOUNT_STEP).into_bytes(),
        }),
        Choice::Skip(why) => Err(skipped_line(names, name, why).into_bytes()),
    }
}

/// `/proc/self/fd/FD` with its NUL in `buf`, with no allocation.
fn fd_path(fd: i64, buf: &mut [u8; 32]) -> u64 {
    const START: &[u8] = b"/proc/self/fd/";
    buf[..START.len()].copy_from_slice(START);
    let mut digits = [0u8; 10];
    let d = decimal(fd as u32, &mut digits);
    buf[START.len()..START.len() + d.len()].copy_from_slice(d);
    buf[START.len() + d.len()] = 0;
    buf.as_ptr() as u64
}

/// `mount(2)` onto the file of the descriptor `fd`: the magic link names
/// that file, and no link of the image redirects it.
pub(crate) fn mount_on_fd(source: &CBuf, fd: i64, flags: u64, empty: &CBuf) -> sys::Sysres {
    let mut buf = [0u8; 32];
    let target = fd_path(fd, &mut buf);
    unsafe { sys::sys(sys::SYS_MOUNT, [source.ptr(), target, empty.ptr(), flags, 0, 0]) }
}

/// The mount point at `name` in the directory `dev`: an `O_PATH` descriptor
/// of a regular file or of a character device, never of a link.
fn mount_point(dev: i64, d: &Device, empty: &CBuf) -> Result<i64, Errno> {
    if d.create {
        let flags = sys::O_CREAT | sys::O_EXCL | sys::O_NOFOLLOW | sys::O_WRONLY | sys::O_CLOEXEC;
        match sys::openat(dev, &d.name, flags, 0o644) {
            Ok(fd) => drop(sys::close(fd)),
            // A file that came after the check: the check below decides.
            Err(e) if e == sys::EEXIST => {}
            Err(e) => return Err(e),
        }
    }
    let fd = sys::openat(dev, &d.name, sys::O_PATH | sys::O_NOFOLLOW | sys::O_CLOEXEC, 0)?;
    let kind = sys::fstatat(fd, empty, sys::AT_EMPTY_PATH).map(|st| kind_of(st.st_mode));
    match kind.map(|k| choose(k, true)) {
        Ok(Choice::Bind { .. }) => Ok(fd),
        other => {
            let _ = sys::close(fd);
            Err(match (other, kind) {
                (Err(e), _) => e,
                (_, Ok(Kind::Link)) => sys::ELOOP,
                _ => sys::EINVAL,
            })
        }
    }
}

/// One bind and its remount. A refused remount removes the bind, so the
/// payload never holds a bind without the flags.
fn bind_one(dev: i64, d: &Device, empty: &CBuf) {
    let fd = match mount_point(dev, d, empty) {
        Ok(fd) => fd,
        Err(e) => return refused_line(&d.point_refused, e),
    };
    let bound = mount_on_fd(&d.source, fd, sys::MS_BIND, empty);
    let _ = sys::close(fd);
    if let Err(e) = bound {
        return refused_line(&d.bind_refused, e);
    }
    // A new open crosses into the bind: the remount needs its root.
    let top = sys::openat(dev, &d.name, sys::O_PATH | sys::O_NOFOLLOW | sys::O_CLOEXEC, 0);
    let remounted = top.and_then(|fd| {
        let r = mount_on_fd(empty, fd, d.remount, empty);
        if r.is_err() {
            let mut buf = [0u8; 32];
            let path = fd_path(fd, &mut buf);
            let _ = unsafe { sys::sys(sys::SYS_UMOUNT2, [path, sys::MNT_DETACH, 0, 0, 0, 0]) };
        }
        let _ = sys::close(fd);
        r
    });
    if let Err(e) = remounted {
        refused_line(&d.remount_refused, e);
    }
}

/// In the child, in a user namespace of the rung, after the optional
/// mounts: each line of a skipped path, then each bind. `own_pts`: the new
/// devpts instance is on `/dev/pts`, and its `ptmx` goes on `/dev/ptmx`.
/// No allocation.
pub(crate) fn bind_each(devices: &Devices, own_pts: bool) {
    for line in &devices.skipped {
        let _ = sys::write(2, line);
    }
    let ptmx = devices.ptmx.as_ref().map(|p| if own_pts { &p.own } else { &p.host });
    if let Some(Err(line)) = ptmx {
        let _ = sys::write(2, line);
    }
    let ptmx = ptmx.and_then(|p| p.as_ref().ok());
    if devices.binds.is_empty() && ptmx.is_none() {
        return;
    }
    let flags = sys::O_PATH | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
    let empty = &devices.empty;
    let dev = match sys::open(&devices.dir, flags, 0) {
        Ok(fd) => fd,
        Err(e) => {
            for d in devices.binds.iter().chain(ptmx) {
                refused_line(&d.point_refused, e);
            }
            return;
        }
    };
    // `/dev/ptmx` comes after the mount on `/dev/pts`: its source is there.
    for d in devices.binds.iter().chain(ptmx) {
        bind_one(dev, d, empty);
    }
    let _ = sys::close(dev);
}

#[cfg(test)]
mod tests;
