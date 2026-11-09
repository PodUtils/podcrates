//! [`TODO/packaging.md`](../../../TODO/packaging.md) T-1003: the launch ladder,
//! and the order it is tried in.
//!
//! memfd, then FUSE, then an ephemeral tmpfs, then a private run directory,
//! then a persistent cache: the onelf shape
//! (`references/qaidvoid__onelf/tree/crates/onelf-rt/src/main.rs:125-131`),
//! shortened where this runtime refuses a rung by probe rather than omitting
//! it. Each **forced** mode refuses with a named reason rather than falling
//! through, copying that file's `:216-218`, `:235-237` and `:255-257`.
//!
//! ⭐ **Rung-complete is the memfd rung, the run-directory rung, the
//! ephemeral-tmpfs rung, the FUSE rung and the persistent-cache rung.**
//! The memfd driver ([`crate::memfd`]) writes bytes, seals where accepted,
//! and execs the fd. The directory rungs replicate the extracted tree into
//! a private per-run directory, onto a per-run tmpfs mount, or into a
//! persistent per-image directory ([`crate::stage`]). The FUSE rung serves
//! the extracted tree over `/dev/fuse` without copying it ([`crate::fuse`]).
//! The single file with an embedded rootfs is a follow-up the entry
//! names, not a rung built here.
//!
//! ⚠ This module orders and refuses; it never measures. What the machine
//! permits arrives as [`Availability`], read by the caller from the probe, so
//! every decision below is a pure function unit tests drive.

use crate::memfd::MemfdRefusal;
use crate::{Error, Result};

/// A rung of the ladder, in trial order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Memfd,
    Fuse,
    Tmpfs,
    RunDir,
    Cache,
}

impl Mode {
    /// The trial order: memfd, FUSE, tmpfs, run directory, cache.
    pub const ORDER: [Mode; 5] = [
        Mode::Memfd,
        Mode::Fuse,
        Mode::Tmpfs,
        Mode::RunDir,
        Mode::Cache,
    ];

    /// The word `PODVM_MODE` carries and `PODVM_ACTIVE_MODE` reports.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Memfd => "memfd",
            Mode::Fuse => "fuse",
            Mode::Tmpfs => "tmpfs",
            Mode::RunDir => "rundir",
            Mode::Cache => "cache",
        }
    }

    /// Parse a `PODVM_MODE` request. `run-dir` is accepted beside `rundir`:
    /// docker spells compound flags both ways and the request must not fail on
    /// a hyphen.
    pub fn parse(s: &str) -> Result<Mode> {
        match s {
            "memfd" => Ok(Mode::Memfd),
            "fuse" => Ok(Mode::Fuse),
            "tmpfs" => Ok(Mode::Tmpfs),
            "rundir" | "run-dir" | "run_dir" => Ok(Mode::RunDir),
            "cache" => Ok(Mode::Cache),
            other => Err(Error::Runtime(format!(
                "PODVM_MODE={other:?} names no launch rung. The rungs are: \
                 memfd, fuse, tmpfs, rundir, cache (TODO/packaging.md T-1003)"
            ))),
        }
    }
}

/// What the prober found, as the ladder's inputs.
///
/// ⭐ Passed in rather than read here. FUSE needs `/dev/fuse`, which `mknod`
/// cannot create, and an ephemeral tmpfs needs a mount: both rungs are refused
/// by probe, not omitted, so the same binary uses them where they exist.
pub struct Availability {
    /// A memfd executes ([`crate::memfd::memfd_exec`], T-1688) and the
    /// payload is static-PIE with no `PT_INTERP` ([`crate::memfd::eligible`]).
    pub memfd: bool,
    /// `/dev/fuse` opens.
    pub fuse: bool,
    /// A mount is permitted.
    pub tmpfs: bool,
    /// The store's runs/ staging accepted a directory: the CLI admits
    /// the run-directory rung only where it staged one.
    pub rundir: bool,
    /// The store's cache/ staging accepted a directory and the cache was
    /// asked for: the CLI admits the persistent-cache rung only where
    /// both hold.
    pub cache: bool,
    /// The publisher asked for cache at pack time, or `PODVM_CACHE` opts this
    /// launch in: the cache rung runs only when asked for, copying onelf's
    /// `CACHE_REQUESTED` gate.
    pub cache_requested: bool,
}

