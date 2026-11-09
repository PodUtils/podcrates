use super::*;

/// The C library of a root file system, found from the interpreter of the
/// payload.
///
/// No table of the paths of distributions. `PT_INTERP` names the loader
/// that runs this payload. On musl that file is the C library; on glibc
/// the loader is usually in the directory of `libc.so.6`, so the libc is
/// the directory of the loader with that name. Both are paths inside
/// `rootfs`, so the caller reads them before the chroot.
///
/// Usually is not always. On a multiarch layout the loader is in `/lib64`
/// and `libc.so.6` is in `/lib/<triplet>/`, so the rule of the directory
/// finds nothing, and each payload of that image would be declined. The
/// fallback is a search with a bound, not a table: the usual library
/// roots, three levels deep, for a file named `libc.so.6` with the machine
/// of the payload. A multilib tree can hold more than one; the machine
/// decides, in the order of the paths.
pub fn libc_beside(rootfs: &str, interp: &str, machine: u16) -> Option<String> {
    let rel = interp.trim_start_matches('/');
    match Flavour::of_libc_name(rel) {
        Flavour::Musl => {
            let root = std::path::Path::new(rootfs);
            // The path as it is, the usual case. Where it is a link or is in
            // another place, resolve first: an absolute target of a link
            // stays under the root, as the kernel of the guest reads it.
            match resolve_in(root, interp) {
                Ok(p) => Some(p.display().to_string()),
                Err(ResolveKind::Absent) => {
                    let base = rel.rsplit('/').next().unwrap_or(rel);
                    search_file(rootfs.trim_end_matches('/'), base, None)
                }
                Err(_) => None,
            }
        }
        Flavour::Glibc => {
            let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let root = rootfs.trim_end_matches('/');
            // Resolved inside the root, as in the musl arm. An `exists()`
            // on the host follows an absolute link onto the host.
            let guest = format!("/{dir}/libc.so.6");
            if let Ok(p) = resolve_in(std::path::Path::new(root), &guest) {
                return Some(p.display().to_string());
            }
            search_file(root, "libc.so.6", Some(machine))
        }
        Flavour::Unknown => None,
    }
}

