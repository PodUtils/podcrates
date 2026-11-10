//! The entry of a payload with no `chroot(2)`.
//!
//! Where the probe denies chroot, the chroot sequence cannot run, but the
//! host can still read the payload file. This module holds the parts of
//! an entry of the payload as it is, with the root of the host:
//!
//! * a dynamic payload runs through the loader of the image, executed by
//!   host path, with the library directories of the image on
//!   `LD_LIBRARY_PATH` and the interposer preloaded by host path;
//! * a static payload runs from a staged memfd, as the memfd rung of the
//!   ladder stages it, with no chroot under it.
//!
//! This rung isolates nothing. The payload sees the file system of the
//! host, with only its working directory inside the image, and an
//! absolute path resolves on the host except where the interposer changes
//! it. The banner of the caller names this on each run, and the active
//! mode variable reads `userland`.
//!
//! The caller does not give a script (`#!`) to this rung: the kernel
//! would open its interpreter on the root of the host. The caller reads
//! the line ([`memfd::shebang_parts`]), resolves the interpreter inside
//! the image, and runs the interpreter with the script as its argument.
//! The caller refuses a foreign architecture: this rung does not prove
//! `binfmt_misc` with no chroot.

use crate::{Names, memfd};

/// Which no-chroot family a payload takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Through the image's own loader, with the interposer preloaded.
    Loader,
    /// From a staged memfd, for static payloads.
    Memfd,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Family::Loader => "loader",
            Family::Memfd => "memfd",
        }
    }
}

/// A dynamic payload's loader invocation, all paths host-absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderPlan {
    /// The image's loader, by host path.
    pub loader_host: String,
    /// The payload, by host path.
    pub payload_host: String,
    /// The image's library directories, by host path, that exist.
    pub lib_dirs: Vec<String>,
}

/// The image's library directories that exist, by host path.
///
/// The loader's own directory first (it is a library directory wherever
/// the image keeps its loader), then the standard ones. An empty answer
/// is a refusal upstream: a dynamic payload with nowhere to load from
/// fails inside the loader with nothing attached to it.
pub fn lib_dirs(rootfs: &str, interp_guest: &str) -> Vec<String> {
    let base = rootfs.trim_end_matches('/');
    let mut out = Vec::new();
    let mut push = |dir: String| {
        if !out.contains(&dir) && std::path::Path::new(&dir).is_dir() {
            out.push(dir);
        }
    };
    if let Some((d, _)) = interp_guest.rsplit_once('/')
        && !d.is_empty()
    {
        push(format!("{base}{d}"));
    }
    for d in [
        "/lib",
        "/lib64",
        "/usr/lib",
        "/usr/lib64",
        "/lib/x86_64-linux-gnu",
        "/lib/aarch64-linux-gnu",
        "/lib/riscv64-linux-gnu",
        "/lib/s390x-linux-gnu",
        "/lib/ppc64le-linux-gnu",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/lib/riscv64-linux-gnu",
        "/usr/lib/s390x-linux-gnu",
        "/usr/lib/ppc64le-linux-gnu",
    ] {
        push(format!("{base}{d}"));
    }
    out
}

/// The program path the loader opens, keeping the invoked name.
///
/// The resolver answers the file; but the kernel's loader opens by path
/// and the payload dispatches on `argv[0]`'s basename. So the loader opens
/// the UNRESOLVED spelling, the guest path as invoked mapped to the host,
/// where it names something openable. Falls back to the resolved file
/// where it does not: a name that only resolved through the search path
/// is still the file the guest kernel would run, and a link whose target
/// is absolute (`/bin/sh` pointing at `/bin/busybox` on alpine) dangles
/// on the host side, so the resolved file is what opens. The invoked
/// name a multi-call binary dispatches on is carried separately by
/// [`loader_argv_for`].
pub fn invocation_path(rootfs: &str, argv0: &str, path_dirs: &[String], resolved_host: &str) -> String {
    let base = rootfs.trim_end_matches('/');
    if argv0.contains('/') {
        let host = format!("{base}/{}", argv0.trim_start_matches('/'));
        if std::path::Path::new(&host).exists() {
            return host;
        }
    } else {
        for d in path_dirs {
            let host = format!("{base}/{}/{argv0}", d.trim_matches('/'));
            if std::path::Path::new(&host).exists() {
                return host;
            }
        }
    }
    resolved_host.to_string()
}

