//! The split of a host path at its base, and the opens under a descriptor.

use crate::{AT_FDCWD, BENEATH, Calls, O_CLOEXEC, O_DIRECTORY, O_PATH, OpenHow, Refusal};
use core::ffi::{CStr, c_int};
use linux_raw_sys::errno::EAGAIN;

/// The tries of one open that the kernel answers with `EAGAIN`. With
/// `RESOLVE_BENEATH` the kernel gives `EAGAIN` where a rename or a mount
/// anywhere on the system raced a `..` of the walk; a busy host renames
/// often, and the next try sees a quiet tree.
pub const EAGAIN_TRIES: usize = 256;

/// `s` with no `/` at its end; `/` itself stays.
fn trim_end(mut s: &[u8]) -> &[u8] {
    while s.len() > 1 && s.ends_with(b"/") {
        s = &s[..s.len() - 1];
    }
    s
}

/// The path of `host` under `base`, with no `/` at its start: `.` for the
/// base itself. A lexical test: the walk then proves it.
///
/// `base` is absolute, or [`Refusal::BaseNotAbsolute`]. A `host` that is
/// not absolute, or not at or under `base`, is [`Refusal::NotUnderBase`].
/// A `..` stays in the result: the kernel refuses it where it goes above
/// the base ([`Refusal::DotDot`]).
pub fn split<'a>(base: &[u8], host: &'a [u8]) -> Result<&'a [u8], Refusal> {
    if base.first() != Some(&b'/') {
        return Err(Refusal::BaseNotAbsolute);
    }
    if host.first() != Some(&b'/') {
        return Err(Refusal::NotUnderBase);
    }
    let base = trim_end(base);
    // The root as the base: each absolute path is under it.
    let base: &[u8] = if base == b"/" { b"" } else { base };
    let rest = host.strip_prefix(base).ok_or(Refusal::NotUnderBase)?;
    if !rest.is_empty() && rest[0] != b'/' {
        return Err(Refusal::NotUnderBase);
    }
    let start = rest.iter().position(|&c| c != b'/').unwrap_or(rest.len());
    let rest = &rest[start..];
    Ok(if rest.is_empty() { b"." } else { rest })
}

/// The directory and the last name of `rel`: `("a/b", "c")` for `a/b/c`,
/// `(".", "c")` for `c`, `("a", "")` for `a/`. The last name can be `.` or
/// `..`; [`open_parent_beneath`] opens such a path whole.
pub fn last_name(rel: &[u8]) -> (&[u8], &[u8]) {
    match rel.iter().rposition(|&c| c == b'/') {
        // Only a leading `/` gives an empty directory: it stays, and the
        // kernel refuses an absolute path under a descriptor.
        Some(i) => {
            let dir = trim_end(&rel[..i]);
            (if dir.is_empty() { b"/" } else { dir }, &rel[i + 1..])
        }
        None => (b".", rel),
    }
}

/// `path` with its NUL in `buf`.
fn with_nul<'b>(path: &[u8], buf: &'b mut [u8]) -> Result<&'b CStr, Refusal> {
    if path.contains(&0) {
        return Err(Refusal::Nul);
    }
    if path.len() >= buf.len() {
        return Err(Refusal::TooLong);
    }
    buf[..path.len()].copy_from_slice(path);
    buf[path.len()] = 0;
    CStr::from_bytes_with_nul(&buf[..=path.len()]).map_err(|_| Refusal::Nul)
}

/// One `openat2`, tried again while the kernel answers `EAGAIN`.
fn call<C: Calls + ?Sized>(c: &C, dirfd: c_int, path: &CStr, how: &OpenHow) -> Result<c_int, c_int> {
    let mut r = Err(EAGAIN as c_int);
    for _ in 0..EAGAIN_TRIES {
        r = c.openat2(dirfd, path, how);
        if r != Err(EAGAIN as c_int) {
            break;
        }
    }
    r
}

/// `rel` opened under the directory `dirfd` with
/// `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`: no link on the way or at the
/// last name, and no step above `dirfd`. `flags` and `mode` are those of
/// `open(2)`; the open adds `O_CLOEXEC`. An empty `rel` is `.`. The
/// descriptor is the caller's.
///
/// `O_CREAT` is safe here: a link that another process plants at the last
/// name is [`Refusal::Link`], not a file out of the directory.
pub fn open_beneath<C: Calls + ?Sized>(
    c: &C,
    dirfd: c_int,
    rel: &[u8],
    flags: u64,
    mode: u64,
    buf: &mut [u8],
) -> Result<c_int, Refusal> {
    let rel: &[u8] = if rel.is_empty() { b"." } else { rel };
    let path = with_nul(rel, buf)?;
    let how = OpenHow { flags: flags | O_CLOEXEC, mode, resolve: BENEATH };
    call(c, dirfd, path, &how).map_err(Refusal::of_walk)
}

/// `host` opened under `base`: the base by its path (it can hold links: the
/// caller chose it), then the rest by [`open_beneath`] under the base with
/// `flags`. The base descriptor is closed before the return; the descriptor
/// of `host` is the caller's.
///
/// [`Refusal::NotUnderBase`]: the base does not apply to `host`, and
/// nothing opened.
pub fn open_under_base<C: Calls + ?Sized>(
    c: &C,
    base: &[u8],
    host: &[u8],
    flags: u64,
    buf: &mut [u8],
) -> Result<c_int, Refusal> {
    let rel = split(base, host)?;
    // Each path must fit before the base opens: a refusal opens nothing.
    with_nul(rel, buf)?;
    let path = with_nul(base, buf)?;
    let how = OpenHow { flags: O_PATH | O_DIRECTORY | O_CLOEXEC, mode: 0, resolve: 0 };
    let dir = call(c, AT_FDCWD, path, &how).map_err(Refusal::of_base)?;
    let r = open_beneath(c, dir, rel, flags, 0, buf);
    c.close(dir);
    r
}

/// The directory of the last name of `rel`, opened under `dirfd` as
/// [`open_beneath`] does (`O_PATH | O_DIRECTORY`), and that last name: for
/// a call that makes the file of the name or does not follow a link at it.
/// The caller then names the place by the descriptor and the name, and
/// adds `O_NOFOLLOW` (or `AT_SYMLINK_NOFOLLOW`) to its own call: a link
/// that another process plants at the name then fails that call.
///
/// Where the last name is empty, `.` or `..`, the descriptor is of `rel`
/// itself and the name is empty.
pub fn open_parent_beneath<'a, C: Calls + ?Sized>(
    c: &C,
    dirfd: c_int,
    rel: &'a [u8],
    buf: &mut [u8],
) -> Result<(c_int, &'a [u8]), Refusal> {
    let (dir, name) = last_name(rel);
    if name.is_empty() || name == b"." || name == b".." {
        return open_beneath(c, dirfd, rel, O_PATH | O_DIRECTORY, 0, buf).map(|fd| (fd, &b""[..]));
    }
    open_beneath(c, dirfd, dir, O_PATH | O_DIRECTORY, 0, buf).map(|fd| (fd, name))
}
