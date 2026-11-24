//! The leg of a delegated cgroup2 subtree: can this process, with no root,
//! make a child cgroup and set its limits?
//!
//! A caller (podvm's `run --memory`, `--cpus`, `--pids-limit`) makes one
//! child cgroup for each run in the delegated directory, writes
//! `memory.max`, `cpu.max` and `pids.max`, and moves the payload into it.
//! This module finds that directory and says why there is none. It writes
//! no limit.
//!
//! The test of a directory is a real write, never `access(W_OK)`: a
//! `mkdir` and `rmdir` of a child cgroup, and an open of
//! `cgroup.subtree_control` for write. `access` reads the capability set,
//! which a user namespace fills with each bit.

use std::path::{Path, PathBuf};

use crate::sys::{self, CBuf, Errno};

/// The mount point of the unified hierarchy.
pub const ROOT: &str = "/sys/fs/cgroup";

/// The controllers that the limits need: `memory.max`, `cpu.max`,
/// `pids.max`.
pub const WANTED: &[&str] = &["memory", "cpu", "pids"];

/// `CGROUP2_SUPER_MAGIC` of `include/uapi/linux/magic.h`.
pub const CGROUP2_SUPER_MAGIC: u64 = linux_raw_sys::general::CGROUP2_SUPER_MAGIC as u64;

/// `CGROUP_SUPER_MAGIC` of `include/uapi/linux/magic.h`: a hierarchy of
/// cgroup v1.
pub const CGROUP_SUPER_MAGIC: u64 = linux_raw_sys::general::CGROUP_SUPER_MAGIC as u64;

