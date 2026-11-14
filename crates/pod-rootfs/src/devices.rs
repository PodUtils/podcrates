//! The device nodes of `/dev`, and regular files in their place where the
//! kernel refuses `mknod`.
//!
//! A root with no `/dev/null` fills its disk with no word: the first shell
//! redirect into it **makes a regular file that grows**, and each command
//! reports success.
//!
//! **A regular file, never a fifo.** A fifo stops a writer with no reader,
//! and `cmd >/dev/null` then waits for ever.
//!
//! **`mknod` first, on each run; the shim is the fallback.** The crate calls
//! it and does not read a capability bit: a runtime can give each capability
//! and still refuse the call. On a host that permits `mknod` the payload gets
//! a real `/dev/null` and the row is `dev-node`, not degraded; where the
//! kernel refuses, the row of the shim names the errno.
//!
//! Each shim is a degraded row:
//!
//! | path | the shim | what it is not |
//! | --- | --- | --- |
//! | `/dev/null` | an empty regular file, cut to zero on each run | a write stays until the next run |
//! | `/dev/zero` | [`FILLED_BYTES`] of `0x00` | it ends |
//! | `/dev/urandom` | [`FILLED_BYTES`] read one time from `getrandom(2)` of the host | it ends, and each read gives the same bytes |
//! | `/dev/random` | the same | the same |
//!
//! `getrandom(2)` serves TLS and crypto in each current C library, so the
//! `/dev/urandom` shim is for compatibility and is not a source of entropy.
//!
//! Each operation goes through [`Root`]: a `dev` that is a symlink out of
//! the root resolves inside it, so no unlink and no symlink of this module
//! lands on the host.

use crate::write::{Kind, Root, Wrote};
use crate::{Action, Fixup, Report, Result, Settings};

/// What `/dev/zero`, `/dev/urandom` and `/dev/random` hold. Each root pays
/// for it, and a payload that reads more than this from a shim does not get
/// what it needs from a shim.
pub const FILLED_BYTES: usize = 1024 * 1024;

/// The space asked for beyond the shims, so that the completion does not
/// fill a root to its last block.
pub const SHIM_HEADROOM_BYTES: u64 = 4 * 1024 * 1024;

/// The four devices, with the numbers that the kernel gives them. A payload
/// that stats a node reads them.
pub const DEVICES: &[(&str, u64, u64)] =
    &[("dev/null", 1, 3), ("dev/zero", 1, 5), ("dev/random", 1, 8), ("dev/urandom", 1, 9)];

/// The `/dev` spellings of the standard streams and of `/dev/fd`. A minimal
/// image ships none of them. They need no privilege, they are what a full
/// `/dev` holds, and the crate never changes a link of the image.
pub const FD_LINKS: &[(&str, &str)] = &[
    ("dev/stdin", "/proc/self/fd/0"),
    ("dev/stdout", "/proc/self/fd/1"),
    ("dev/stderr", "/proc/self/fd/2"),
    ("dev/fd", "/proc/self/fd"),
];

pub(crate) fn apply(root: &Root, settings: &Settings, r: &mut Report) {
    crate::record(r, "dev-shim", run(root, settings));
}

/// Is the right character device at this path, and does it open?
fn already_right(root: &Root, path: &str, k: &Kind, major: u64, minor: u64) -> Result<bool> {
    Ok(matches!(k, Kind::CharDev { major: a, minor: b } if *a == major && *b == minor) && root.char_opens(path)?)
}

/// Remove a symlink at a device path before the `mknod`, which then cannot
/// follow it. `Ok` names what went (`a symlink to ...`); `Err` names why the
/// path is refused: a `mknod` through a link that stays can make the node
/// at its target.
fn displace_link(root: &Root, path: &str, target: &str) -> std::result::Result<String, String> {
    let refused = |why: String| {
        format!(
            "{path} is a symlink to {target} that the crate could not remove ({why}); a mknod(2) through it can \
             make the node at its target, so the path is refused"
        )
    };
    match root.unlink(path) {
        Ok(true) => Ok(format!("a symlink to {target}")),
        Ok(false) => Err(refused("it is gone, or its directory is out of reach".to_string())),
        // On two layers the new entry is made in the upper layer, where no
        // link is, and it hides the link of the lower layer.
        Err(e) if crate::write::layer_refusal(&e).is_some() => {
            Ok(format!("a symlink to {target} of the lower layer, which the new entry of the upper layer hides"))
        }
        Err(e) => Err(refused(e.to_string())),
    }
}

