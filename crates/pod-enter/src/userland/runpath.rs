//! The directories of `DT_RUNPATH` and `DT_RPATH` of a program and of each
//! library that it loads, as guest paths, for the library path of the
//! loader family ([`super::lib_dirs_for`]).
//!
//! The loader of the image runs by host path with no chroot. It reads each
//! absolute directory of a `RUNPATH` as a host path, and the library path
//! that the entry gives it is the one place where a directory of the image
//! can stand in. The walk reads the files that the loader reads, in its
//! order: the program, then each `DT_NEEDED` name breadth first, each found
//! in the directories found so far and then in the default ones. Each
//! entry expands its tokens as the loader of the image does
//! ([`Tokens`]), and resolves inside the image through the table: a
//! directory that leaves the image, or that the image does not hold, is
//! not added.
//!
//! The list is one list for each object of the process: the loader
//! searches a `RUNPATH` for the needs of its own object only, and the
//! library path for each object. A directory of one library thus also
//! serves the others; a library that the loader would not find in the
//! image is found, never a library of the host.

use std::cell::OnceCell;
use std::collections::VecDeque;
use std::path::Path;

use crate::abi::{Flavour, GuestRoot, Search, resolve_dir_in, resolve_file};

/// The most files that the walk reads, and the most directories that it
/// gives: the image controls the files that it reads.
pub const MOST_OBJECTS: usize = 64;
pub const MOST_DIRS: usize = 64;

/// The guest directories of the `RUNPATH` (else `RPATH`) entries of the
/// program at `program_guest` and of each library that it loads, in the
/// order of the loader, each one that resolves inside `root`.
///
/// A program that does not read gives an empty list: the loader names its
/// own failure. A library of another machine is passed over, as the loader
/// passes over it. A relative entry is left out: the loader takes it from
/// the working directory, which is in the image already.
pub fn runpath_dirs<G: GuestRoot + ?Sized>(root: &G, interp_guest: &str, program_guest: &str) -> Vec<String> {
    let Ok((host, parts)) = resolve_file(root, program_guest) else { return Vec::new() };
    let Ok(program) = Search::read(&host.display().to_string()) else { return Vec::new() };
    let tokens = Tokens::new(root, interp_guest);
    let defaults = super::default_dirs(root, interp_guest);
    // The kernel gives the program the `$ORIGIN` of its file with each link
    // followed.
    let origin = format!("/{}", parts[..parts.len().saturating_sub(1)].join("/"));
    let machine = program.machine;
    let mut found: Vec<String> = Vec::new();
    let mut seen = vec![host];
    let mut queue = VecDeque::from([(program, origin)]);
    while let Some((object, origin)) = queue.pop_front() {
        for entry in object.dirs() {
            let Some(dir) = tokens.expand(entry, &origin) else { continue };
            if dir.starts_with('/')
                && found.len() < MOST_DIRS
                && !found.contains(&dir)
                && resolve_dir_in(root, &dir).is_ok()
            {
                found.push(dir);
            }
        }
        for name in &object.needed {
            if seen.len() >= MOST_OBJECTS {
                break;
            }
            if tokens.musl && crate::abi::musl_builtin(name) {
                continue;
            }
            let guests: Vec<String> = if name.starts_with('/') {
                vec![name.clone()]
            } else if name.contains('/') {
                Vec::new()
            } else {
                found.iter().chain(&defaults).map(|d| format!("{}/{name}", d.trim_end_matches('/'))).collect()
            };
            for guest in guests {
                let Ok((host, _)) = resolve_file(root, &guest) else { continue };
                if seen.contains(&host) {
                    break;
                }
                let Ok(lib) = Search::read(&host.display().to_string()) else { continue };
                if lib.machine != machine {
                    continue;
                }
                // The loader gives a library the directory of the path that
                // it opened, with no link followed.
                let origin = guest.rsplit_once('/').map(|(d, _)| d).unwrap_or_default();
                let origin = if origin.is_empty() { "/".to_string() } else { origin.to_string() };
                seen.push(host);
                queue.push_back((lib, origin));
                break;
            }
        }
    }
    found
}

/// The dynamic string tokens of the loader of one image.
///
/// `$ORIGIN` (or `${ORIGIN}`) is the guest directory of the object that
/// names it, for both C libraries. glibc also expands `$LIB` and
/// `$PLATFORM`; musl leaves them as they are, so an entry with one names
/// no directory there. The values of glibc come at the first entry that
/// needs them: most programs name neither.
pub struct Tokens<'a, G: GuestRoot + ?Sized> {
    root: &'a G,
    interp_guest: String,
    musl: bool,
    lib: OnceCell<Option<String>>,
    platform: OnceCell<Option<String>>,
}

