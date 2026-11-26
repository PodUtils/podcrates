//! The refusal of a working directory that the image does not hold.

use pod_core::Class;

use crate::abi::{GuestRoot, ResolveKind};
use crate::entry::workdir_kind;
use crate::{Error, Result};

/// Whether docker makes a working directory that is missing. The words of
/// the refusal say "docker creates it" only where it is true.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkdirMade {
    /// `docker run -w`: docker makes the directory, and the entry refuses
    /// it instead.
    ByDocker,
    /// `docker exec -w`: docker fails on a missing directory, as the entry
    /// does.
    NotMade,
}

/// The refusal of a working directory that does not resolve.
///
/// `context` names what the caller asked for beside the path: the image
/// where the caller knows it (`for image "alpine"`), the root file system
/// where only it is known. One builder, so the check before the fork and
/// [`check_workdir`] say the same.
///
/// `volume` is the volume (`HOST:GUEST`) that holds the path, where the
/// caller knows it: a missing directory then names the volume, not the
/// image.
///
/// `made` is `Some` where the caller said whether docker makes a missing
/// directory. A check in the plan knows no caller, so it says `None` and
/// gives no clause.
pub(crate) fn refuse_workdir(
    workdir: &str,
    context: &str,
    kind: ResolveKind,
    volume: Option<&str>,
    made: Option<WorkdirMade>,
) -> Error {
    let (context, why) = match (kind, volume) {
        (ResolveKind::Absent, Some(v)) => (format!("in the volume {v}"), "is not a directory in the volume"),
        (ResolveKind::Absent, None) => (context.to_string(), "is not a directory in the image"),
        (ResolveKind::Escapes, _) => (context.to_string(), "escapes the image"),
        (ResolveKind::Loop, _) => (context.to_string(), "has too many levels of symlinks"),
    };
    // Only a missing directory is one that docker could make.
    let clause = match (kind, made) {
        (ResolveKind::Absent, Some(WorkdirMade::ByDocker)) => ": docker creates it, and the entry refuses it instead",
        _ => "",
    };
    Error::runtime(Class::NotFound, format!("the working directory {workdir:?} {context} {why}{clause}"))
}

/// Check the working directory before the fork, for `docker run`.
///
/// Same as [`check_workdir_as`] with [`WorkdirMade::ByDocker`].
pub fn check_workdir<G: GuestRoot + ?Sized>(root: &G, workdir: &str, image: &str) -> Result<()> {
    check_workdir_as(root, workdir, image, WorkdirMade::ByDocker)
}

/// Check the working directory before the fork.
///
/// Resolve `workdir` under `root`: the root file system, or the table of a
/// run with no chroot ([`crate::userland::Maps`]), where a working
/// directory in a volume is the directory of the volume. Refuse, with the
/// path and the image (or, for a directory inside a volume, the volume as
/// `HOST:GUEST`), a directory that is missing, escapes, loops or is a file.
/// A missing directory says "docker creates it, and the entry refuses it
/// instead" where `made` is [`WorkdirMade::ByDocker`], and does not say it
/// for [`WorkdirMade::NotMade`] (`docker exec`). Empty and `/` always pass;
/// `/tmp` passes where the image holds it.
///
/// Call it before a change of the root file system. The entry checks again
/// with no image name and no clause, and the child reports a directory
/// that went missing after the check as its own step.
pub fn check_workdir_as<G: GuestRoot + ?Sized>(root: &G, workdir: &str, image: &str, made: WorkdirMade) -> Result<()> {
    if workdir.contains('\0') {
        return Err(Error::runtime(Class::Usage, "the working directory contains a NUL byte"));
    }
    workdir_kind(root, workdir).map_err(|kind| {
        refuse_workdir(
            workdir,
            &format!("for image {image:?}"),
            kind,
            root.volume_label(workdir).as_deref(),
            Some(made),
        )
    })
}