fn run(root: &Root, settings: &Settings) -> Result<Vec<Fixup>> {
    let mut out = Vec::new();
    root.mkdirs("dev")?;

    // One draw for both random shims: they are two names of one file of
    // bytes. No draw at all unless a shim is necessary.
    let mut filler: Option<(Vec<u8>, Vec<u8>)> = None;

    for (path, major, minor) in DEVICES {
        let k = root.kind(path)?;

        // 1. The right node is there (the image shipped it, or an earlier
        // run made it). Not degraded: it is the real thing.
        if already_right(root, path, &k, *major, *minor)? {
            out.push(
                Fixup::new("dev-node", path, Action::Unchanged)
                    .why(format!("a real character device {major}:{minor} is already here")),
            );
            continue;
        }

        // 2. Name what is there before a change. A directory is refused; a
        // symlink is removed before the `mknod`; each other thing is
        // replaced, and the row names it.
        let displaced = match &k {
            Kind::Missing | Kind::Regular { .. } => None,
            Kind::Dir => {
                out.push(
                    Fixup::new("dev-shim", path, Action::Failed)
                        .why(format!(
                            "{path} is a directory in this image; the crate does not replace a directory with a file"
                        ))
                        .degraded(),
                );
                continue;
            }
            Kind::Symlink(t) => match displace_link(root, path, t) {
                Ok(displaced) => Some(displaced),
                Err(why) => {
                    out.push(Fixup::new("dev-shim", path, Action::Failed).why(why).degraded());
                    continue;
                }
            },
            Kind::CharDev { major: a, minor: b } => Some(format!("a character device {a}:{b}")),
            Kind::Other(m) => Some(match m & 0o170000 {
                0o060000 => "a block device".into(),
                0o140000 => "a socket".into(),
                0o010000 => "a fifo".into(),
                _ => format!("a file of type {m:o}"),
            }),
        };

        // 3. Ask the kernel. Where it permits `mknod`, the payload gets the
        // real thing.
        match root.mknod_char(path, 0o666, *major, *minor, &settings.names.scratch_prefix)? {
            Ok(()) => {
                let mut detail = format!(
                    "a real character device {major}:{minor}. This host permits mknod(2), so the crate made the \
                     node and not a shim"
                );
                if let Some(s) = &displaced {
                    detail.push_str(&format!(". It replaced {s}."));
                }
                // A removed symlink leaves the path empty, so the node is
                // created; each other thing at the path is replaced.
                let action = match (&displaced, &k) {
                    (Some(_), Kind::Symlink(_)) | (None, _) => Action::Created,
                    (Some(_), _) => Action::Rewrote,
                };
                out.push(Fixup::new("dev-node", path, action).why(detail));
            }
            Err(why) => {
                // 4. The wall: a shim, with the errno of the kernel. A check of
                // the space before a large write, and the failure names the
                // place: three shims of 1 MiB on a small tmpfs fill it.
                if filler.is_none() {
                    pod_hostprobe::space::require(
                        &settings.host,
                        &root.written().to_string_lossy(),
                        "the /dev shims",
                        (FILLED_BYTES * 3) as u64,
                        4,
                        SHIM_HEADROOM_BYTES,
                    )?;
                }
                let (zeros, rand) = filler.get_or_insert_with(|| {
                    (vec![0u8; FILLED_BYTES], random_bytes(FILLED_BYTES).unwrap_or_else(|_| vec![0u8; FILLED_BYTES]))
                });
                let mut rows = shim_for(root, path, &why, displaced.as_deref(), zeros, rand)?;
                // A removed symlink leaves the path empty, as for the node
                // above; on two layers the link of the lower layer stays,
                // hidden, and the row says the same.
                if matches!(k, Kind::Symlink(_)) {
                    for f in rows.iter_mut().filter(|f| f.action == Action::Rewrote) {
                        f.action = Action::Created;
                    }
                }
                out.extend(rows);
            }
        }
    }
    fd_links(root, &mut out)?;
    Ok(out)
}