impl<'a, G: GuestRoot + ?Sized> Tokens<'a, G> {
    /// The tokens of the loader at `interp_guest` in `root`.
    pub fn new(root: &'a G, interp_guest: &str) -> Tokens<'a, G> {
        Tokens {
            root,
            interp_guest: interp_guest.to_string(),
            musl: Flavour::of_libc_name(interp_guest) == Flavour::Musl,
            lib: OnceCell::new(),
            platform: OnceCell::new(),
        }
    }

    /// The entry `entry` of an object in the guest directory `origin`, with
    /// each token expanded. `None` where it names a token that has no value
    /// here: the loader drops such an entry or reads it as a path that does
    /// not exist. A `$` that starts no token stays, as the loader keeps it.
    pub fn expand(&self, entry: &str, origin: &str) -> Option<String> {
        let mut out = String::new();
        let mut rest = entry;
        while let Some(at) = rest.find('$') {
            out.push_str(&rest[..at]);
            let after = &rest[at + 1..];
            let (name, len) = match after.strip_prefix('{') {
                Some(b) => match b.find('}') {
                    Some(end) => (&b[..end], end + 2),
                    None => ("", 0),
                },
                None => {
                    let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
                    (&after[..end], end)
                }
            };
            let value = match name {
                "ORIGIN" => Some(origin.to_string()),
                "LIB" | "PLATFORM" if self.musl => return None,
                "LIB" => Some(self.lib.get_or_init(|| self.glibc_lib()).clone()?),
                "PLATFORM" => Some(self.platform.get_or_init(|| self.glibc_platform()).clone()?),
                _ => None,
            };
            match value {
                Some(v) => {
                    out.push_str(&v);
                    rest = &after[len..];
                }
                None => {
                    out.push('$');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        Some(out)
    }

    /// glibc's `$LIB`: the directory of its C library. Debian builds it as
    /// `lib/TRIPLET` for a multiarch directory (measured: glibc 2.39 of
    /// Ubuntu 24.04 gives `lib/x86_64-linux-gnu`); upstream gives the last
    /// component of that directory (`lib64` on Fedora, `lib` on Arch).
    fn glibc_lib(&self) -> Option<String> {
        let libc = self.libc()?;
        let guest = self.root.guest_of(Path::new(&libc))?;
        let dir = guest.rsplit_once('/')?.0;
        let last = dir.rsplit('/').next().filter(|l| !l.is_empty())?;
        Some(if last.contains("-linux-") { format!("lib/{last}") } else { last.to_string() })
    }

    /// glibc's `$PLATFORM` on this host ([`glibc_platform`]).
    fn glibc_platform(&self) -> Option<String> {
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        glibc_platform(&cpuinfo, self.declares_glibc_2_26(), at_platform)
    }

    /// The host path of the C library beside the loader.
    fn libc(&self) -> Option<String> {
        let loader = crate::abi::resolve_in(self.root, &self.interp_guest).ok()?;
        let machine = Search::read(&loader.display().to_string()).ok()?.machine;
        let rootfs = self.root.root().to_string_lossy().into_owned();
        crate::abi::libc_beside(&rootfs, &self.interp_guest, machine)
    }

    /// The C library declares `GLIBC_2.26`: the first glibc that gives an
    /// x86-64 CPU of the Haswell class its own `$PLATFORM`.
    fn declares_glibc_2_26(&self) -> bool {
        let Some(libc) = self.libc() else { return false };
        crate::abi::Elf::read(&libc).is_ok_and(|e| e.declares.iter().any(|v| v == "GLIBC_2.26"))
    }
}

/// glibc's `$PLATFORM` from the text of `/proc/cpuinfo`, the version rule
/// and a reader of `AT_PLATFORM`. On x86-64, glibc 2.26 and later give
/// `haswell` to an Intel CPU with AVX2, FMA, BMI1, BMI2, LZCNT (`abm`),
/// MOVBE and POPCNT (`init_cpu_features`); each other host takes the
/// `AT_PLATFORM` string of the kernel (`x86_64`, `aarch64`). The
/// `xeon_phi` of glibc (AVX512ER) is not read: no current CPU has it.
pub fn glibc_platform(cpuinfo: &str, glibc_2_26: bool, at_platform: fn() -> Option<String>) -> Option<String> {
    if cfg!(target_arch = "x86_64") && glibc_2_26 {
        let field = |name: &str| {
            cpuinfo
                .lines()
                .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim() == name).map(|(_, v)| v.trim().to_string()))
                .unwrap_or_default()
        };
        let flags = field("flags");
        let has = |f: &str| flags.split_whitespace().any(|x| x == f);
        let haswell = ["avx2", "fma", "bmi1", "bmi2", "abm", "movbe", "popcnt"].iter().all(|f| has(f));
        if field("vendor_id") == "GenuineIntel" && haswell {
            return Some("haswell".to_string());
        }
    }
    at_platform()
}

/// The `AT_PLATFORM` string of this process: its address from
/// `/proc/self/auxv`, its bytes from `/proc/self/mem`. `None` where the
/// kernel gives none (riscv64) or a read fails.
pub fn at_platform() -> Option<String> {
    use std::os::unix::fs::FileExt;
    const AT_PLATFORM: u64 = 15;
    let auxv = std::fs::read("/proc/self/auxv").ok()?;
    let word = std::mem::size_of::<usize>();
    let value = |b: &[u8]| -> u64 {
        let mut w = [0u8; 8];
        w[..word].copy_from_slice(b);
        if word == 8 { u64::from_ne_bytes(w) } else { u64::from(u32::from_ne_bytes([w[0], w[1], w[2], w[3]])) }
    };
    let at =
        auxv.chunks_exact(2 * word).map(|p| (value(&p[..word]), value(&p[word..]))).find(|(k, _)| *k == AT_PLATFORM)?.1;
    let mut buf = [0u8; 64];
    let n = std::fs::File::open("/proc/self/mem").ok()?.read_at(&mut buf, at).ok()?;
    let end = buf[..n].iter().position(|c| *c == 0)?;
    String::from_utf8(buf[..end].to_vec()).ok().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests;
