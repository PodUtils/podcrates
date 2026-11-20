//! The confinement of a payload (`Plan.confine`, `pod-confine`): the
//! compile before the fork, the refusal that the child writes to the
//! readiness pipe, and its sentence in the parent; the exec and the write
//! table of the entry with no chroot, and the check of each program that
//! the kernel does not see ([`check_exec`], [`check_exec_argv`],
//! [`check_exec_guest`]).

use std::os::unix::ffi::OsStrExt;

use pod_confine::{Compiled, Refusal};
use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::abi::{GuestRoot, ResolveKind};
use crate::ns_upper::View;
use crate::{Error, Plan, Result, RootDir, errno_class};

mod program;

pub use program::{check_exec_argv, check_exec_guest};

/// The step byte of a refusal of `pod_confine::apply` or of
/// `Compiled::open_paths`. The five bytes of the refusal follow it.
pub(crate) const STEP: u8 = 15;

/// The compiled confinement of `plan`, or `None` where it has none.
///
/// `root` is the root file system of a rung with a chroot: the paths of
/// the spec are guest paths, and each resolves under the root descriptor
/// as the kernel resolves it after the chroot (`RESOLVE_IN_ROOT`). `None`
/// is the entry with no chroot: host paths. Where the plan has volumes, or
/// the root is an overlay with the upper directory `upper`, each path
/// resolves through them ([`open_through`], [`View`]): a path in a volume
/// is a path of its host side, as the child binds it, and each other path
/// is the entry that the upper layer shows over the lower one.
///
/// A host with no Landlock, or a path that does not exist, is a refusal
/// by name here, before the banner and the fork. Then the descriptors of
/// the rules close: the child opens each path again after its mounts and
/// its root change (`Compiled::open_paths`). Landlock does not read a rule
/// on a directory under a mount point, so a descriptor of the compile does
/// not cover the tmpfs that the namespace rung mounts on `/tmp`. No
/// descriptor of a rule crosses the fork, so a passed descriptor takes the
/// number of no rule.
pub(crate) fn compile(
    plan: &Plan,
    root: Option<&RootDir>,
    upper: Option<crate::ns_upper::UpperDir<'_>>,
) -> Result<Option<Compiled>> {
    let Some(spec) = &plan.confine else { return Ok(None) };
    let abi = pod_confine::check()?;
    let mut compiled = match root {
        Some(r) if !plan.volumes.is_empty() || upper.is_some() => {
            let view = View::new(r, plan, upper);
            spec.compile_by(abi, |path| open_through(&view, r, path))?
        }
        Some(r) => spec.compile_in(abi, r.fd())?,
        None => spec.compile(abi)?,
    };
    compiled.close_paths();
    Ok(Some(compiled))
}

/// The `O_PATH` descriptor of the guest path `guest` of a rule, through
/// the volumes of a plan and the upper layer of an overlay root (`view`):
/// a path at or under the guest path of a volume opens in its host side,
/// each other path in the layer that shows it (the image where there is no
/// upper layer); a path that the upper layer removes is missing. The walk is the walk of the working directory: a link of
/// the image resolves in the guest view, and a link to a volume lands in
/// the volume. Where the image does not hold a directory above
/// the guest path of a volume, the child makes it: the compile gets the
/// root of the image, a directory, for its check of the type, and the
/// child opens the made directory. A path in neither the image nor a
/// volume gives `ENOENT`, which the compile refuses by name.
pub(crate) fn open_through<G: GuestRoot>(view: &G, root: &RootDir, guest: &str) -> std::result::Result<i64, Errno> {
    let (host, parts) = crate::abi::walk_in(view, guest).map_err(|k| match k {
        ResolveKind::Absent => sys::ENOENT,
        ResolveKind::Loop => sys::ELOOP,
        ResolveKind::Escapes => sys::EXDEV,
    })?;
    let path = CBuf::from_bytes(host.as_os_str().as_bytes()).ok_or(sys::EINVAL)?;
    match sys::open(&path, sys::O_PATH | sys::O_CLOEXEC, 0) {
        Err(e) if e == sys::ENOENT && view.above_mount(&parts) => sys::fcntl(root.fd(), sys::F_DUPFD_CLOEXEC, 0),
        r => r,
    }
}

/// The entries `NAME=TABLE` of the exec table (`names.exec_table`) and of
/// the write table (`names.write_table`) of `plan` for the payload of the
/// entry with no chroot, or none with no confinement or on a rung with a
/// chroot. There the spec has host paths, and each one resolves as
/// Landlock opens it.
pub(crate) fn table_vars(plan: &Plan, userland: bool) -> Result<Vec<String>> {
    match &plan.confine {
        Some(spec) if userland => Ok(vec![
            format!("{}={}", plan.names.exec_table, spec.exec_table().resolved().encode()?),
            format!("{}={}", plan.names.write_table, spec.table(pod_confine::Access::WRITE).resolved().encode()?),
        ]),
        _ => Ok(Vec::new()),
    }
}

/// Whether the plan's confinement lets the program at the host path
/// `program` run, for a caller of the entry with no chroot. There the
/// kernel executes the loader of the image and only maps the program, and
/// runs the interpreter of a script with the script as an argument:
/// Landlock checks `EXECUTE` on the loader and the interpreter alone. The
/// caller checks the payload (for a script: the script and its
/// interpreter) before [`crate::spawn_userland`], and before it stages a
/// static payload into a memfd. `Ok` with no confinement. A program
/// outside each execute path is [`Error::CannotInvoke`] (126, as `EACCES`
/// of the kernel), and a program that does not resolve is
/// [`Error::NotFound`] (127). [`check_exec_argv`] and [`check_exec_guest`]
/// check each file of an argv: the `#!` lines and the program of a
/// dynamic loader too.
pub fn check_exec(plan: &Plan, program: &str) -> Result<()> {
    let Some(spec) = &plan.confine else { return Ok(()) };
    let real = std::fs::canonicalize(program).map_err(|e| Error::NotFound(format!("{program}: {e}")))?;
    let real = real.to_string_lossy();
    if spec.exec_table().resolved().allows(&real) {
        return Ok(());
    }
    Err(Error::CannotInvoke(format!(
        "{program}: the confinement allows no exec of {real}: it is outside each execute path (EACCES)"
    )))
}

/// The message of the child for a refusal: [`STEP`], then the refusal.
/// No allocation: the child writes it between its fork and its exec.
pub(crate) fn message(r: Refusal) -> [u8; 6] {
    let b = r.to_bytes();
    [STEP, b[0], b[1], b[2], b[3], b[4]]
}

/// The error of a refusal message: the call, the path or the port of its
/// rule, and the errno.
pub(crate) fn failure(compiled: Option<&Compiled>, msg: &[u8; 6]) -> Error {
    let refusal = Refusal::from_bytes([msg[1], msg[2], msg[3], msg[4], msg[5]]);
    match (refusal, compiled) {
        (Some(r), Some(c)) => Error::runtime(
            errno_class(r.errno),
            format!("the payload could not be started: the confinement failed at {}", r.describe(c)),
        ),
        _ => Error::runtime(
            pod_core::Class::Unavailable,
            format!(
                "the payload could not be started: the child reported a confinement step that names no call {msg:?}"
            ),
        ),
    }
}

#[cfg(test)]
mod tests;
