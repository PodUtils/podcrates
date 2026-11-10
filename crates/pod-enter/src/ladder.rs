//! The launch ladder, and the order in which it tries its rungs.
//!
//! memfd, then FUSE, then a tmpfs, then a private run directory, then a
//! persistent cache: the order of onelf, with each rung that the runtime
//! refuses refused by a probe, not left out. Each forced mode refuses with
//! a named reason and does not fall through.
//!
//! The memfd driver ([`crate::memfd`]) writes the bytes, seals them where
//! the kernel accepts it, and execs the descriptor. The directory rungs
//! copy the extracted tree into a private run directory, onto a tmpfs
//! mount for one run, or into a persistent directory for one image
//! ([`crate::stage`]). The FUSE rung serves the extracted tree over
//! `/dev/fuse` with no copy ([`crate::fuse`]).
//!
//! This module orders and refuses; it never measures. What the machine
//! permits comes in as [`Availability`], which the caller reads from the
//! probe, so each decision below is a pure function.

use pod_core::{Class, Failure, Stage};

use crate::abi::{ResolveKind, resolve_in};
use crate::memfd::MemfdRefusal;
use crate::{Error, Names, Result};

/// A rung of the ladder, in the order of the trial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Memfd,
    Fuse,
    Tmpfs,
    RunDir,
    Cache,
}

impl Mode {
    /// The order of the trial: memfd, FUSE, tmpfs, run directory, cache.
    pub const ORDER: [Mode; 5] = [Mode::Memfd, Mode::Fuse, Mode::Tmpfs, Mode::RunDir, Mode::Cache];

    /// The word of the request variable and of the active mode variable.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Memfd => "memfd",
            Mode::Fuse => "fuse",
            Mode::Tmpfs => "tmpfs",
            Mode::RunDir => "rundir",
            Mode::Cache => "cache",
        }
    }

    /// Parse a request. `run-dir` and `run_dir` are accepted beside
    /// `rundir`: docker spells compound flags in both ways.
    pub fn parse(names: &Names, s: &str) -> Result<Mode> {
        match s {
            "memfd" => Ok(Mode::Memfd),
            "fuse" => Ok(Mode::Fuse),
            "tmpfs" => Ok(Mode::Tmpfs),
            "rundir" | "run-dir" | "run_dir" => Ok(Mode::RunDir),
            "cache" => Ok(Mode::Cache),
            other => Err(Error::runtime(
                Class::Usage,
                format!(
                    "{}={other:?} names no launch rung. The rungs are: memfd, fuse, tmpfs, rundir, cache",
                    names.mode_request
                ),
            )),
        }
    }
}

/// What the probe found, as the inputs of the ladder.
///
/// The caller gives these values; the ladder does not read them. FUSE
/// needs `/dev/fuse`, which `mknod` cannot make, and a tmpfs needs a
/// mount: the probe refuses both rungs, so the same binary uses them
/// where they exist.
pub struct Availability {
    /// A memfd executes ([`crate::memfd::memfd_exec`]) and the payload is
    /// static with no `PT_INTERP` ([`crate::memfd::eligible`]).
    pub memfd: bool,
    /// `/dev/fuse` opens.
    pub fuse: bool,
    /// A mount is permitted.
    pub tmpfs: bool,
    /// The caller staged a run directory.
    pub rundir: bool,
    /// The caller can stage a cache directory and the cache was asked for.
    pub cache: bool,
    /// The publisher asked for the cache when it packed the image, or the
    /// cache variable allows it for this launch: the cache rung runs only
    /// when asked for.
    pub cache_requested: bool,
}

impl Availability {
    /// Why `mode` is down, with the probe or the missing staging.
    pub fn refusal_reason(names: &Names, mode: Mode, memfd_why: Option<&MemfdRefusal>) -> String {
        match mode {
            Mode::Memfd => match memfd_why {
                Some(why) => format!("memfd mode is unavailable: {why}"),
                None => "memfd mode is unavailable: memfd_create is refused on this kernel".to_string(),
            },
            Mode::Fuse => "/dev/fuse did not open on this machine, so FUSE mode is unavailable".to_string(),
            Mode::Tmpfs => "a mount is refused on this runtime, so tmpfs mode is unavailable".to_string(),
            Mode::RunDir => "the store's runs/ staging is not writable on this machine, so the \
                             run-directory rung is unavailable"
                .to_string(),
            Mode::Cache => format!(
                "the store's cache/ staging is not writable on this machine, or the cache was not \
                 asked for ({}=1), so the persistent-cache rung is unavailable",
                names.cache
            ),
        }
    }

