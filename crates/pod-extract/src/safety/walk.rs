use super::*;

/// [`Resolve::InRoot`]'s own walk, and it is a different shape rather than the
/// same one with another flag.
///
/// ⛔ **`RESOLVE_IN_ROOT` treats THE DESCRIPTOR IT IS GIVEN as `/`.** Stepping
/// one component at a time moves that root with every step: after descending
/// into `etc`, an absolute link `/var/aptreal` rebases onto `<rootfs>/etc` and
/// answers `ENOENT`. Measured on 2026-09-09 by
/// `experiments/85-completion-symlink-escape.sh` door F, which is openSUSE's
/// own `/etc/ssl/certs -> /var/lib/ca-certificates/pem` shape.
///
/// So every open here is from the **rootfs descriptor**, with the accumulated
/// path, and that descriptor never moves. ⚠ Quadratic in the depth of the path
/// and the depth is four or five: a directory this walks is a configuration
/// directory, not a tree.
pub(super) fn in_root_parent(
    root: &Dir,
    dirs: &[&str],
    last: &str,
    make_dirs: bool,
) -> Result<std::result::Result<(Dir, String), Refusal>> {
    if !have_openat2() {
        // ⛔ There is no `O_NOFOLLOW` equivalent of `RESOLVE_IN_ROOT`, and
        // re-implementing it in user space means resolving each link here and
        // re-walking, which races anything else writing the tree. A caller gets
        // a refusal it can report rather than an approximation nobody measured.
        return Ok(Err(Refusal::Escapes(
            "this kernel has no openat2(2), so podvm cannot follow a symlinked \
             directory while proving it stays inside the rootfs"
                .to_string(),
        )));
    }
    let open_at_root = |rel: &str| -> Result<std::result::Result<Dir, Errno>> {
        let c = cbuf(rel)?;
        let how = sys::OpenHow {
            flags: sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC,
            mode: 0,
            resolve: sys::RESOLVE_IN_ROOT,
        };
        Ok(sys::openat2(root.fd(), &c, &how).map(Dir))
    };
    let mut cur = match open_at_root(".")? {
        Ok(d) => d,
        Err(e) => {
            return Err(Error::Extract(format!(
                "cannot re-open the destination: {}",
                e.name()
            )))
        }
    };
    let mut acc = String::new();
    for d in dirs {
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(d);
        match open_at_root(&acc)? {
            Ok(next) => cur = next,
            Err(e) if e == sys::ENOENT && make_dirs => {
                // ⚠ Created against the RESOLVED parent, by bare name. The name
                // does not exist, so no link can be traversed by creating it.
                let c = cbuf(d)?;
                match sys::mkdirat(cur.fd(), &c, 0o755) {
                    Ok(_) => {}
                    Err(e) if e == sys::EEXIST => {}
                    Err(e) => {
                        return Err(Error::Extract(format!(
                            "cannot create the directory {d:?}: {}",
                            e.name()
                        )))
                    }
                }
                match open_at_root(&acc)? {
                    Ok(next) => cur = next,
                    Err(e) => return Ok(Err(Refusal::Escapes(format!("{d:?}: {}", e.name())))),
                }
            }
            Err(e) => return Ok(Err(Refusal::Escapes(format!("{d:?}: {}", e.name())))),
        }
    }
    Ok(Ok((cur, last.to_string())))
}

pub(super) fn dup_dir(d: &Dir, how: Resolve) -> Result<Dir> {
    let c = cbuf(".")?;
    let fd = if how.use_openat2() {
        let h = sys::OpenHow {
            flags: sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC,
            mode: 0,
            resolve: how.flags(),
        };
        sys::openat2(d.fd(), &c, &h)
    } else {
        sys::openat(
            d.fd(),
            &c,
            sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC,
            0,
        )
    }
    .map_err(|e| Error::Extract(format!("cannot re-open the destination: {}", e.name())))?;
    Ok(Dir(fd))
}

/// One component of the walk: descend into `name`, optionally creating it.
pub(super) fn step(
    cur: &Dir,
    name: &str,
    make: bool,
    how: Resolve,
) -> Result<std::result::Result<Dir, Refusal>> {
    let c = cbuf(name)?;
    // ⚠ The directory is created BEFORE the descent is attempted, because a
    // layer names `usr/bin/x` with no entry for `usr/bin` more often than not.
    // `EEXIST` is the ordinary case and is not an error here; what matters is
    // that the descent below refuses a symlink whatever created it.
    if make {
        match sys::mkdirat(cur.fd(), &c, 0o755) {
            Ok(_) => {}
            Err(e) if e == sys::EEXIST => {}
            Err(e) => {
                return Err(Error::Extract(format!(
                    "cannot create the directory {name:?}: {}",
                    e.name()
                )))
            }
        }
    }
    let flags = sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC;
    let r = if how.use_openat2() {
        let h = sys::OpenHow {
            flags,
            mode: 0,
            resolve: how.flags(),
        };
        sys::openat2(cur.fd(), &c, &h)
    } else if how == Resolve::InRoot {
        // ⛔ **`RESOLVE_IN_ROOT` IS `openat2`'s AND HAS NO `O_NOFOLLOW`
        // EQUIVALENT.** Re-implementing it by hand means resolving each symlink
        // in this process, rebasing an absolute target onto the root and
        // re-walking -- a race against anything else writing the tree, which is
        // exactly what the descriptor walk exists to avoid. So on a kernel
        // without `openat2` this REFUSES rather than approximating: the caller
        // records the fixup as one it could not apply and says why, which is a
        // third state and not a silent failure.
        return Ok(Err(Refusal::Escapes(format!(
            "{name:?}: this component is reached through a symlink and this kernel \
             has no openat2(2), so podvm cannot follow it while proving it stays \
             inside the rootfs"
        ))));
    } else {
        // ⛔ The fallback, and `O_NOFOLLOW` is what carries it. Opening one
        // component at a time from a held descriptor means `..` cannot appear
        // (the lexical check removed it) and a symlink cannot be traversed
        // (this flag refuses it with ELOOP).
        sys::openat(cur.fd(), &c, flags | sys::O_NOFOLLOW, 0)
    };
    match r {
        Ok(fd) => Ok(Ok(Dir(fd))),
        // ⭐ THE FINDING, and the three errnos that carry it. The kernel is
        // saying the resolution left the destination or crossed a symlink.
        // ELOOP is O_NOFOLLOW's refusal, EXDEV is RESOLVE_BENEATH's, and
        // ENOTDIR is a component that is a file.
        Err(e) if e == Errno(40) || e == Errno(18) || e == Errno(20) => {
            Ok(Err(Refusal::Escapes(format!("{name:?}: {}", e.name()))))
        }
        Err(e) => Err(Error::Extract(format!(
            "cannot descend into {name:?}: {}",
            e.name()
        ))),
    }
}
