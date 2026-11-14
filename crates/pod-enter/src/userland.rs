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
//! host, with only its working directory inside the image or a volume, and
//! an absolute path resolves on the host except where the interposer
//! changes it through the table of path maps ([`Maps`]: the root pair and
//! one pair for each volume). The banner of the caller names this on each run, and the active
//! mode variable reads `userland`.
//!
//! The caller does not give a script (`#!`) to this rung: the kernel
//! would open its interpreter on the root of the host. The caller reads
//! the line ([`memfd::shebang_parts`]), resolves the interpreter inside
//! the image, and runs the interpreter with the script as its argument.
//! The caller refuses a foreign architecture: this rung does not prove
//! `binfmt_misc` with no chroot.

use pod_core::Class;
use pod_hostprobe::sys::CBuf;

use crate::abi::{GuestRoot, resolve_dir};
use crate::entry::refuse_workdir;
use crate::{Error, Names, RootDir, memfd};

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
    /// The loader is glibc 2.16 or later: it takes `--inhibit-cache`, and
    /// a caller that gives it keeps the host's `/etc/ld.so.cache` out of
    /// the search (T-182). False where that is not proven: an older loader
    /// reads the option as the name of a program.
    pub inhibit_cache: bool,
}

/// The standard library directories of an image, after the loader's own.
const STANDARD_DIRS: [&str; 21] = [
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
    // musl's own default path holds /usr/local/lib (T-182).
    "/usr/local/lib",
    "/usr/local/lib64",
    "/usr/local/lib/x86_64-linux-gnu",
    "/usr/local/lib/aarch64-linux-gnu",
    "/usr/local/lib/riscv64-linux-gnu",
    "/usr/local/lib/s390x-linux-gnu",
    "/usr/local/lib/ppc64le-linux-gnu",
];

/// The image's library directories that exist, by host path.
///
/// The loader's own directory first (it is a library directory wherever
/// the image keeps its loader), then the standard ones of the image, then
/// each directory of the image's loader configuration ([`ldconf`]: the
/// loader on this rung reads the host's, never the image's). Each guest
/// directory resolves inside the image through the table: a link such as
/// `/lib -> /usr/lib` stays in the image, and a volume serves its own
/// directories. An empty answer is a refusal upstream: a dynamic payload
/// with nowhere to load from fails inside the loader with nothing attached
/// to it.
pub fn lib_dirs<G: GuestRoot + ?Sized>(root: &G, interp_guest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |guest: &str| {
        if let Ok(host) = crate::abi::resolve_dir_in(root, guest) {
            let host = host.display().to_string();
            if !out.contains(&host) {
                out.push(host);
            }
        }
    };
    if let Some((d, _)) = interp_guest.rsplit_once('/')
        && !d.is_empty()
    {
        push(d);
    }
    STANDARD_DIRS.iter().for_each(|d| push(d));
    ldconf::configured_dirs(root, interp_guest).iter().for_each(|d| push(d));
    out
}

/// True where the loader at `interp_guest` is glibc and the libc beside it
/// declares a version of 2.16 or later, the first glibc whose loader takes
/// `--inhibit-cache`.
pub fn takes_inhibit_cache(rootfs: &str, interp_guest: &str, loader_host: &str) -> bool {
    use crate::abi::{Elf, Flavour};
    if Flavour::of_libc_name(interp_guest) != Flavour::Glibc {
        return false;
    }
    let Ok(loader) = Elf::read(loader_host) else { return false };
    let Some(libc) = crate::abi::libc_beside(rootfs, interp_guest, loader.machine) else { return false };
    let Ok(libc) = Elf::read(&libc) else { return false };
    libc.declares.iter().any(|v| {
        let mut parts = v.strip_prefix("GLIBC_").unwrap_or_default().split('.').map(|p| p.parse::<u32>().ok());
        matches!((parts.next(), parts.next()), (Some(Some(major)), Some(Some(minor))) if (major, minor) >= (2, 16))
    })
}