/// What the leg found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leg {
    /// The cgroup of the process (the line `0::PATH` of
    /// `/proc/self/cgroup`), where it has one.
    pub own: Option<String>,
    pub verdict: Delegation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delegation {
    /// The nearest directory at or above the cgroup of the process that
    /// the user can write, with each controller of [`WANTED`].
    Delegated {
        path: PathBuf,
        /// The controllers that `cgroup.controllers` offers there.
        controllers: Vec<String>,
        /// The controllers that `cgroup.subtree_control` enables there for
        /// its children already.
        enabled: Vec<String>,
    },
    /// No delegation, and why.
    None(Reason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// `statfs(2)` of [`ROOT`] failed.
    NoMount(Errno),
    /// [`ROOT`] is not cgroup2 (cgroup v1, or a hybrid with a tmpfs there).
    NotCgroup2 { f_type: u64 },
    /// `/proc/self/cgroup` has no line `0::PATH`, or the path is not a
    /// clean absolute path.
    NoMembership,
    /// No directory at or above the cgroup of the process passed the write
    /// test. The errno is the one of the cgroup of the process.
    NotWritable { cgroup: PathBuf, errno: Errno },
    /// The nearest writable directory lacks a controller of [`WANTED`].
    MissingControllers { path: PathBuf, offered: Vec<String>, missing: Vec<String> },
}

impl Delegation {
    pub fn is_delegated(&self) -> bool {
        matches!(self, Delegation::Delegated { .. })
    }

    /// One line for a report or an error.
    pub fn words(&self) -> String {
        match self {
            Delegation::Delegated { path, controllers, .. } => {
                format!("delegated: {} ({})", path.display(), controllers.join(" "))
            }
            Delegation::None(Reason::NoMount(e)) => format!("none: statfs of {ROOT}: {}", e.name()),
            Delegation::None(Reason::NotCgroup2 { f_type }) => {
                format!("none: {ROOT} is not cgroup2 (f_type {f_type:#x}; cgroup v1 or a hybrid)")
            }
            Delegation::None(Reason::NoMembership) => "none: /proc/self/cgroup has no line 0::PATH".to_string(),
            Delegation::None(Reason::NotWritable { cgroup, errno }) => format!(
                "none: no directory at or above {} can be written by this user ({})",
                cgroup.display(),
                errno.name()
            ),
            Delegation::None(Reason::MissingControllers { path, missing, .. }) => {
                format!("none: {} has no controller {}", path.display(), missing.join(", "))
            }
        }
    }
}

/// Run the leg on this host.
pub fn probe() -> Leg {
    let magic = CBuf::new(ROOT).ok_or(sys::EINVAL).and_then(|p| sys::statfs(&p)).map(|s| s.f_type);
    let membership = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    assess(magic, &membership, Path::new(ROOT), &write_test)
}

/// The leg from its inputs: the `f_type` of the mount (or the errno of
/// `statfs`), the text of `/proc/self/cgroup`, the root of the hierarchy,
/// and the write test of a directory. A test gives a made-up tree.
pub fn assess(
    magic: Result<u64, Errno>,
    membership: &str,
    root: &Path,
    writable: &dyn Fn(&Path) -> Result<(), Errno>,
) -> Leg {
    let own = own_cgroup(membership);
    let verdict = match magic {
        Err(e) => Delegation::None(Reason::NoMount(e)),
        Ok(f_type) if f_type != CGROUP2_SUPER_MAGIC => Delegation::None(Reason::NotCgroup2 { f_type }),
        Ok(_) => match own.as_deref() {
            None => Delegation::None(Reason::NoMembership),
            Some(own) => nearest(root, own, writable),
        },
    };
    Leg { own, verdict }
}

/// The path of the line `0::PATH`, when it is absolute and has no `.` or
/// `..` component.
fn own_cgroup(membership: &str) -> Option<String> {
    let path = membership.lines().find_map(|l| l.strip_prefix("0::"))?;
    let clean = path.starts_with('/') && path.split('/').all(|c| c != "." && c != "..");
    clean.then(|| path.to_string())
}

fn nearest(root: &Path, own: &str, writable: &dyn Fn(&Path) -> Result<(), Errno>) -> Delegation {
    let cgroup = root.join(own.trim_start_matches('/'));
    let mut first = None;
    for dir in cgroup.ancestors().take_while(|d| d.starts_with(root)) {
        match writable(dir) {
            Ok(()) => return controllers_at(dir),
            Err(e) => {
                first.get_or_insert(e);
            }
        }
    }
    Delegation::None(Reason::NotWritable { cgroup, errno: first.unwrap_or(sys::ENOENT) })
}

fn controllers_at(dir: &Path) -> Delegation {
    let words = |name: &str| -> Vec<String> {
        let text = std::fs::read_to_string(dir.join(name)).unwrap_or_default();
        text.split_whitespace().map(str::to_string).collect()
    };
    let offered = words("cgroup.controllers");
    let missing: Vec<String> =
        WANTED.iter().filter(|w| !offered.iter().any(|o| o == *w)).map(|w| w.to_string()).collect();
    if !missing.is_empty() {
        return Delegation::None(Reason::MissingControllers { path: dir.to_path_buf(), offered, missing });
    }
    Delegation::Delegated { path: dir.to_path_buf(), controllers: offered, enabled: words("cgroup.subtree_control") }
}

/// The write test of a directory: a child cgroup made and removed, and
/// `cgroup.subtree_control` opened for write. Nothing is written into the
/// file.
fn write_test(dir: &Path) -> Result<(), Errno> {
    let path = |p: &Path| p.to_str().and_then(CBuf::new).ok_or(sys::EINVAL);
    let child = dir.join(format!("pod-hostprobe-{}", std::process::id()));
    let child = path(&child)?;
    sys::mkdir(&child, 0o755)?;
    sys::unlinkat(sys::AT_FDCWD as i64, &child, sys::AT_REMOVEDIR)?;
    let fd = sys::open(&path(&dir.join("cgroup.subtree_control"))?, sys::O_WRONLY | sys::O_CLOEXEC, 0)?;
    let _ = sys::close(fd);
    Ok(())
}

#[cfg(test)]
mod tests;
