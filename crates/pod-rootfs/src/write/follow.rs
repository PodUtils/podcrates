use super::*;

/// The resolve of a path held in the root, with a follow of the last
/// component.
fn in_root(flags: u64, mode: u32) -> sys::OpenHow {
    sys::OpenHow { flags, mode: mode as u64, resolve: sys::RESOLVE_IN_ROOT }
}

/// The text of a refusal on a kernel with no `openat2`.
fn no_openat2(rel: &str) -> String {
    format!(
        "this kernel has no openat2(2), so the crate cannot follow a symlink at {rel} while it proves that the \
         path stays inside the root"
    )
}

impl Root {
    /// Try to make a character device at `rel`.
    ///
    /// The completion calls this FIRST and makes a shim only where it fails:
    /// on a host that permits `mknod`, a real `/dev/null` is what the payload
    /// gets. The crate calls it, and does not read a capability bit: a
    /// runtime can give each capability and still refuse the call.
    ///
    /// The node must also open: on a file system mounted `nodev` a node is
    /// made but each open of it fails, and a payload that writes to it gets
    /// `EACCES`. A node that does not open is removed. The mode is set
    /// through the open descriptor, never by name.
    ///
    /// `Ok(Ok(()))`: a device is there. `Ok(Err(why))`: the kernel refused,
    /// and `why` names the errno. `Err`: the crate could not try.
    pub fn mknod_char(
        &self,
        rel: &str,
        mode: u32,
        major: u64,
        minor: u64,
        scratch_prefix: &str,
    ) -> Result<std::result::Result<(), String>> {
        if let Some(l) = self.two() {
            return match l.upper_parent(rel, true)? {
                Ok((up, name)) => mknod_in(up.0, rel, &name, mode, major, minor, scratch_prefix),
                Err(why) => Ok(Err(why)),
            };
        }
        let (parent, name) = match self.parent(rel, true)? {
            Ok(x) => x,
            Err(why) => return Ok(Err(why)),
        };
        mknod_in(parent.fd(), rel, &name, mode, major, minor, scratch_prefix)
    }

    /// Does the character device at `rel` open? A node on a file system
    /// mounted `nodev` does not.
    pub fn char_opens(&self, rel: &str) -> Result<bool> {
        if let Some(l) = self.two() {
            return Ok(l.open_read(rel, false)?.is_some());
        }
        let Ok((parent, name)) = self.parent(rel, false)? else {
            return Ok(false);
        };
        let c = cbuf(&name)?;
        Ok(sys::openat(parent.fd(), &c, READ_FLAGS | sys::O_NOFOLLOW, 0).map(|fd| drop(Fd(fd))).is_ok())
    }

    /// Open `rel` **with a follow of the last component**, held in the root.
    ///
    /// The opposite of [`Root::write`], and the difference is the owner of
    /// the file. `write` replaces: `/etc/mtab`, `/etc/resolv.conf`,
    /// `/etc/hosts` and `/etc/passwd` belong to the completion, and a follow
    /// there is the escape. This one FOLLOWS: the CA trust store belongs to
    /// the image and the crate adds to it, and a write to the link and not
    /// through it puts the bytes where nothing reads them
    /// (`/etc/ssl/ca-bundle.pem -> /var/lib/ca-certificates/ca-bundle.pem` on
    /// openSUSE).
    ///
    /// ONE `openat2` from the descriptor of the root with `RESOLVE_IN_ROOT`,
    /// so the kernel does the confinement: an absolute target rebases onto
    /// the root. `Ok(Err(why))`: the crate cannot open it (no `openat2`, or
    /// the kernel refused).
    fn open_following(&self, rel: &str, flags: u64, mode: u32) -> Result<std::result::Result<Fd, String>> {
        Self::parts(rel)?;
        if !safety::have_openat2() {
            return Ok(Err(no_openat2(rel)));
        }
        let c = cbuf(rel)?;
        Ok(sys::openat2(self.fd(), &c, &in_root(flags, mode)).map(Fd).map_err(|e| format!("{rel}: {}", e.name())))
    }