    fn is_up(&self, mode: Mode) -> bool {
        match mode {
            Mode::Memfd => self.memfd,
            Mode::Fuse => self.fuse,
            Mode::Tmpfs => self.tmpfs,
            Mode::RunDir => self.rundir,
            Mode::Cache => self.cache,
        }
    }
}

/// The host path of the payload file, for the memfd leg.
///
/// An argument with a `/` names a path under the root file system. A bare
/// name is looked for along `path_dirs`, and the first file wins: the
/// order of the child after the chroot. [`crate::abi::resolve_in`] does
/// the walk of the links as the kernel of the guest does it, and this
/// function adds the search of `PATH` over it.
pub fn resolve_payload(rootfs: &str, argv0: &str, path_dirs: &[String]) -> Result<String> {
    let root = std::path::Path::new(rootfs);
    if !root.is_dir() {
        return Err(Error::runtime(Class::NotFound, format!("{rootfs}: not a directory that the entry can read")));
    }
    let say = |guest: &str, kind: ResolveKind| match kind {
        ResolveKind::Absent => Error::runtime(Class::NotFound, format!("{argv0} names no file in the image")),
        ResolveKind::Escapes => Error::runtime(Class::Policy, format!("{guest} escapes the image")),
        ResolveKind::Loop => Error::runtime(Class::Usage, format!("{guest} has too many levels of symlinks")),
    };
    if argv0.contains('/') {
        return resolve_in(root, argv0).map(|p| p.display().to_string()).map_err(|kind| say(argv0, kind));
    }
    let mut refused: Option<Error> = None;
    for d in path_dirs {
        let guest = format!("{}/{argv0}", d.trim_end_matches('/'));
        match resolve_in(root, &guest) {
            Ok(p) => return Ok(p.display().to_string()),
            Err(ResolveKind::Absent) => continue,
            // The first refusal wins, so a loop reads as a loop, not as a
            // missing file after the other directories miss.
            Err(kind) => {
                if refused.is_none() {
                    refused = Some(say(&guest, kind));
                }
            }
        }
    }
    Err(refused.unwrap_or_else(|| say(argv0, ResolveKind::Absent)))
}

/// The resolved path of the payload as the guest sees it.
///
/// The same order as [`resolve_payload`] (the argument as a path, else
/// each `PATH` directory in order) through the same walk of the links,
/// then the prefix of the root file system comes off. The walk followed
/// the links of the image, so the rest is the canonical path of the guest
/// (`/bin/bash` on an image with a merged `/usr` gives `/usr/bin/bash`, as
/// the `readlink /proc/self/exe` of the kernel would).
///
/// `None` where nothing resolves. An interposer can give this path for
/// `/proc/self/exe` in a chroot with no procfs.
pub fn guest_exe(rootfs: &str, argv0: &str, path_dirs: &[String]) -> Option<String> {
    let root = std::path::Path::new(rootfs);
    let base = rootfs.trim_end_matches('/');
    let first = |guest: &str| -> Option<String> {
        let host = resolve_in(root, guest).ok()?;
        let rel = host.to_string_lossy();
        let rel = rel.strip_prefix(base)?;
        Some(if rel.is_empty() { "/".to_string() } else { rel.to_string() })
    };
    if argv0.contains('/') {
        return first(argv0);
    }
    path_dirs.iter().find_map(|d| first(&format!("{}/{argv0}", d.trim_end_matches('/'))))
}

