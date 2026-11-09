use super::*;

/// Where a rootfs's C library is, found from the payload's own interpreter.
///
/// ⭐ **No table of distribution paths.** `PT_INTERP` names the loader that will
/// run this payload. On musl that file IS the C library; on glibc the loader
/// usually lives in the same directory as `libc.so.6`, so the libc is the
/// loader's own directory plus that name. ⚠ Both are returned as paths INSIDE
/// `rootfs`, so the caller reads them before the chroot.
///
/// ⚠ **Usually is not always.** On a multiarch layout the loader sits in
/// `/lib64` while `libc.so.6` sits in `/lib/<triplet>/`, so the directory
/// rule finds nothing and every payload of that image would be declined.
/// The fallback is a bounded structural search, not a table: the conventional
/// library roots, three deep, for a file named exactly `libc.so.6` with the
/// payload's own machine. A multilib tree can hold several; the machine
/// decides, deterministically by path order.
pub fn libc_beside(rootfs: &str, interp: &str, machine: u16) -> Option<String> {
    let rel = interp.trim_start_matches('/');
    match Flavour::of_libc_name(rel) {
        Flavour::Musl => {
            let root = std::path::Path::new(rootfs);
            // ⭐ The verbatim path is the common case. Where it is a link or
            // lives elsewhere, resolve first: an absolute link target stays
            // under the root, the way the guest kernel would read it.
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
            // ⭐ T-1644: resolved inside the root, as the musl arm is. A
            // host `exists()` follows an absolute link onto the host.
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
/// Bounded twice: three directories deep and 512 entries visited, so a large
/// tree cannot turn classification into a walk. Deterministic: candidates in
/// path order. `machine` filters multilib doubles where given; `None` takes
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
                // ⚠ No symlink following while walking: a link cycle would
                // loop the search, and the machine read below follows one
                // level where it has to.
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
/// ⭐ Candidates resolve INSIDE the rootfs first. An absolute symlink in an
/// image names the host from outside and the image from inside: void's
/// `/lib/ld-musl-x86_64.so.1` points at `/usr/lib64/libc.so`, which is a
/// real file in the chroot and nothing on the host, so reading the link
/// directly answers about the wrong machine. A link escaping the root is
/// skipped rather than followed.
///
/// ⚠ Read, not executed: the header is 64 bytes and the machine field two
/// bytes into it. A file that does not parse is skipped rather than trusted.
fn first_match(root: &str, found: &[String], machine: Option<u16>) -> Option<String> {
    let canon_root = std::fs::canonicalize(root).ok()?;
    let mut sorted = found.to_vec();
    sorted.sort();
    for p in sorted {
        // ⭐ Inside the root or not at all: a dangling or escaping link is
        // skipped, and what remains is read from where it resolves.
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

/// How a guest path failed to resolve inside a rootfs.
///
/// ⛔ Three shapes and not two: `Absent` is an ordinary miss along a search,
/// while `Escapes` and `Loop` are refusals that must surface rather than read
/// as missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveKind {
    Absent,
    Escapes,
    Loop,
}

/// Resolve `guest` (a `/`-joined path as seen inside the root) to a host
/// path for reading, following symlinks the way the guest kernel would.
///
/// ⭐ An absolute link target stays UNDER the root. Reading it directly
/// follows it onto the host instead: one matrix row links its loader at an
/// absolute path whose target exists only in the chroot, so the naive read
/// declined every payload of that image while each one ran fine. Relative
/// targets resolve against the link's own directory. `..` past the root and
/// a link loop are refused rather than followed.
///
/// # Safety
/// `root` must be a readable directory. Nothing is written and nothing is
/// executed; the walk is bounded by `MAX_LINKS` replacements.
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
/// TODO/enter.md T-1412: the workdir check's walk. One walk with
/// [`resolve_in`], differing only in the final type check (a directory
/// here, a file there), so a fix to the walk reaches both.
pub fn resolve_dir_in(
    root: &std::path::Path,
    guest: &str,
) -> Result<std::path::PathBuf, ResolveKind> {
    let full = walk_in(root, guest)?;
    match std::fs::symlink_metadata(&full) {
        Ok(m) if m.file_type().is_dir() => Ok(full),
        _ => Err(ResolveKind::Absent),
    }
}

/// The symlink walk [`resolve_in`] and [`resolve_dir_in`] share.
///
/// Empty parts are skipped, so `"/tmp/"` and `"tmp"` resolve like
/// `"/tmp"`, and `"/"` resolves to the root itself. `..` past the root
/// escapes rather than resolving onto the host.
fn walk_in(root: &std::path::Path, guest: &str) -> Result<std::path::PathBuf, ResolveKind> {
    const MAX_LINKS: usize = 40;
    let mut parts: Vec<String> = guest
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
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
            let mut next = if target.is_absolute() {
                Vec::new()
            } else {
                out.clone()
            };
            out.clear();
            next.extend(
                target
                    .to_string_lossy()
                    .split('/')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            );
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
