//! [`TODO/complete.md`](../../../TODO/complete.md) T-0401: device shims as
//! regular files.
//!
//! ⭐ **The trap this exists for is silent and it fills the disk.** `mknod` is
//! denied on the runtimes podvm targets, so an extracted rootfs has no
//! `/dev/null`. The first shell redirection into it then **creates a growing
//! regular file**, and the container fills its own filesystem while every
//! command in it reports success. The same capture is in the corpus, against
//! `ruri`: `references/Azathothas__container-research/tree/verification/README.md:126`.
//!
//! ⛔ **A regular file, never a fifo.** A fifo blocks a writer with no reader,
//! which turns `cmd >/dev/null` into a hang, and a hang is the failure
//! [`TODO/RULES.md`](../../../TODO/RULES.md) section 8 exists to prevent.
//!
//! ⭐ **`mknod` IS TRIED FIRST, on every run, and the shim is the fallback.**
//! Probed by calling it rather than inferred: this runtime hands a process every
//! capability bit and denies the operation anyway, so the capability says
//! nothing. On a machine that permits `mknod` the payload gets a real
//! `/dev/null` and the row reads `Native`; where the kernel refuses, the errno
//! it refused with is in the banner beside the shim. ⛔ A shim reported on a
//! machine that did not need one is a degradation podvm invented, which is the
//! same class of wrong as hiding one.
//!
//! ⛔ **Every shim is reported as a shim.** These are not devices:
//!
//! | path | the shim | what it is not |
//! | --- | --- | --- |
//! | `/dev/null` | an empty regular file, truncated on every run | writes are kept until the next run, not discarded |
//! | `/dev/zero` | [`FILLED_BYTES`] of `0x00` | it ends at EOF |
//! | `/dev/urandom` | [`FILLED_BYTES`] read once from the host's `getrandom(2)` | it ends at EOF, and re-reading gives the same bytes |
//! | `/dev/random` | the same | the same |
//!
//! ⚠ `/dev/random` is not named in T-0401, which names `null`, `zero` and
//! `urandom`. It is here because its shape and its failure are identical and
//! its absence is the same silent one; the entry records the addition.
//!
//! ⚠ `getrandom(2)` already covers TLS and crypto in every modern libc, so
//! `/dev/urandom` is a compatibility shim and not an entropy source. Anything
//! that needs entropy and reads this file instead of calling `getrandom` is
//! reading a file, and the banner says so.

use crate::write::{Kind, Root, Wrote};
use crate::{Action, Fixup, Report, Result};

/// How much `/dev/zero`, `/dev/urandom` and `/dev/random` hold.
///
/// ⚠ A number, so it is finite, and 1 MiB rather than more because it is paid
/// per extracted rootfs and a payload that reads more than this from a shim is
/// a payload the shim was never going to satisfy.
pub const FILLED_BYTES: usize = 1024 * 1024;

/// Asked for beyond the three shims themselves, so the completion layer does
/// not fill a rootfs to its last block and leave the payload nothing.
pub const SHIM_HEADROOM_BYTES: u64 = 4 * 1024 * 1024;

/// Is the right character device already at this path?
///
/// ⚠ The NUMBERS are checked, not just the type. A node that is there and is
/// `1:5` where `1:3` was wanted is a wrong answer rather than a present one,
/// and a payload that stats it reads those numbers.
fn already_right(k: &Kind, major: u64, minor: u64) -> bool {
    matches!(k, Kind::CharDev { major: a, minor: b } if *a == major && *b == minor)
}

/// Remove a symlink at a device path before the `mknod` attempt, which
/// then cannot follow it to a target outside the rootfs.
///
/// `Ok` names what was displaced (`a symlink to ...`); `Err` names why
/// the path is refused instead: asking `mknod` through an unremovable
/// link could create the node outside this rootfs, so the refusal is
/// loud rather than half-followed.
fn displace_link(root: &Root, path: &str, target: &str) -> std::result::Result<String, String> {
    if std::fs::remove_file(std::path::Path::new(root.path()).join(path)).is_err() {
        return Err(format!(
            "{path} is a symlink to {target} that podvm could not remove; \
             asking mknod(2) through it could create the node outside \
             this rootfs, and --strict refuses the run"
        ));
    }
    Ok(format!("a symlink to {target}"))
}

pub(crate) fn apply(root: &Root, r: &mut Report) {
    crate::record(r, "T-0401", "dev-shim", run(root));
}