impl Availability {
    /// Why `mode` is down, naming the probe or the missing wiring.
    pub fn refusal_reason(mode: Mode, memfd_why: Option<&MemfdRefusal>) -> String {
        match mode {
            Mode::Memfd => match memfd_why {
                Some(why) => format!("memfd mode is unavailable: {why}"),
                None => {
                    "memfd mode is unavailable: memfd_create is refused on this kernel".to_string()
                }
            },
            Mode::Fuse => {
                "/dev/fuse did not open on this machine, so FUSE mode is unavailable".to_string()
            }
            Mode::Tmpfs => {
                "a mount is refused on this runtime, so tmpfs mode is unavailable".to_string()
            }
            Mode::RunDir => "the store's runs/ staging is not writable on this \
                 machine, so the run-directory rung is unavailable"
                .to_string(),
            Mode::Cache => "the store's cache/ staging is not writable on this \
                 machine, or the cache was not asked for (PODVM_CACHE=1), so \
                 the persistent-cache rung is unavailable"
                .to_string(),
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

/// The payload file, read from outside the rootfs, for the memfd leg.
///
/// An argument with a `/` in it names a path under the rootfs. A bare name is
/// looked for along `path_dirs` and the first file wins, which is the order
/// the child tries after the chroot. One walker for every reader of the
/// payload from the host side: `crate::abi::resolve_in` owns the guest-kernel
/// symlink walk, and this owns the `PATH` search over it, so a second copy of
/// either is the copy that diverges (`docs/conventions/code.md`).
pub fn resolve_payload(rootfs: &str, argv0: &str, path_dirs: &[String]) -> Result<String> {
    use crate::abi::{resolve_in, ResolveKind};
    let root = std::path::Path::new(rootfs);
    if !root.is_dir() {
        return Err(Error::Runtime(format!(
            "{rootfs}: not a directory podvm can read"
        )));
    }
    let say = |guest: &str, kind: ResolveKind| match kind {
        ResolveKind::Absent => format!("{argv0} names no file in the image"),
        ResolveKind::Escapes => format!("{guest} escapes the image"),
        ResolveKind::Loop => format!("{guest} has too many levels of symlinks"),
    };
    if argv0.contains('/') {
        return resolve_in(root, argv0)
            .map(|p| p.display().to_string())
            .map_err(|kind| Error::Runtime(say(argv0, kind)));
    }
    let mut refused: Option<Error> = None;
    for d in path_dirs {
        let guest = format!("{}/{argv0}", d.trim_end_matches('/'));
        match resolve_in(root, &guest) {
            Ok(p) => return Ok(p.display().to_string()),
            Err(ResolveKind::Absent) => continue,
            // ⚠ The first refusal wins, so a loop reads as a loop rather
            // than as a missing file once the other directories miss.
            Err(kind) => {
                if refused.is_none() {
                    refused = Some(Error::Runtime(say(&guest, kind)));
                }
            }
        }
    }
    Err(refused.unwrap_or_else(|| Error::Runtime(format!("{argv0} names no file in the image"))))
}

/// The environment name carrying the payload's resolved guest path.
///
/// Set beside the exec by the caller that resolved it (the CLI, which owns
/// the rootfs host path): the interposer answers `readlink` and `open` of
/// `/proc/self/exe` with it where the real call fails, which is the chroot
/// entry with no procfs. Where the real call succeeds (no chroot, host
/// `/proc` mounted) the interposer prefers it, so a stale or foreign value
/// here can never poison a truth the kernel still answers.
pub const GUEST_EXE_VAR: &str = "PODVM_GUEST_EXE";

/// The payload's resolved path as the guest sees it, for [`GUEST_EXE_VAR`].
///
/// Mirrors [`resolve_payload`]'s trial order (the argument as a path, else
/// each `PATH` directory in order) through the same guest-kernel symlink
/// walk, then strips the rootfs prefix: the walk followed the image's own
/// links, so the remainder is the canonical guest path (`/bin/bash` on a
/// `/usr`-merged image resolves to `/usr/bin/bash`, which is what the
/// kernel's own `readlink /proc/self/exe` would print).
///
/// `None` where nothing resolves: the caller sets no variable and the
/// interposer falls through to the honest failure, which the entry refusal
/// already names.
pub fn guest_exe(rootfs: &str, argv0: &str, path_dirs: &[String]) -> Option<String> {
    use crate::abi::resolve_in;
    let root = std::path::Path::new(rootfs);
    let base = rootfs.trim_end_matches('/');
    let first = |guest: &str| -> Option<String> {
        let host = resolve_in(root, guest).ok()?;
        let rel = host.to_string_lossy();
        let rel = rel.strip_prefix(base)?;
        Some(if rel.is_empty() {
            "/".to_string()
        } else {
            rel.to_string()
        })
    };
    if argv0.contains('/') {
        return first(argv0);
    }
    for d in path_dirs {
        let guest = format!("{}/{argv0}", d.trim_end_matches('/'));
        if let Some(g) = first(&guest) {
            return Some(g);
        }
    }
    None
}

/// The payload bytes the memfd leg is judged on.
///
/// Bounded at 128 MiB, which is `crate::abi::Elf::read`'s own ceiling: this
/// runtime may not assume a file it was pointed at is the size it expected,
/// and a payload past it is a named refusal rather than a buffer.
pub fn payload_bytes(rootfs: &str, argv0: &str, path_dirs: &[String]) -> Result<Vec<u8>> {
    const CEILING: u64 = 128 * 1024 * 1024;
    let path = resolve_payload(rootfs, argv0, path_dirs)?;
    let len = std::fs::metadata(&path)
        .map_err(|e| Error::Runtime(format!("{path}: {e}")))?
        .len();
    if len > CEILING {
        return Err(Error::Runtime(format!(
            "{path} is {len} bytes, over the {CEILING}-byte ceiling podvm \
             reads a payload within"
        )));
    }
    std::fs::read(&path).map_err(|e| Error::Runtime(format!("{path}: {e}")))
}

/// Choose the rung: the forced one where asked, else the first one up.
///
/// ⛔ A forced mode that is down REFUSES with a named reason rather than
/// falling through to a rung the caller did not ask for. Falling through
/// would run the payload from a place the caller forbade, and after the exec
/// nothing can be said about where it went.
pub fn choose(
    forced: Option<Mode>,
    avail: &Availability,
    memfd_why: Option<&MemfdRefusal>,
) -> Result<Mode> {
    if let Some(mode) = forced {
        if avail.is_up(mode) {
            return Ok(mode);
        }
        return Err(Error::Runtime(format!(
            "PODVM_MODE={} was forced but {}",
            mode.name(),
            Availability::refusal_reason(mode, memfd_why)
        )));
    }
    for mode in Mode::ORDER {
        // ⛔ The cache leaves an extraction on disk, so it runs only when asked
        // for. Otherwise a machine with nothing else up silently litters it.
        if mode == Mode::Cache && !avail.cache_requested {
            continue;
        }
        if avail.is_up(mode) {
            return Ok(mode);
        }
    }
    if avail.cache && !avail.cache_requested {
        return Err(Error::Runtime(
            "no execution rung is available on this host; set PODVM_CACHE=1 to \
             allow a persistent extraction under the cache directory \
             (TODO/packaging.md T-1003)"
                .to_string(),
        ));
    }
    Err(Error::Runtime(format!(
        "no execution rung is available on this host: {}",
        Mode::ORDER
            .iter()
            .map(|m| Availability::refusal_reason(*m, memfd_why))
            .collect::<Vec<_>>()
            .join("; ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EXIT_RUNTIME_ERROR;

    fn all_up() -> Availability {
        Availability {
            memfd: true,
            fuse: true,
            tmpfs: true,
            rundir: true,
            cache: true,
            cache_requested: true,
        }
    }

    fn all_down() -> Availability {
        Availability {
            memfd: false,
            fuse: false,
            tmpfs: false,
            rundir: false,
            cache: false,
            cache_requested: false,
        }
    }

    /// ⭐ The order is the design: memfd first, cache last and only when asked.
    #[test]
    fn the_trial_order_is_memfd_first_and_cache_last() {
        assert_eq!(
            Mode::ORDER.map(Mode::name),
            ["memfd", "fuse", "tmpfs", "rundir", "cache"]
        );
        assert_eq!(choose(None, &all_up(), None).unwrap(), Mode::Memfd);
    }

    /// ⛔ A forced mode that is down refuses naming it, rather than falling
    /// through to a rung the caller did not ask for.
    #[test]
    fn a_forced_mode_that_is_down_refuses_rather_than_falling_through() {
        let e = choose(Some(Mode::Fuse), &all_down(), None).unwrap_err();
        let text = format!("{e}");
        assert!(text.contains("fuse"), "{text}");
        assert!(text.contains("/dev/fuse"), "{text}");
        assert_eq!(e.exit_code(), EXIT_RUNTIME_ERROR);
    }

    /// A forced mode that is up is honoured, including past an earlier rung.
    #[test]
    fn a_forced_mode_that_is_up_is_honoured() {
        assert_eq!(
            choose(Some(Mode::Tmpfs), &all_up(), None).unwrap(),
            Mode::Tmpfs
        );
    }

    /// Unknown requests refuse with the list, never a guess.
    #[test]
    fn an_unknown_request_refuses_with_the_list() {
        let e = Mode::parse("memfd2").unwrap_err();
        let text = format!("{e}");
        assert!(text.contains("memfd2"), "{text}");
        assert!(text.contains("rundir"), "{text}");
        assert_eq!(e.exit_code(), EXIT_RUNTIME_ERROR);
    }

    /// `run-dir` and `run_dir` spell the same rung as `rundir`.
    #[test]
    fn the_run_dir_spellings_agree() {
        assert_eq!(Mode::parse("run-dir").unwrap(), Mode::RunDir);
        assert_eq!(Mode::parse("run_dir").unwrap(), Mode::RunDir);
        assert_eq!(Mode::parse("rundir").unwrap(), Mode::RunDir);
    }

    /// ⛔ The cache runs only when asked for: a cache-capable machine without
    /// the opt-in skips it rather than littering the disk silently.
    #[test]
    fn the_cache_runs_only_when_asked_for() {
        let mut avail = all_down();
        avail.cache = true;
        let e = choose(None, &avail, None).unwrap_err();
        assert!(format!("{e}").contains("PODVM_CACHE=1"), "{e}");

        avail.cache_requested = true;
        assert_eq!(choose(None, &avail, None).unwrap(), Mode::Cache);
    }

    /// With nothing up and nothing asked, the refusal names every rung.
    #[test]
    fn nothing_up_is_a_refusal_naming_every_rung() {
        let e = choose(None, &all_down(), None).unwrap_err();
        let text = format!("{e}");
        for rung in ["memfd", "/dev/fuse", "tmpfs", "run-directory", "cache"] {
            assert!(text.contains(rung), "{text}");
        }
    }

    /// The memfd refusal carries the eligibility answer beside it.
    #[test]
    fn a_forced_memfd_names_why_the_payload_cannot_go_that_way() {
        let why = MemfdRefusal::HasInterp("/lib64/ld-linux-x86-64.so.2".to_string());
        let e = choose(Some(Mode::Memfd), &all_down(), Some(&why)).unwrap_err();
        let text = format!("{e}");
        assert!(text.contains("ld-linux"), "{text}");
    }

    /// A refusal is docker's runtime error, read from the error and not a
    /// literal: [`Error::exit_code`] is the one place that maps it.
    #[test]
    fn refusals_are_dockers_runtime_error() {
        let e = choose(Some(Mode::Fuse), &all_down(), None).unwrap_err();
        assert_eq!(e.exit_code(), EXIT_RUNTIME_ERROR);
    }

    fn fixture_root(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("podvm-ladder-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("bin")).unwrap();
        d
    }

    /// A bare name is found along `path_dirs`, in order, and an absolute
    /// argument names the path under the rootfs.
    #[test]
    fn a_payload_resolves_along_path_dirs_and_by_path() {
        let d = fixture_root("resolve");
        std::fs::write(d.join("bin/prog"), b"\x7fELF").unwrap();
        let root = d.to_string_lossy().to_string();
        let dirs = ["/sbin", "/bin"]
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>();
        let got = resolve_payload(&root, "prog", &dirs).unwrap();
        assert!(got.ends_with("bin/prog"), "{got}");
        let got = resolve_payload(&root, "/bin/prog", &dirs).unwrap();
        assert!(got.ends_with("bin/prog"), "{got}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A name the image does not hold is a sentence, never a silent miss.
    #[test]
    fn a_missing_payload_is_a_sentence() {
        let d = fixture_root("missing");
        let root = d.to_string_lossy().to_string();
        let e = resolve_payload(&root, "absent", &["/bin".to_string()]).unwrap_err();
        assert!(e.to_string().contains("names no file"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A `..` that escapes the root is refused rather than followed: the path
    /// it names is the host's, and reading it answers about the wrong machine.
    #[test]
    fn a_payload_escaping_the_image_is_refused() {
        let d = fixture_root("escape");
        std::os::unix::fs::symlink("../../outside", d.join("bin/evil")).unwrap();
        let root = d.to_string_lossy().to_string();
        let e = resolve_payload(&root, "evil", &["/bin".to_string()]).unwrap_err();
        assert!(e.to_string().contains("escapes the image"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The bytes are what the file holds, whole.
    #[test]
    fn the_payload_bytes_are_the_files() {
        let d = fixture_root("bytes");
        std::fs::write(d.join("bin/prog"), b"#!/bin/sh\necho hi\n").unwrap();
        let root = d.to_string_lossy().to_string();
        let got = payload_bytes(&root, "prog", &["/bin".to_string()]).unwrap();
        assert_eq!(got, b"#!/bin/sh\necho hi\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// T-0413: the guest path is the resolved one, not the invoked one. A
    /// `/usr`-merged `/bin` link resolves through, so the interposer's
    /// `/proc/self/exe` answer is what the kernel would print.
    #[test]
    fn the_guest_exe_is_the_resolved_guest_path() {
        let d = fixture_root("exe");
        std::fs::create_dir_all(d.join("usr/bin")).unwrap();
        std::fs::write(d.join("usr/bin/bash"), b"\x7fELF").unwrap();
        // The merged-/usr link replaces the plain directory the fixture
        // helper makes: a symlink call where a directory stands fails.
        std::fs::remove_dir(d.join("bin")).unwrap();
        std::os::unix::fs::symlink("usr/bin", d.join("bin")).unwrap();
        let root = d.to_string_lossy().to_string();
        let dirs = ["/bin".to_string()];
        assert_eq!(
            guest_exe(&root, "bash", &dirs).as_deref(),
            Some("/usr/bin/bash")
        );
        assert_eq!(
            guest_exe(&root, "/bin/bash", &dirs).as_deref(),
            Some("/usr/bin/bash")
        );
        assert_eq!(guest_exe(&root, "absent", &dirs), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