/// The basename of a host path: the final `/`-separated element, or the
/// whole string where it carries none.
fn basename_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The argv the loader execs for a dynamic payload.
///
/// Usually the loader, the payload by host path, then the payload's own
/// arguments. Where the payload resolved to a busybox multi-call binary
/// through another name (`sh` through `/bin/sh`, whose absolute link the
/// host cannot open, so the resolved file is what opens), the invoked
/// name is passed as the first payload argument: busybox dispatches on
/// `argv[0]`'s basename, which the resolved file renamed to `busybox`, so
/// the applet name rides beside it exactly as `busybox sh ...` spells
/// it on a command line. Under a chroot the kernel would exec the link
/// with the caller's argv and busybox would select the same applet, so
/// this reproduces that selection without it.
///
/// Returns the argv and whether the applet name was inserted: the banner
/// names the insert, because the payload's `argv[0]` is not the path that
/// was opened.
pub fn loader_argv_for(loader_host: &str, payload_host: &str, argv0: &str, rest: &[String]) -> (Vec<String>, bool) {
    let invoked = basename_of(argv0);
    let resolved = basename_of(payload_host);
    if resolved == "busybox" && !invoked.is_empty() && invoked != "busybox" {
        let mut argv = Vec::with_capacity(rest.len() + 3);
        argv.push(loader_host.to_string());
        argv.push(payload_host.to_string());
        argv.push(invoked.to_string());
        argv.extend(rest.iter().cloned());
        return (argv, true);
    }
    let mut argv = Vec::with_capacity(rest.len() + 2);
    argv.push(loader_host.to_string());
    argv.push(payload_host.to_string());
    argv.extend(rest.iter().cloned());
    (argv, false)
}

/// Resolve the loader invocation for a dynamic payload.
///
/// `interp_guest` is the `PT_INTERP` path as the image sees it,
/// `payload_host` the payload by host path. The loader resolves inside
/// the root file system first: the `/lib64/ld-linux-*.so.2` of Debian is
/// an absolute link whose target exists only in the image, so a plain
/// join with the host path points at nothing, and `is_executable` on it
/// would refuse a payload that the kernel of the guest runs. The resolved
/// file must exist and execute by host path, and at least one library
/// directory must exist; else the refusal names the guest path, and the
/// loader does not fail later.
pub fn loader_plan(rootfs: &str, interp_guest: &str, payload_host: &str) -> Result<LoaderPlan, String> {
    // The root file system is the root of the resolution of each guest
    // path on this rung; the root of the host is never a fallback. Where
    // the walk fails, the refusal names the plain join beside the guest
    // path, so a missing loader still reads as missing.
    let loader_host = match crate::abi::resolve_in(std::path::Path::new(rootfs), interp_guest) {
        Ok(p) => p.display().to_string(),
        Err(_) => format!("{}{interp_guest}", rootfs.trim_end_matches('/')),
    };
    if !memfd::is_executable(std::path::Path::new(&loader_host)) {
        return Err(format!(
            "{interp_guest} names the image's loader, but {loader_host} is not \
             an executable file on the host side"
        ));
    }
    let dirs = lib_dirs(rootfs, interp_guest);
    if dirs.is_empty() {
        return Err(format!(
            "the image keeps no library directory beside {interp_guest}, so its \
             loader would fail on the first library it cannot find"
        ));
    }
    Ok(LoaderPlan { loader_host, payload_host: payload_host.to_string(), lib_dirs: dirs })
}