/// The four devices, with the numbers the kernel assigns them.
///
/// ⚠ `major:minor` is a kernel constant and not a choice: `1:3` is `/dev/null`
/// on every Linux there is, and a payload that stats the node reads them.
const DEVICES: &[(&str, u64, u64)] = &[
    ("dev/null", 1, 3),
    ("dev/zero", 1, 5),
    ("dev/random", 1, 8),
    ("dev/urandom", 1, 9),
];

fn run(root: &Root) -> Result<Vec<Fixup>> {
    let mut out = Vec::new();
    root.mkdirs("dev")?;

    // ⚠ Drawn ONCE and used for both random shims, which is honest: they are
    // two names for one file of bytes, and drawing twice would imply a property
    // neither has. ⛔ Not drawn at all unless a shim is actually needed: a
    // megabyte of entropy per container start is a cost, and on a machine that
    // permits `mknod` there is nothing to fill.
    let mut filler: Option<(Vec<u8>, Vec<u8>)> = None;

    for (path, major, minor) in DEVICES {
        let k = root.kind(path)?;

        // 1. the right node is already there -- the image shipped one, or an
        // earlier run of this fixup made it. ⛔ Not degraded: it is the real
        // thing, whoever put it there.
        if already_right(&k, *major, *minor) {
            out.push(
                Fixup::new("T-0401", "dev-node", path, Action::Unchanged).why(format!(
                    "a real character device {major}:{minor} is already here"
                )),
            );
            continue;
        }

        // 1b. T-0415: name what is there before touching it. A directory
        // is refused with its path named; anything else is replaced by
        // the shim below, which says what it replaced. A symlink is
        // removed before the `mknod` attempt, which then cannot follow it:
        // on a machine that permits `mknod` the call would otherwise
        // create the node at the link's target, inside this rootfs or
        // outside it. Where the link cannot be removed the path is
        // refused the same way a directory is, loudly instead of
        // half-followed.
        let displaced = match &k {
            Kind::Missing | Kind::Regular { .. } => None,
            Kind::Dir => {
                out.push(
                    Fixup::new("T-0401", "dev-shim", path, Action::Failed)
                        .why(format!(
                            "{path} is a directory in this image; podvm will not replace a \
                             directory with a file, and --strict refuses the run"
                        ))
                        .degraded(),
                );
                continue;
            }
            Kind::Symlink(t) => match displace_link(root, path, t) {
                Ok(displaced) => Some(displaced),
                Err(why) => {
                    out.push(
                        Fixup::new("T-0401", "dev-shim", path, Action::Failed)
                            .why(why)
                            .degraded(),
                    );
                    continue;
                }
            },
            Kind::CharDev { major: a, minor: b } => Some(format!("a character device {a}:{b}")),
            Kind::Other(m) => Some(match m & 0o170000 {
                // File-type bits, UAPI `linux/stat.h`.
                0o060000 => "a block device".into(),
                0o140000 => "a socket".into(),
                0o10000 => "a fifo".into(),
                _ => format!("a file of type {m:o}"),
            }),
        };

        // 2. ⭐ ASK THE KERNEL. On a machine that permits `mknod` the payload
        // gets the real thing and this is not a degradation at all.
        match root.mknod_char(path, 0o666, *major, *minor)? {
            Ok(()) => {
                // T-0415 and T-1686: the row names what the node replaced on
                // this path too, as the shim's does. A removed symlink leaves
                // the path empty, so the node is created; anything else at the
                // path is rewritten.
                let mut detail = format!(
                    "a real character device {major}:{minor}. This machine permits \
                     mknod(2), so podvm made the node rather than a shim"
                );
                if let Some(s) = &displaced {
                    detail.push_str(&format!(". It replaced {s}."));
                }
                let action = match (&displaced, &k) {
                    (Some(_), Kind::Symlink(_)) | (None, _) => Action::Created,
                    (Some(_), _) => Action::Rewrote,
                };
                out.push(Fixup::new("T-0401", "dev-node", path, action).why(detail));
                continue;
            }
            Err(why) => {
                // 3. the wall this entry exists for. Fall through to the shim,
                // carrying the errno the kernel refused with.
                // ⛔ RULES.md section 8 and T-0806: `statvfs` before a large
                // write, and the error names the destination. Three shims of
                // one MiB each into a rootfs on a small tmpfs is exactly the
                // write that fills it, and the failure would otherwise arrive
                // as a truncated `/dev/zero` nothing reports.
                if filler.is_none() {
                    podvm_image::space::require(
                        root.path(),
                        "the /dev shims T-0401 falls back to",
                        (FILLED_BYTES * 3) as u64,
                        4,
                        SHIM_HEADROOM_BYTES,
                    )
                    .map_err(|e| crate::Error::Complete(format!("{e}")))?;
                }
                let (zeros, rand) = filler.get_or_insert_with(|| {
                    (
                        vec![0u8; FILLED_BYTES],
                        random_bytes(FILLED_BYTES).unwrap_or_else(|_| vec![0u8; FILLED_BYTES]),
                    )
                });
                out.extend(shim_for(
                    root,
                    path,
                    &why,
                    displaced.as_deref(),
                    zeros,
                    rand,
                )?);
            }
        }
    }
    // T-0413: the `/dev` spellings the emulation resolves through, staged
    // where absent. The `dev/` directory stands from `mkdirs` above, and a
    // link needs no privilege, so this holds on every machine the shims do.
    fd_links(root, &mut out)?;
    Ok(out)
}