    /// Can the crate reach a DIRECTORY at `rel`, with a follow of the last
    /// component, held in the root?
    ///
    /// Another question than `kind(rel) == Kind::Dir`: [`Root::kind`]
    /// answers `Symlink` for openSUSE's `/etc/ssl/certs ->
    /// /var/lib/ca-certificates/pem`, a directory that the crate must write
    /// into, and also for a link OUT of the root, which it must not follow.
    /// `RESOLVE_IN_ROOT` rebases a link out of the root, so `Some(false)`
    /// means "no directory there". `None`: this kernel has no `openat2`, and
    /// the crate could not ask.
    pub fn dir_following(&self, rel: &str) -> Result<Option<bool>> {
        if let Some(l) = self.two() {
            return Ok(Some(l.dir(rel)?.is_some()));
        }
        Self::parts(rel)?;
        if !safety::have_openat2() {
            return Ok(None);
        }
        let c = cbuf(rel)?;
        let flags = sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC;
        Ok(Some(sys::openat2(self.fd(), &c, &in_root(flags, 0)).map(|fd| drop(Fd(fd))).is_ok()))
    }

    /// Read the regular file at `rel`, with a follow of the last component.
    /// `None`: no file, not a regular file, or no `openat2`.
    pub fn read_following(&self, rel: &str) -> Result<Option<Vec<u8>>> {
        if let Some(l) = self.two() {
            return match l.open_read(rel, true)? {
                Some(fd) => read_regular(&fd, rel),
                None => Ok(None),
            };
        }
        match self.open_following(rel, READ_FLAGS, 0)? {
            Ok(fd) => read_regular(&fd, rel),
            Err(_) => Ok(None),
        }
    }

    /// Write the regular file at `rel`, with a follow of the last component.
    /// `Ok(Err(why))`: the crate cannot write it safely (no `openat2`, a
    /// refused resolution, or not a regular file), and the caller reports a
    /// fixup that it could not apply.
    pub fn write_following(&self, rel: &str, bytes: &[u8], mode: u32) -> Result<std::result::Result<Wrote, String>> {
        let existed = self.read_following(rel)?;
        if existed.as_deref() == Some(bytes) {
            return Ok(Ok(Wrote::Unchanged));
        }
        // `O_NONBLOCK`: no wait for a reader of a fifo. `O_TRUNC` does
        // nothing to a fifo or a device, and the type check below refuses
        // both before a byte goes out.
        let flags = sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC | sys::O_NONBLOCK | sys::O_NOCTTY | sys::O_CLOEXEC;
        let opened = match self.two() {
            // The copy of the upper layer: a file of the lower layer is
            // copied up first.
            Some(l) => l.open_write(rel, flags, mode)?,
            None => self.open_following(rel, flags, mode)?,
        };
        let fd = match opened {
            Ok(fd) => fd,
            Err(why) => return Ok(Err(why)),
        };
        if fstat(&fd, rel)?.st_mode & sys::S_IFMT != sys::S_IFREG {
            return Ok(Err(format!("{rel} is not a regular file; the crate does not write into it")));
        }
        write_all(&fd, bytes, rel)?;
        sys::fchmod(fd.0, mode as u64).map_err(|e| error::sys(format!("cannot set the mode of {rel}"), e))?;
        Ok(Ok(match existed {
            None => Wrote::Created,
            Some(_) => Wrote::Replaced,
        }))
    }

    /// Make `rel` a directory, and each parent of it.
    pub fn mkdirs(&self, rel: &str) -> Result<()> {
        if let Some(l) = self.two() {
            return l.mkdirs(rel);
        }
        let mut all = Self::parts(rel)?;
        // `open_parent_with` makes the parents and gives back the last
        // component, so the directory itself is one more component.
        all.push(".");
        match safety::open_parent_with(&self.dir, &all, true, Resolve::InRoot)? {
            Ok(_) => Ok(()),
            Err(r) => Err(error::policy(format!("{rel}: {}", r.why()))),
        }
    }

    /// Each name directly in `rel`, sorted, or an empty list where `rel` is
    /// not a directory that the crate can reach: most of these directories
    /// are absent in most images.
    pub fn list(&self, rel: &str) -> Result<Vec<String>> {
        if let Some(l) = self.two() {
            return l.list(rel);
        }
        let mut all = Self::parts(rel)?;
        all.push(".");
        let dir = match safety::open_parent_with(&self.dir, &all, false, Resolve::InRoot) {
            Ok(Ok((d, _))) => d,
            Ok(Err(_)) | Err(_) => return Ok(Vec::new()),
        };
        let mut names: Vec<String> = sys::getdents64(dir.fd())
            .map_err(|e| error::sys(format!("cannot list {rel}"), e))?
            .into_iter()
            .map(|d| String::from_utf8_lossy(&d.name).to_string())
            .filter(|n| n != "." && n != "..")
            .collect();
        names.sort();
        Ok(names)
    }