/// Rewrite the environment for a no-chroot entry.
///
/// * `LD_PRELOAD`'s guest-absolute object becomes the host path, so the
///   loader finds what `place` wrote. Other preloaded objects ride
///   unchanged.
/// * `LD_LIBRARY_PATH` becomes the image's library directories, so the
///   loader resolves the payload's `DT_NEEDED` from the image and not
///   the host. Later wins, like every other `*_for` here: the caller's
///   own value, if any, is dropped in favour of the image's.
/// * `names.lib_path` carries the same list. The payload can change
///   `LD_LIBRARY_PATH`; the interposer reads this variable for the
///   library path of the loader that runs each dynamic child. The
///   caller's own value of it is dropped, as for `LD_LIBRARY_PATH`.
///
/// `guest_preload` is the path the tier was placed under as the payload
/// sees it (`LD_PRELOAD`'s first element where the tier loaded). Where
/// the tier declined and nothing carries it, only the library path is
/// set: a dynamic payload without the tier still needs its libraries.
///
/// Returns the environment and the caller preload that was dropped, if
/// any: an `LD_PRELOAD` naming guest paths cannot resolve without a
/// chroot, so it is dropped rather than left to fail the load, and the
/// caller names the drop on the banner.
pub fn host_env(
    names: &Names,
    rootfs: &str,
    guest_preload: &str,
    env: &[String],
    lib_dirs: &[String],
) -> (Vec<String>, Option<String>) {
    let base = rootfs.trim_end_matches('/');
    let mut out = Vec::with_capacity(env.len() + 1);
    let mut dropped: Option<String> = None;
    for e in env {
        let Some((k, v)) = e.split_once('=') else {
            continue;
        };
        if k == "LD_PRELOAD" {
            if !v.split(' ').any(|p| p == guest_preload) {
                dropped = Some(v.to_string());
                continue;
            }
            let mut joined = String::new();
            for p in v.split(' ') {
                if !joined.is_empty() {
                    joined.push(' ');
                }
                if p == guest_preload {
                    joined.push_str(base);
                }
                joined.push_str(p);
            }
            out.push(format!("LD_PRELOAD={joined}"));
        } else if k == "LD_LIBRARY_PATH" || k == names.lib_path {
            continue;
        } else {
            out.push(e.clone());
        }
    }
    out.push(format!("LD_LIBRARY_PATH={}", lib_dirs.join(":")));
    out.push(format!("{}={}", names.lib_path, lib_dirs.join(":")));
    (out, dropped)
}

/// The longest side of a pair of the table that the interposer reads. A
/// longer side is malformed there and the interposer skips its pair,
/// which on this rung would stop each change of a path and leave host
/// paths bare. The builder below refuses it.
const MAX_MAP_SIDE: usize = 4096;

/// Build the table of path maps (`names.maps`) that anchors the root of a
/// run with no chroot.
///
/// The `/` of the guest maps to the host path of the root file system, so
/// an absolute guest path resolves inside the image and not on the tree
/// of the host. The paths that the interposer never changes (its own
/// guard directory, `/proc`, the list of the user) are not in the table:
/// the interposer enforces them, and skips a pair of a table that tries
/// to move them.
///
/// A root file system that the table cannot spell is refused: relative,
/// with a NUL, with `:` or `,`, empty after the trim, or too long. An
/// empty table changes nothing, which on this rung is a silent write to
/// the host, so a malformed root is a refusal, never a table.
///
/// The caller sets the variable to the returned table before the payload
/// starts, after it removes each value of the caller, and names the table
/// on the banner: the change of paths must be visible, never silent.
pub fn userland_maps_table(names: &Names, rootfs: &str) -> Result<String, String> {
    if rootfs.contains('\0') {
        return Err("the rootfs path contains a NUL byte".to_string());
    }
    let base = rootfs.trim_end_matches('/');
    if base.is_empty() {
        return Err("the rootfs path is empty".to_string());
    }
    if !base.starts_with('/') {
        return Err(format!("{rootfs:?} is not an absolute rootfs path"));
    }
    if base.contains(':') || base.contains(',') {
        return Err(format!(
            "{rootfs:?} cannot be spelled in a {} table: `:` and `,` separate pairs, so a table \
             carrying it would parse as the wrong mapping",
            names.maps
        ));
    }
    if base.len() > MAX_MAP_SIDE {
        return Err(format!(
            "the rootfs path is {} bytes, over the {MAX_MAP_SIDE}-byte table side the interposer reads",
            base.len()
        ));
    }
    Ok(format!("/:{base}"))
}

#[cfg(test)]
mod tests;