/// The `/dev` spellings the T-0413 emulation resolves through.
///
/// A minimal image ships no `/dev/fd` and no standard stream links (the
/// pinned debian row holds the four shims and nothing else, measured
/// 2026-09-26), and past a link the image does not hold the emulation
/// refuses rather than invents. So the completion layer stages the
/// conventional links where they are absent: they need no privilege, they
/// are what a populated `/dev` holds, and an image's own link is never
/// retargeted. Anything else at the path refuses the way a directory at a
/// device path does.
const FD_LINKS: &[(&str, &str)] = &[
    ("dev/stdin", "/proc/self/fd/0"),
    ("dev/stdout", "/proc/self/fd/1"),
    ("dev/stderr", "/proc/self/fd/2"),
    ("dev/fd", "/proc/self/fd"),
];

fn fd_links(root: &Root, out: &mut Vec<Fixup>) -> Result<()> {
    for (path, target) in FD_LINKS {
        match root.kind(path)? {
            // The conventional link is already here: whoever put it here,
            // it is the real thing. ⛔ Not degraded.
            Kind::Symlink(t) if t == *target => {
                out.push(
                    Fixup::new("T-0413", "dev-fd-link", path, Action::Unchanged)
                        .why(format!("a symlink to {target} is already here")),
                );
            }
            // The image's own link points elsewhere: its business, never
            // retargeted. The emulation resolves through what is there.
            Kind::Symlink(t) => {
                out.push(
                    Fixup::new("T-0413", "dev-fd-link", path, Action::Unchanged).why(format!(
                        "a symlink to {t} is already here; podvm keeps the image's own link"
                    )),
                );
            }
            Kind::Missing => {
                let at = std::path::Path::new(root.path()).join(path);
                match std::os::unix::fs::symlink(target, &at) {
                    // The conventional content, needing no privilege: what
                    // a populated `/dev` holds. ⛔ Not degraded.
                    Ok(()) => out.push(
                        Fixup::new("T-0413", "dev-fd-link", path, Action::Created).why(format!(
                            "a symlink to {target}: the T-0413 emulation resolves this \
                             spelling through it"
                        )),
                    ),
                    Err(e) => out.push(
                        Fixup::new("T-0413", "dev-fd-link", path, Action::Failed)
                            .why(format!("{path} could not link to {target}: {e}"))
                            .degraded(),
                    ),
                }
            }
            // Anything else at the path stays: podvm will not replace a
            // directory or a file with a link, and --strict refuses the run.
            Kind::Dir => {
                out.push(
                    Fixup::new("T-0413", "dev-fd-link", path, Action::Failed)
                        .why(format!(
                            "{path} is a directory in this image; podvm will not replace a \
                             directory with a link, and --strict refuses the run"
                        ))
                        .degraded(),
                );
            }
            Kind::Regular { .. } => {
                out.push(
                    Fixup::new("T-0413", "dev-fd-link", path, Action::Failed)
                        .why(format!(
                            "{path} is a regular file in this image; podvm will not replace \
                             a file with a link, and --strict refuses the run"
                        ))
                        .degraded(),
                );
            }
            Kind::CharDev { major: a, minor: b } => {
                out.push(
                    Fixup::new("T-0413", "dev-fd-link", path, Action::Failed)
                        .why(format!(
                            "{path} is a character device {a}:{b} in this image; podvm keeps \
                             the image's own node, and --strict refuses the run"
                        ))
                        .degraded(),
                );
            }
            Kind::Other(m) => {
                out.push(
                    Fixup::new("T-0413", "dev-fd-link", path, Action::Failed)
                        .why(format!(
                            "{path} is a file of type {m:o} in this image; podvm will not \
                             replace it with a link, and --strict refuses the run"
                        ))
                        .degraded(),
                );
            }
        }
    }
    Ok(())
}