/// The program path the loader opens, keeping the invoked name.
///
/// The resolver answers the file; but the kernel's loader opens by path
/// and the payload dispatches on `argv[0]`'s basename. So the loader opens
/// the UNRESOLVED spelling, the guest path as invoked mapped to the host
/// through the table, where the host opens the resolved file through it
/// (the same device and inode). It falls back to the resolved file where
/// it does not: a name that only resolved through the search path is
/// still the file the guest kernel would run, and a link whose target is
/// absolute (`/bin/sh` pointing at `/bin/busybox` on alpine) names a file
/// of the host from the host side, so the resolved file is what opens.
/// The invoked name a multi-call binary dispatches on is carried
/// separately by [`loader_argv_for`].
pub fn invocation_path<G: GuestRoot + ?Sized>(
    root: &G,
    argv0: &str,
    path_dirs: &[String],
    resolved_host: &str,
) -> String {
    let same = |guest: &str| -> Option<String> {
        let parts: Vec<String> = guest.split('/').filter(|c| !c.is_empty() && *c != ".").map(str::to_string).collect();
        if parts.is_empty() || parts.iter().any(|c| c == "..") {
            return None;
        }
        let host = root.host_of(&parts).0;
        same_file(&host, std::path::Path::new(resolved_host)).then(|| host.display().to_string())
    };
    let found = if argv0.contains('/') {
        same(argv0)
    } else {
        path_dirs.iter().find_map(|d| same(&format!("{}/{argv0}", d.trim_end_matches('/'))))
    };
    found.unwrap_or_else(|| resolved_host.to_string())
}

/// True where the host opens `a` and `b` as one file: the same device and
/// inode, each link followed.
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.is_file() && x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
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
/// `root` is the root file system or the table of the run
/// ([`Maps`]), `interp_guest` the `PT_INTERP` path as the image sees it,
/// `payload_host` the payload by host path. The loader resolves inside
/// the root file system first: the `/lib64/ld-linux-*.so.2` of Debian is
/// an absolute link whose target exists only in the image, so a plain
/// join with the host path points at nothing, and `is_executable` on it
/// would refuse a payload that the kernel of the guest runs. The resolved
/// file must exist and execute by host path, and at least one library
/// directory must exist; else the refusal names the guest path, and the
/// loader does not fail later.
pub fn loader_plan<G: GuestRoot + ?Sized>(
    root: &G,
    interp_guest: &str,
    payload_host: &str,
) -> Result<LoaderPlan, String> {
    // The table is the root of the resolution of each guest path on this
    // rung; the root of the host is never a fallback. Where the walk
    // fails, the refusal names the plain join beside the guest path, so a
    // missing loader still reads as missing.
    let rootfs = root.root().to_string_lossy().into_owned();
    let loader_host = match crate::abi::resolve_in(root, interp_guest) {
        Ok(p) => p.display().to_string(),
        Err(_) => format!("{}{interp_guest}", rootfs.trim_end_matches('/')),
    };
    if !memfd::is_executable(std::path::Path::new(&loader_host)) {
        return Err(format!(
            "{interp_guest} names the image's loader, but {loader_host} is not \
             an executable file on the host side"
        ));
    }
    let dirs = lib_dirs(root, interp_guest);
    if dirs.is_empty() {
        return Err(format!(
            "the image keeps no library directory beside {interp_guest}, so its \
             loader would fail on the first library it cannot find"
        ));
    }
    let inhibit_cache = takes_inhibit_cache(&rootfs, interp_guest, &loader_host);
    Ok(LoaderPlan { loader_host, payload_host: payload_host.to_string(), lib_dirs: dirs, inhibit_cache })
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

pub mod ldconf;
mod maps;
pub use maps::*;

/// The working directory of an entry with no chroot, resolved through the
/// table before the fork: `None` for the root itself, a path relative to
/// the descriptor of the root file system where the root pair serves it,
/// or the host path of a volume where a volume serves it.
///
/// The walk follows each link as the guest kernel does and keeps the
/// guest path, so the child never follows a link of the image on the
/// host. A table whose root is not `root.path` is refused: the relative
/// path would name a directory of another tree.
pub(crate) fn userland_workdir(root: &RootDir, maps: &Maps, working_dir: &str) -> crate::Result<Option<CBuf>> {
    if maps.rootfs().trim_end_matches('/') != root.path.trim_end_matches('/') {
        return Err(Error::runtime(
            Class::Usage,
            format!(
                "the table of path maps has the root {:?}, and the entry holds the root {:?}",
                maps.rootfs(),
                root.path
            ),
        ));
    }
    let guest = if working_dir.is_empty() { "/" } else { working_dir };
    let (host, parts) = resolve_dir(maps, guest).map_err(|kind| {
        let volume = maps.volume_label(guest);
        refuse_workdir(
            working_dir,
            &format!("in the image's rootfs {:?} or its volumes", root.path),
            kind,
            volume.as_deref(),
        )
    })?;
    let text = if maps.in_volume(&parts) {
        host.display().to_string()
    } else if parts.is_empty() {
        return Ok(None);
    } else {
        parts.join("/")
    };
    CBuf::new(&text).map(Some).ok_or_else(|| Error::runtime(Class::Usage, "the working directory contains a NUL byte"))
}

#[cfg(test)]
mod tests;
