use super::*;

/// The walk of [`Resolve::InRoot`]: a different shape, not the same walk with
/// another flag.
///
/// **`RESOLVE_IN_ROOT` takes THE DESCRIPTOR THAT IT GETS as `/`.** A walk of
/// one component at a time moves that root at each step: after a descent into
/// `etc`, an absolute link `/var/aptreal` rebases onto `<rootfs>/etc` and
/// gives `ENOENT` (the shape of `/etc/ssl/certs ->
/// /var/lib/ca-certificates/pem` on openSUSE).
///
/// So each open here starts from the **descriptor of the root file system**,
/// with the path until now, and that descriptor does not move. The cost is
/// quadratic in the depth of the path, and the depth is four or five: this
/// walk is for configuration directories, not for a tree.
pub(super) fn in_root_parent(
    root: &Dir,
    dirs: &[&str],
    last: &str,
    make_dirs: bool,
) -> Result<std::result::Result<(Dir, String), Refusal>> {
    if !have_openat2() {
        // `RESOLVE_IN_ROOT` has no `O_NOFOLLOW` equal. To do it in user space
        // is to resolve each link here and walk again, in a race with each
        // other writer of the tree. The caller gets a refusal that it can
        // report, not an approximation that nobody measured.
        return Ok(Err(Refusal::Escapes(
            "this kernel has no openat2(2), so the crate cannot follow a symlinked \
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
        Err(e) => return Err(error::sys("cannot re-open the destination", e)),
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
                // Made in the RESOLVED parent, by its name. The name does not
                // exist, so the make goes through no link.
                let c = cbuf(d)?;
                // A finished root keeps the modes of the image: a closed
                // parent (0555) is opened for this one make.
                let open = OwnerBits::add(cur.fd());
                match sys::mkdirat(cur.fd(), &c, 0o755) {
                    Ok(_) => {}
                    Err(e) if e == sys::EEXIST => {}
                    Err(e) => return Err(error::sys(format!("cannot create the directory {d:?}"), e)),
                }
                drop(open);
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
        let h =
            sys::OpenHow { flags: sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC, mode: 0, resolve: how.flags() };
        sys::openat2(d.fd(), &c, &h)
    } else {
        sys::openat(d.fd(), &c, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC, 0)
    }
    .map_err(|e| error::sys("cannot re-open the destination", e))?;
    Ok(Dir(fd))
}

/// One component of the walk: descend into `name`, optionally creating it.
pub(super) fn step(cur: &Dir, name: &str, make: bool, how: Resolve) -> Result<std::result::Result<Dir, Refusal>> {
    let c = cbuf(name)?;
    // The directory is made BEFORE the descent: a layer often names
    // `usr/bin/x` with no entry for `usr/bin`. `EEXIST` is the usual case and
    // not an error here; the descent below refuses a symlink, whatever made it.
    if make {
        match sys::mkdirat(cur.fd(), &c, 0o755) {
            Ok(_) => {}
            Err(e) if e == sys::EEXIST => {}
            Err(e) => return Err(error::sys(format!("cannot create the directory {name:?}"), e)),
        }
    }
    let flags = sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC;
    let r = if how.use_openat2() {
        let h = sys::OpenHow { flags, mode: 0, resolve: how.flags() };
        sys::openat2(cur.fd(), &c, &h)
    } else if how == Resolve::InRoot {
        // **`RESOLVE_IN_ROOT` is of `openat2` and has no `O_NOFOLLOW`
        // equal.** To do it by hand is to resolve each symlink in this process,
        // rebase an absolute target onto the root and walk again: a race with
        // each other writer of the tree, which the descriptor walk prevents.
        // So on a kernel with no `openat2` this REFUSES and does not
        // approximate: the caller records a step that it could not apply, with
        // the reason, a third state and not a silent failure.
        return Ok(Err(Refusal::Escapes(format!(
            "{name:?}: this component is reached through a symlink and this kernel \
             has no openat2(2), so the crate cannot follow it while proving it stays \
             inside the rootfs"
        ))));
    } else {
        // The walk, on `O_NOFOLLOW`. One component at a time from a held
        // descriptor: `..` cannot come (the lexical check removed it) and a
        // symlink cannot be crossed (this flag refuses it with ELOOP).
        sys::openat(cur.fd(), &c, flags | sys::O_NOFOLLOW, 0)
    };
    match r {
        Ok(fd) => Ok(Ok(Dir(fd))),
        // The finding, in three errnos: the resolution left the destination
        // or crossed a symlink. ELOOP is the refusal of O_NOFOLLOW (and of
        // RESOLVE_NO_SYMLINKS), EXDEV the refusal of RESOLVE_BENEATH, and
        // ENOTDIR a component that is a file.
        Err(e) if e == sys::ELOOP || e == sys::EXDEV || e == sys::ENOTDIR => {
            Ok(Err(Refusal::Escapes(format!("{name:?}: {}", e.name()))))
        }
        Err(e) => Err(error::sys(format!("cannot descend into {name:?}"), e)),
    }
}