/// The regular-file shim for one device, once `mknod` has refused.
fn shim_for(
    root: &Root,
    path: &str,
    why: &str,
    displaced: Option<&str>,
    zeros: &[u8],
    rand: &[u8],
) -> Result<Vec<Fixup>> {
    if path == "dev/null" {
        // ⭐ Truncated on EVERY run, not created once. The failure this fixup
        // is for leaves a multi-gigabyte regular file behind, and a rootfs is
        // shared between containers, so the second container inherits the
        // first one's spill unless this runs every time.
        let grew = match root.kind(path)? {
            Kind::Regular { len, .. } if len > 0 => Some(len),
            _ => None,
        };
        let w = root.write(path, b"", 0o666)?;
        let mut detail = match grew {
            Some(len) => format!(
                "mknod(2) refused with {why}, so this is a regular file, and it was \
                 {len} bytes: a payload had already redirected into it. Writes are \
                 kept until the next run, not discarded"
            ),
            None => format!(
                "mknod(2) refused with {why}, so this is a regular file: writes are \
                 kept until the next run rather than discarded, and reads give EOF"
            ),
        };
        // T-0415: what the shim replaced, where it replaced anything. A
        // reader auditing the rootfs can tell a deliberate stand-in from a
        // fresh one without re-running the fixup.
        if let Some(s) = displaced {
            detail.push_str(&format!(" It replaced {s}."));
        }
        return Ok(vec![shim(path, w, detail)]);
    }
    let (bytes, what) = if path == "dev/zero" {
        (zeros, "zero bytes")
    } else {
        (rand, "bytes read once from the host's getrandom(2)")
    };
    filled(root, path, bytes, what, why, displaced)
}

fn filled(
    root: &Root,
    path: &str,
    bytes: &[u8],
    what: &str,
    why: &str,
    displaced: Option<&str>,
) -> Result<Vec<Fixup>> {
    // ⚠ A shim already the right length is left alone: rewriting a megabyte on
    // every container start is a cost with no reading behind it.
    if let Kind::Regular { len, .. } = root.kind(path)?
        && len as usize == bytes.len()
    {
        return Ok(vec![Fixup::new(
            "T-0401",
            "dev-shim",
            path,
            Action::Unchanged,
        )
        .why(format!("already a {} byte shim", bytes.len()))
        .degraded()]);
    }
    let w = root.write(path, bytes, 0o666)?;
    let mut detail = format!(
        "mknod(2) refused with {why}, so this is a regular file of {} {what}. \
         refused: It ENDS: a read past {} bytes gives EOF where the device would not",
        bytes.len(),
        bytes.len()
    );
    // T-0415, as above: what the shim replaced.
    if let Some(s) = displaced {
        detail.push_str(&format!(" It replaced {s}."));
    }
    Ok(vec![shim(path, w, detail)])
}

fn shim(path: &str, w: Wrote, detail: impl Into<String>) -> Fixup {
    Fixup::new("T-0401", "dev-shim", path, crate::act(w))
        .why(detail)
        .degraded()
}

/// `getrandom(2)`, in bounded chunks.
///
/// ⛔ **Never `std::fs::read("/dev/urandom")`.** That call has no EOF: it
/// allocated 13 GB here before the OOM killer took the process
/// ([`TODO/supervise.md`](../../../TODO/supervise.md) T-0602). The shim for a
/// device is not built by reading the device.
fn random_bytes(n: usize) -> Result<Vec<u8>> {
    let mut out = vec![0u8; n];
    let mut off = 0;
    while off < n {
        // ⚠ The kernel caps one getrandom at 32 MiB and can return short; the
        // loop is the contract, not a precaution.
        match podvm_probe::sys::getrandom(&mut out[off..]) {
            Ok(0) => break,
            Ok(k) => off += k as usize,
            Err(e) if e == podvm_probe::sys::EINTR => continue,
            Err(e) => {
                return Err(crate::Error::Complete(format!(
                    "getrandom(2) for the /dev/urandom shim: {}",
                    e.name()
                )))
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
