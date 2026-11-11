//! The confinement of a payload (`Plan.confine`, `pod-confine`): the
//! compile before the fork, the refusal that the child writes to the
//! readiness pipe, and its sentence in the parent.

use pod_confine::{Compiled, Refusal};

use crate::{Error, Plan, Result, RootDir, errno_class};

/// The step byte of a refusal of `pod_confine::apply`. The five bytes of
/// the refusal follow it.
pub(crate) const STEP: u8 = 15;

/// The compiled confinement of `plan`, or `None` where it has none.
///
/// `root` is the root file system of a rung with a chroot: the paths of
/// the spec are guest paths, and each resolves under the root descriptor
/// as the kernel resolves it after the chroot (`RESOLVE_IN_ROOT`). The
/// rule holds the inode, so the open before the fork names the file that
/// the payload sees. `None` is the entry with no chroot: host paths.
///
/// A host with no Landlock, or a path that does not exist, is a refusal
/// by name here, before the banner and the fork.
pub(crate) fn compile(plan: &Plan, root: Option<&RootDir>) -> Result<Option<Compiled>> {
    let Some(spec) = &plan.confine else { return Ok(None) };
    let abi = pod_confine::check()?;
    let mut compiled = match root {
        Some(r) => spec.compile_in(abi, r.fd())?,
        None => spec.compile(abi)?,
    };
    // The child duplicates each passed descriptor onto its number before
    // the confinement: a rule on one of those numbers would lose its file.
    let taken: Vec<i64> = plan.fds.pass.iter().map(|&(child, _)| child).collect();
    compiled.keep_clear_of(&taken)?;
    Ok(Some(compiled))
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