fn fd_links(root: &Root, out: &mut Vec<Fixup>) -> Result<()> {
    for (path, target) in FD_LINKS {
        let refuse = |what: String| {
            Fixup::new("dev-fd-link", path, Action::Failed)
                .why(format!("{path} is {what} in this image; the crate does not replace it with a link"))
                .degraded()
        };
        match root.kind(path)? {
            // The usual link is here: not degraded, whoever put it here.
            Kind::Symlink(t) if t == *target => out.push(
                Fixup::new("dev-fd-link", path, Action::Unchanged)
                    .why(format!("a symlink to {target} is already here")),
            ),
            // The link of the image points elsewhere: it stays.
            Kind::Symlink(t) => out.push(
                Fixup::new("dev-fd-link", path, Action::Unchanged)
                    .why(format!("a symlink to {t} is already here; the crate keeps the link of the image")),
            ),
            Kind::Missing => out.push(match root.symlink(path, target)? {
                Ok(()) => Fixup::new("dev-fd-link", path, Action::Created).why(format!("a symlink to {target}")),
                Err(why) => Fixup::new("dev-fd-link", path, Action::Failed)
                    .why(format!("{path} could not link to {target}: {why}"))
                    .degraded(),
            }),
            Kind::Dir => out.push(refuse("a directory".to_string())),
            Kind::Regular { .. } => out.push(refuse("a regular file".to_string())),
            Kind::CharDev { major: a, minor: b } => out.push(refuse(format!("a character device {a}:{b}"))),
            Kind::Other(m) => out.push(refuse(format!("a file of type {m:o}"))),
        }
    }
    Ok(())
}

/// The regular-file shim of one device, after `mknod` was refused.
fn shim_for(
    root: &Root,
    path: &str,
    why: &str,
    displaced: Option<&str>,
    zeros: &[u8],
    rand: &[u8],
) -> Result<Vec<Fixup>> {
    if path == "dev/null" {
        // Cut to zero on EACH run: a payload that wrote into it leaves a
        // large file, and the next container of a shared root gets it.
        let grew = match root.kind(path)? {
            Kind::Regular { len, .. } if len > 0 => Some(len),
            _ => None,
        };
        let w = root.write(path, b"", 0o666)?;
        let mut detail = match grew {
            Some(len) => format!(
                "mknod(2) refused with {why}, so this is a regular file, and it held {len} bytes: a payload wrote \
                 into it. A write stays until the next run"
            ),
            None => format!(
                "mknod(2) refused with {why}, so this is a regular file: a write stays until the next run, and a \
                 read gives EOF"
            ),
        };
        if let Some(s) = displaced {
            detail.push_str(&format!(". It replaced {s}."));
        }
        return Ok(vec![shim(path, w, detail)]);
    }
    let (bytes, what) = if path == "dev/zero" {
        (zeros, "zero bytes")
    } else {
        (rand, "bytes read one time from getrandom(2) of the host")
    };
    filled(root, path, bytes, what, why, displaced)
}

fn filled(root: &Root, path: &str, bytes: &[u8], what: &str, why: &str, displaced: Option<&str>) -> Result<Vec<Fixup>> {
    // A shim of the right length stays: a write of 1 MiB on each start
    // gives nothing.
    if let Kind::Regular { len, .. } = root.kind(path)?
        && len as usize == bytes.len()
    {
        return Ok(vec![
            Fixup::new("dev-shim", path, Action::Unchanged)
                .why(format!("already a shim of {} bytes", bytes.len()))
                .degraded(),
        ]);
    }
    let w = root.write(path, bytes, 0o666)?;
    let mut detail = format!(
        "mknod(2) refused with {why}, so this is a regular file of {} {what}. It ENDS: a read after {} bytes gives \
         EOF, and the device does not",
        bytes.len(),
        bytes.len()
    );
    if let Some(s) = displaced {
        detail.push_str(&format!(". It replaced {s}."));
    }
    Ok(vec![shim(path, w, detail)])
}

fn shim(path: &str, w: Wrote, detail: impl Into<String>) -> Fixup {
    Fixup::new("dev-shim", path, crate::act(w)).why(detail).degraded()
}

/// `getrandom(2)`, in bounded parts. Never a read of `/dev/urandom` to its
/// end: the device has no end.
fn random_bytes(n: usize) -> Result<Vec<u8>> {
    let mut out = vec![0u8; n];
    let mut off = 0;
    while off < n {
        // The kernel can give fewer bytes than asked: the loop is the
        // contract.
        match pod_hostprobe::sys::getrandom(&mut out[off..]) {
            Ok(0) => break,
            Ok(k) => off += k as usize,
            Err(e) if e == pod_hostprobe::sys::EINTR => continue,
            Err(e) => return Err(crate::error::sys("getrandom(2) for the /dev/urandom shim", e)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
