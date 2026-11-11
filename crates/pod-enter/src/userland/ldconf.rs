//! The library directories that the image's own loader configuration names
//! (T-182, PodUtils/podvm#247): glibc's `/etc/ld.so.conf` with its
//! `include` globs, and musl's `/etc/ld-musl-ARCH.path`. On this rung the
//! loader runs on the host, so it reads the configuration of the host,
//! never the image's: the entry reads the image's here, inside the image
//! (each path is resolved in the table, so a link cannot point out of it),
//! and gives its directories on the library path. Each read has a bound.

use crate::abi::{GuestRoot, resolve_dir_in, resolve_in};

/// At most this many configuration files are read (the first and each
/// file of an `include`).
pub const MOST_FILES: usize = 32;

/// At most this depth of `include` under the first file.
pub const MOST_DEPTH: usize = 4;

/// At most this many directories come from the configuration.
pub const MOST_DIRS: usize = 64;

/// A configuration file over this size is not read.
pub const MOST_BYTES: u64 = 64 * 1024;

/// The guest directories that the image's loader configuration names, in
/// the order of the files: `/etc/ld-musl-ARCH.path` for a musl loader
/// (`ld-musl-ARCH.so.1`), else `/etc/ld.so.conf`. A file that is absent,
/// too large or not text gives nothing.
pub fn configured_dirs<G: GuestRoot + ?Sized>(root: &G, interp_guest: &str) -> Vec<String> {
    let base = interp_guest.rsplit('/').next().unwrap_or(interp_guest);
    let mut dirs = Vec::new();
    if let Some(arch) = base.strip_prefix("ld-musl-").and_then(|r| r.strip_suffix(".so.1")) {
        // musl reads one directory for each line, or between colons.
        if let Some(text) = read(root, &format!("/etc/ld-musl-{arch}.path")) {
            text.split(['\n', ':']).for_each(|d| push(&mut dirs, d));
        }
    } else {
        let mut files = 0;
        conf(root, "/etc/ld.so.conf", 0, &mut files, &mut dirs);
    }
    dirs
}

fn push(dirs: &mut Vec<String>, dir: &str) {
    let dir = dir.trim().trim_end_matches('/');
    if dir.starts_with('/') && dirs.len() < MOST_DIRS && !dirs.iter().any(|d| d == dir) {
        dirs.push(dir.to_string());
    }
}

/// The text of the guest file `guest`, resolved inside the image.
fn read<G: GuestRoot + ?Sized>(root: &G, guest: &str) -> Option<String> {
    let host = resolve_in(root, guest).ok()?;
    if std::fs::metadata(&host).ok()?.len() > MOST_BYTES {
        return None;
    }
    std::fs::read_to_string(&host).ok()
}

/// One file of the glibc format: a directory on each line (an old
/// `DIR=TYPE` names a type after `=`), `include PATTERN...` (a relative
/// pattern is relative to the directory of the file), `hwcap` lines, and
/// `#` comments.
fn conf<G: GuestRoot + ?Sized>(root: &G, guest: &str, depth: usize, files: &mut usize, dirs: &mut Vec<String>) {
    if depth > MOST_DEPTH || *files >= MOST_FILES {
        return;
    }
    *files += 1;
    let Some(text) = read(root, guest) else { return };
    let here = guest.rsplit_once('/').map_or("", |(d, _)| d);
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let mut words = line.split_whitespace();
        match words.next() {
            None | Some("hwcap") => {}
            Some("include") => {
                for pattern in words {
                    let pattern =
                        if pattern.starts_with('/') { pattern.to_string() } else { format!("{here}/{pattern}") };
                    for file in glob(root, &pattern) {
                        conf(root, &file, depth + 1, files, dirs);
                    }
                }
            }
            Some(_) => {
                for word in line.split(|c: char| c.is_whitespace() || c == ',' || c == ':') {
                    push(dirs, word.split('=').next().unwrap_or_default());
                }
            }
        }
    }
}

/// The guest files that `pattern` names, sorted as glob(3) sorts them. A
/// wildcard (`*`, `?`) counts only in the last component; a name that
/// starts with a dot matches only a pattern that starts with one.
fn glob<G: GuestRoot + ?Sized>(root: &G, pattern: &str) -> Vec<String> {
    let Some((dir, name)) = pattern.rsplit_once('/') else { return Vec::new() };
    if dir.contains(['*', '?']) {
        return Vec::new();
    }
    if !name.contains(['*', '?']) {
        return vec![pattern.to_string()];
    }
    let Ok(host) = resolve_dir_in(root, if dir.is_empty() { "/" } else { dir }) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(host) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| (!n.starts_with('.') || name.starts_with('.')) && matches(name.as_bytes(), n.as_bytes()))
        .collect();
    names.sort();
    names.truncate(MOST_FILES);
    names.into_iter().map(|n| format!("{dir}/{n}")).collect()
}

/// `*` matches any run of bytes, `?` one byte, each other byte itself.
fn matches(pattern: &[u8], name: &[u8]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(&c) if c == b'?' || c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    p = sp + 1;
                    n = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests;