/// A file called `name` under the conventional library roots.
///
/// Two bounds: three directories deep and 512 entries, so a large tree
/// cannot make the check a long walk. The candidates come in the order of
/// their paths. `machine` filters multilib doubles where given; `None` takes
/// the first name match, which is exact for arch-named loaders.
fn search_file(root: &str, name: &str, machine: Option<u16>) -> Option<String> {
    const ROOTS: [&str; 4] = ["lib", "lib64", "usr/lib", "usr/lib64"];
    let mut found = Vec::new();
    let mut visited = 0usize;
    for r in ROOTS {
        let base = format!("{root}/{r}");
        let mut stack = vec![(base, 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if depth > 3 || visited > 512 {
                return first_match(root, &found, machine);
            }
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let mut names: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            names.sort_by_key(|e| e.file_name());
            for e in names {
                visited += 1;
                if visited > 512 {
                    return first_match(root, &found, machine);
                }
                let ft = match e.file_type() {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                // The walk follows no link: a loop of links would make the
                // search loop, and the read of the machine below follows a
                // link where it must.
                if ft.is_dir() {
                    stack.push((e.path().display().to_string(), depth + 1));
                } else if e.file_name().to_string_lossy().as_ref() == name {
                    found.push(e.path().display().to_string());
                }
            }
        }
    }
    first_match(root, &found, machine)
}

/// The first candidate matching `machine` where given, in path order.
///
/// The candidates resolve inside the root file system first. An absolute
/// link in an image names the host from outside and the image from inside:
/// the `/lib/ld-musl-x86_64.so.1` of Void points at `/usr/lib64/libc.so`,
/// a real file in the chroot and nothing on the host, so a direct read of
/// the link answers about the wrong machine. A link that escapes the root
/// is skipped, not followed.
///
/// Read, not executed: the header is 64 bytes and the machine field is at
/// byte 18. A file that does not parse is skipped, not trusted.
fn first_match(root: &str, found: &[String], machine: Option<u16>) -> Option<String> {
    let canon_root = std::fs::canonicalize(root).ok()?;
    let mut sorted = found.to_vec();
    sorted.sort();
    for p in sorted {
        // Inside the root or not at all: a link to nothing or out of the
        // root is skipped, and each other file is read where it resolves.
        let canon = match std::fs::canonicalize(&p) {
            Ok(c) if c.starts_with(&canon_root) => c,
            _ => continue,
        };
        if machine.is_none() {
            return Some(canon.display().to_string());
        }
        let bytes = match std::fs::read(&canon) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if bytes.len() < 64 || &bytes[0..4] != b"\x7fELF" {
            continue;
        }
        let m = match bytes[5] {
            1 => u16::from_le_bytes([bytes[18], bytes[19]]),
            2 => u16::from_be_bytes([bytes[18], bytes[19]]),
            _ => continue,
        };
        if Some(m) == machine {
            return Some(canon.display().to_string());
        }
    }
    None
}

/// How a guest path failed to resolve inside a root file system.
///
/// Three forms, not two: `Absent` is a usual miss along a search, and
/// `Escapes` and `Loop` are refusals that must show, not read as missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveKind {
    Absent,
    Escapes,
    Loop,
}

/// Resolve `guest` (a `/`-joined path as seen inside the root) to a host
/// path for reading, following symlinks the way the guest kernel would.
///
/// An absolute target of a link stays under the root. A direct read
/// follows it onto the host: an image that links its loader at an absolute
/// path whose target exists only in the chroot would have each payload
/// declined, and each one runs. A relative target resolves against the
/// directory of the link. A `..` past the root and a loop of links are
/// refused, not followed.
///
/// `root` must be a readable directory. Nothing is written and nothing
/// executes; the walk follows at most `MAX_LINKS` links.
pub fn resolve_in(root: &std::path::Path, guest: &str) -> Result<std::path::PathBuf, ResolveKind> {
    let full = walk_in(root, guest)?;
    match std::fs::symlink_metadata(&full) {
        Ok(m) if m.file_type().is_file() => Ok(full),
        _ => Err(ResolveKind::Absent),
    }
}

/// Resolve `guest` to a host path for a working directory, following
/// symlinks the way the guest kernel would.
///
/// The walk of the check of the working directory. One walk with
/// [`resolve_in`]; only the last check of the type is different (a
/// directory here, a file there), so a fix of the walk reaches both.
pub fn resolve_dir_in(root: &std::path::Path, guest: &str) -> Result<std::path::PathBuf, ResolveKind> {
    let full = walk_in(root, guest)?;
    match std::fs::symlink_metadata(&full) {
        Ok(m) if m.file_type().is_dir() => Ok(full),
        _ => Err(ResolveKind::Absent),
    }
}

/// The walk of the links that [`resolve_in`] and [`resolve_dir_in`] share.
///
/// Empty parts are skipped, so `"/tmp/"` and `"tmp"` resolve like
/// `"/tmp"`, and `"/"` resolves to the root itself. `..` past the root
/// escapes rather than resolving onto the host.
fn walk_in(root: &std::path::Path, guest: &str) -> Result<std::path::PathBuf, ResolveKind> {
    const MAX_LINKS: usize = 40;
    let mut parts: Vec<String> = guest.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect();
    let mut out: Vec<String> = Vec::new();
    let mut links = 0usize;
    let mut i = 0usize;
    while i < parts.len() {
        let p = parts[i].clone();
        if p == "." {
            i += 1;
            continue;
        }
        if p == ".." {
            if out.pop().is_none() {
                return Err(ResolveKind::Escapes);
            }
            i += 1;
            continue;
        }
        let mut probe = out.clone();
        probe.push(p.clone());
        let disk = probe.iter().fold(root.to_path_buf(), |a, c| a.join(c));
        let md = match std::fs::symlink_metadata(&disk) {
            Ok(m) => m,
            Err(_) => return Err(ResolveKind::Absent),
        };
        if md.file_type().is_symlink() {
            links += 1;
            if links > MAX_LINKS {
                return Err(ResolveKind::Loop);
            }
            let target = std::fs::read_link(&disk).map_err(|_| ResolveKind::Absent)?;
            let mut next = if target.is_absolute() { Vec::new() } else { out.clone() };
            out.clear();
            next.extend(target.to_string_lossy().split('/').filter(|s| !s.is_empty()).map(str::to_string));
            next.extend(parts[i + 1..].iter().cloned());
            parts = next;
            i = 0;
            continue;
        }
        out.push(p);
        i += 1;
    }
    Ok(out.iter().fold(root.to_path_buf(), |a, c| a.join(c)))
}