/// The `$0` that the kernel gives a `#!` interpreter for the script.
///
/// An `argv0` with a slash is the answer as given. A bare name is the
/// unresolved join of the first `PATH` directory where it resolves inside
/// the root (the same walk as [`guest_exe`]), with no link followed in the
/// result. `None` where nothing resolves.
pub fn guest_invoked(rootfs: &str, argv0: &str, path_dirs: &[String]) -> Option<String> {
    if argv0.contains('/') {
        return Some(argv0.to_string());
    }
    let root = std::path::Path::new(rootfs);
    path_dirs.iter().find_map(|d| {
        let guest = format!("{}/{argv0}", d.trim_end_matches('/'));
        resolve_in(root, &guest).ok().map(|_| guest)
    })
}

/// The payload for the decision of the entry: the resolved path, the size,
/// the head and the open file.
///
/// The decision reads the head only ([`crate::abi::HEAD_BYTES`]): a `#!`
/// line, the ELF header and the program headers are there. The memfd rung
/// copies the payload from [`Payload::file`] in bounded pieces
/// ([`crate::memfd::stage_file`]), so no step holds the whole file, and
/// the size of the payload has no bound.
#[derive(Debug)]
pub struct Payload {
    /// The resolved path on the host.
    pub path: String,
    /// The size of the file when it was opened.
    pub len: u64,
    /// The first bytes of the file, at most [`crate::abi::HEAD_BYTES`].
    pub head: Vec<u8>,
    file: std::fs::File,
}

impl Payload {
    /// The file that the decision read. The stage copies from it, so the
    /// stage and the decision see the same file, also after a rename.
    pub fn file(&self) -> &std::fs::File {
        &self.file
    }
}

/// Resolve the payload, open it, and read its head for the decision.
///
/// A file that does not open or read is an error that names the path and
/// the error of the system.
pub fn payload_head(rootfs: &str, argv0: &str, path_dirs: &[String]) -> Result<Payload> {
    use std::os::unix::fs::FileExt;
    let path = resolve_payload(rootfs, argv0, path_dirs)?;
    let io = |e: std::io::Error| {
        Error::Runtime(Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("{path}: {e}"), e))
    };
    let file = std::fs::File::open(&path).map_err(io)?;
    let len = file.metadata().map_err(io)?.len();
    let mut head = vec![0u8; crate::abi::HEAD_BYTES as usize];
    let mut got = 0usize;
    while got < head.len() {
        match file.read_at(&mut head[got..], got as u64) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io(e)),
        }
    }
    head.truncate(got);
    Ok(Payload { path, len, head, file })
}

/// Choose the rung: the forced one where the caller asks, else the first
/// one that is up.
///
/// A forced mode that is down refuses with a named reason and does not
/// fall through to a rung that the caller did not ask for: the payload
/// would run from a place that the caller forbade, and after the exec
/// nothing can say where it went.
pub fn choose(
    names: &Names,
    forced: Option<Mode>,
    avail: &Availability,
    memfd_why: Option<&MemfdRefusal>,
) -> Result<Mode> {
    if let Some(mode) = forced {
        if avail.is_up(mode) {
            return Ok(mode);
        }
        return Err(Error::runtime(
            Class::Unsupported,
            format!(
                "{}={} was forced but {}",
                names.mode_request,
                mode.name(),
                Availability::refusal_reason(names, mode, memfd_why)
            ),
        ));
    }
    for mode in Mode::ORDER {
        // The cache leaves an extraction on the disk, so it runs only
        // when asked for.
        if mode == Mode::Cache && !avail.cache_requested {
            continue;
        }
        if avail.is_up(mode) {
            return Ok(mode);
        }
    }
    if avail.cache && !avail.cache_requested {
        return Err(Error::runtime(
            Class::Unsupported,
            format!(
                "no execution rung is available on this host; set {}=1 to allow a persistent \
                 extraction under the cache directory",
                names.cache
            ),
        ));
    }
    let reasons: Vec<String> = Mode::ORDER.iter().map(|m| Availability::refusal_reason(names, *m, memfd_why)).collect();
    Err(Error::runtime(
        Class::Unsupported,
        format!("no execution rung is available on this host: {}", reasons.join("; ")),
    ))
}

#[cfg(test)]
mod tests;