    /// Remove `rel` if it is there, with no follow of the last component.
    /// `Ok(false)`: nothing there, or its directory is out of reach.
    ///
    /// On two layers, an entry that the lower layer holds is refused with
    /// [`LayerRefusal::Remove`]: only a whiteout removes it.
    pub fn unlink(&self, rel: &str) -> Result<bool> {
        if let Some(l) = self.two() {
            return l.unlink(rel);
        }
        let Ok((parent, name)) = self.parent(rel, false)? else {
            return Ok(false);
        };
        let c = cbuf(&name)?;
        match sys::unlinkat(parent.fd(), &c, 0) {
            Ok(_) => Ok(true),
            Err(e) if e == sys::ENOENT => Ok(false),
            Err(e) => Err(error::sys(format!("cannot remove {rel}"), e)),
        }
    }

    /// Make a symlink at `rel` to `target`. `Ok(Err(why))`: its directory is
    /// out of reach, or the kernel refused.
    pub fn symlink(&self, rel: &str, target: &str) -> Result<std::result::Result<(), String>> {
        if let Some(l) = self.two() {
            return l.symlink(rel, target);
        }
        let (parent, name) = match self.parent(rel, false)? {
            Ok(x) => x,
            Err(why) => return Ok(Err(why)),
        };
        let (c, t) = (cbuf(&name)?, cbuf(target)?);
        Ok(sys::symlinkat(&t, parent.fd(), &c).map(|_| ()).map_err(|e| e.name()))
    }
}

/// Make the character device `name` in `parent` ([`Root::mknod_char`]).
fn mknod_in(
    parent: i64,
    rel: &str,
    name: &str,
    mode: u32,
    major: u64,
    minor: u64,
    scratch_prefix: &str,
) -> Result<std::result::Result<(), String>> {
    let c = cbuf(name)?;
    let dev = sys::makedev(major, minor);
    // A new node that does not settle is removed, and the refusal names why.
    let settled = |name: &CBuf| {
        settle_node(parent, name, mode, major, minor).inspect_err(|_| {
            let _ = sys::unlinkat(parent, name, 0);
        })
    };
    match sys::mknodat(parent, &c, sys::S_IFCHR | mode as u64, dev) {
        Ok(_) => return Ok(settled(&c)),
        Err(e) if e != sys::EEXIST => return Ok(Err(e.name())),
        Err(_) => {}
    }

    // An existing entry stays when the kernel refuses `mknod`: the crate
    // makes a node under a temporary name first, then puts it in place with
    // one rename. The name holds the process and a count, so two completions
    // at the same time do not share it.
    static CALL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let candidate = loop {
        let call = CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let candidate = cbuf(&format!("{scratch_prefix}{}-{call}", std::process::id()))?;
        match sys::mknodat(parent, &candidate, sys::S_IFCHR | mode as u64, dev) {
            Ok(_) => match settled(&candidate) {
                Ok(()) => break candidate,
                Err(why) => return Ok(Err(why)),
            },
            Err(e) if e == sys::EEXIST => continue,
            Err(e) => return Ok(Err(e.name())),
        }
    };
    match sys::renameat(parent, &candidate, parent, &c) {
        Ok(_) => Ok(Ok(())),
        Err(e) => {
            let _ = sys::unlinkat(parent, &candidate, 0);
            Ok(Err(format!("cannot replace the existing {rel} with a character device: {}", e.name())))
        }
    }
}

/// Open a node that `mknodat` just made, check it, and set its mode through
/// the descriptor. A node that does not open is of no use to the payload.
fn settle_node(parent: i64, name: &CBuf, mode: u32, major: u64, minor: u64) -> std::result::Result<(), String> {
    let fd = Fd(sys::openat(parent, name, READ_FLAGS | sys::O_NOFOLLOW, 0).map_err(|e| {
        format!("the node was made and does not open ({}), as on a file system mounted nodev", e.name())
    })?);
    let empty = CBuf::new("").ok_or("an empty name")?;
    let st = sys::fstatat(fd.0, &empty, sys::AT_EMPTY_PATH).map_err(|e| e.name())?;
    if st.st_mode & sys::S_IFMT != 0o020000 || split_dev(st.st_rdev) != (major, minor) {
        return Err(format!("the node at the name is not the character device {major}:{minor}"));
    }
    sys::fchmod(fd.0, mode as u64).map_err(|e| e.name())?;
    Ok(())
}
