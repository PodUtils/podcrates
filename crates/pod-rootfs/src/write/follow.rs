use super::*;

impl Root {
    /// Try to create a character device at `rel`.
    ///
    /// ⭐ **T-0401 calls this FIRST and shims only where it fails.** `mknod` is
    /// denied on the runtimes podvm is built for, and that denial is the whole
    /// premise of the shim; on a machine that permits it, a real `/dev/null` is
    /// what the payload should get, and reporting a shim there would be podvm
    /// inventing a degradation. ⛔ Probed by calling it, never inferred from a
    /// capability bit: this runtime hands a process every capability and denies
    /// the operation anyway.
    ///
    /// `Ok(true)` a device is now there, `Ok(false)` the kernel refused and the
    /// errno is in the string, `Err` podvm could not even try.
    pub fn mknod_char(
        &self,
        rel: &str,
        mode: u32,
        major: u64,
        minor: u64,
    ) -> Result<std::result::Result<(), String>> {
        let (parent, name) = match self.parent(rel, true)? {
            Ok(x) => x,
            Err(why) => return Ok(Err(why)),
        };
        let c = CBuf::new(&name).ok_or_else(|| Error::Complete(format!("{rel} contains a NUL")))?;
        let create = |name: &CBuf| {
            sys::mknodat(
                parent.fd(),
                name,
                sys::S_IFCHR | mode as u64,
                sys::makedev(major, minor),
            )
        };
        match create(&c) {
            Ok(_) => {
                let _ = sys::fchmodat(parent.fd(), &c, mode as u64, 0);
                return Ok(Ok(()));
            }
            Err(e) if e != sys::EEXIST => return Ok(Err(e.name())),
            Err(_) => {}
        }

        // An existing shim must survive when the kernel refuses `mknod`.
        // Create a sibling candidate first, then replace the old entry with one
        // fd-relative rename. The name includes both the process and a call id
        // so concurrent completion passes do not share a candidate.
        static CALL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let candidate = loop {
            let call = CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let candidate = CBuf::new(&format!(".podvm-mknod-{}-{call}", std::process::id()))
                .ok_or_else(|| {
                    Error::Complete("cannot construct the device candidate name".into())
                })?;
            match create(&candidate) {
                Ok(_) => break candidate,
                Err(e) if e == sys::EEXIST => continue,
                Err(e) => return Ok(Err(e.name())),
            }
        };
        let _ = sys::fchmodat(parent.fd(), &candidate, mode as u64, 0);
        match sys::renameat(parent.fd(), &candidate, parent.fd(), &c) {
            Ok(_) => Ok(Ok(())),
            Err(e) => {
                let _ = sys::unlinkat(parent.fd(), &candidate, 0);
                Ok(Err(format!(
                    "cannot replace the existing {rel} with a character device: {}",
                    e.name()
                )))
            }
        }
    }

    /// Open `rel` **following the last component**, confined to the rootfs.
    ///
    /// ⭐ **The opposite of [`Root::write`], and the difference is who owns the
    /// file.** `write` replaces: `/etc/mtab`, `/etc/resolv.conf`,
    /// `/etc/hosts` and `/etc/passwd` are files podvm owns, and following a
    /// link there is T-0405's escape. This one FOLLOWS: the CA trust store is
    /// the image's file and podvm is adding to it, so writing to the link
    /// rather than through it puts the bytes where nothing reads them.
    ///
    /// ⛔ Measured on 2026-09-09 against `registry.opensuse.org/opensuse/leap`:
    /// `/etc/ssl/ca-bundle.pem` is a symlink to
    /// `/var/lib/ca-certificates/ca-bundle.pem`, and replacing the link left
    /// curl's default trust store untouched -- `curl --cacert
    /// /etc/ssl/ca-bundle.pem` returned 200 on the same host where the default
    /// returned `self-signed certificate in certificate chain`.
    ///
    /// ⚠ ONE `openat2` from the rootfs descriptor with `RESOLVE_IN_ROOT`, so
    /// the kernel does the confinement: an absolute target rebases onto the
    /// rootfs and nothing resolves outside it. On a kernel without `openat2`
    /// this returns `None` and the caller reports a fixup it could not apply.
    fn open_following(&self, rel: &str, create: bool, mode: u32) -> Result<Option<i64>> {
        if !podvm_extract::safety::have_openat2() {
            return Ok(None);
        }
        // ⚠ Lexically checked first, exactly as every other path here is.
        Self::parts(rel)?;
        let Some(c) = CBuf::new(rel) else {
            return Ok(None);
        };
        let flags = if create {
            sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC | sys::O_CLOEXEC
        } else {
            sys::O_RDONLY | sys::O_CLOEXEC
        };
        let how = sys::OpenHow {
            flags,
            mode: mode as u64,
            resolve: sys::RESOLVE_IN_ROOT,
        };
        match sys::openat2(self.dir.fd(), &c, &how) {
            Ok(fd) => Ok(Some(fd)),
            Err(_) => Ok(None),
        }
    }

    /// Can podvm reach a DIRECTORY at `rel`, following the last component and
    /// staying inside the rootfs?
    ///
    /// ⭐ **A different question from `kind(rel) == Kind::Dir`, and the
    /// difference is two real images.** [`Root::kind`] does not follow the last
    /// component, so it answers `Symlink` for openSUSE's own
    /// `/etc/ssl/certs -> /var/lib/ca-certificates/pem` -- a directory podvm
    /// must write into -- and also for a link pointing OUT of the rootfs, which
    /// it must not. Measured on 2026-09-09 by
    /// `experiments/85-completion-symlink-escape.sh` door D and by the openSUSE
    /// row of `experiments/240-distro-sweep.sh`: a `Kind::Dir` test skipped both,
    /// silently, and one of them was the path the image's TLS stack reads.
    ///
    /// ⛔ `RESOLVE_IN_ROOT`, so a link out of the rootfs is REBASED rather than
    /// followed: `Some(false)` means "podvm cannot reach a directory there",
    /// which is what the caller has to report.
    ///
    /// ⚠ `None` where this kernel has no `openat2`, which is the third state
    /// again: podvm could not ask, and a caller must not read that as no.
    pub fn dir_following(&self, rel: &str) -> Result<Option<bool>> {
        if !podvm_extract::safety::have_openat2() {
            return Ok(None);
        }
        Self::parts(rel)?;
        let Some(c) = CBuf::new(rel) else {
            return Ok(Some(false));
        };
        let how = sys::OpenHow {
            flags: sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC,
            mode: 0,
            resolve: sys::RESOLVE_IN_ROOT,
        };
        match sys::openat2(self.dir.fd(), &c, &how) {
            Ok(fd) => {
                let _ = sys::close(fd);
                Ok(Some(true))
            }
            Err(_) => Ok(Some(false)),
        }
    }

    /// Read `rel`, following the last component. See [`Root::open_following`].
    pub fn read_following(&self, rel: &str) -> Result<Option<Vec<u8>>> {
        const CEILING: usize = 8 * 1024 * 1024;
        let Some(fd) = self.open_following(rel, false, 0)? else {
            return Ok(None);
        };
        let mut out = Vec::new();
        let mut buf = [0u8; 65536];
        loop {
            match sys::read(fd, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    out.extend_from_slice(&buf[..n as usize]);
                    if out.len() > CEILING {
                        let _ = sys::close(fd);
                        return Err(Error::Complete(format!(
                            "{rel} is larger than {CEILING} bytes"
                        )));
                    }
                }
                Err(e) if e == sys::EINTR => continue,
                Err(_) => break,
            }
        }
        let _ = sys::close(fd);
        Ok(Some(out))
    }

    /// Write `rel`, following the last component. See [`Root::open_following`].
    ///
    /// ⚠ `None` where the kernel cannot do it safely, so the caller reports a
    /// fixup it could not apply rather than one it silently skipped.
    pub fn write_following(&self, rel: &str, bytes: &[u8], mode: u32) -> Result<Option<Wrote>> {
        let existed = self.read_following(rel)?;
        if existed.as_deref() == Some(bytes) {
            return Ok(Some(Wrote::Unchanged));
        }
        let Some(fd) = self.open_following(rel, true, mode)? else {
            return Ok(None);
        };
        let mut off = 0;
        while off < bytes.len() {
            match sys::write(fd, &bytes[off..]) {
                Ok(0) => break,
                Ok(n) => off += n as usize,
                Err(e) if e == sys::EINTR => continue,
                Err(e) => {
                    let _ = sys::close(fd);
                    return Err(Error::Complete(format!("cannot write {rel}: {}", e.name())));
                }
            }
        }
        let _ = sys::fchmod(fd, mode as u64);
        let _ = sys::close(fd);
        Ok(Some(match existed {
            None => Wrote::Created,
            Some(_) => Wrote::Replaced,
        }))
    }

    /// Make `rel` a directory, and every parent of it.
    pub fn mkdirs(&self, rel: &str) -> Result<()> {
        let parts = Self::parts(rel)?;
        let mut all: Vec<&str> = parts.clone();
        // ⚠ `open_parent` creates the parents and hands back the last
        // component, so the directory itself is one more `mkdirat`.
        all.push(".");
        match safety::open_parent_with(&self.dir, &all, true, Resolve::InRoot)
            .map_err(|e| Error::Complete(format!("{rel}: {e}")))?
        {
            Ok(_) => Ok(()),
            Err(r) => Err(Error::Complete(format!("{rel}: {}", r.why()))),
        }
    }

    /// Every name directly inside `rel`, sorted, or an empty list where `rel` is
    /// not a directory podvm can reach.
    pub fn list(&self, rel: &str) -> Result<Vec<String>> {
        let parts = Self::parts(rel)?;
        let mut all: Vec<&str> = parts.clone();
        all.push(".");
        let dir = match safety::open_parent_with(&self.dir, &all, false, Resolve::InRoot) {
            Ok(Ok((d, _))) => d,
            // ⚠ A directory podvm cannot reach lists as empty rather than
            // failing: most of these directories are absent in most images.
            Ok(Err(_)) | Err(_) => return Ok(Vec::new()),
        };
        let mut names: Vec<String> = sys::getdents64(dir.fd())
            .map_err(|e| Error::Complete(format!("cannot list {rel}: {}", e.name())))?
            .into_iter()
            .map(|d| String::from_utf8_lossy(&d.name).to_string())
            .filter(|n| n != "." && n != "..")
            .collect();
        names.sort();
        Ok(names)
    }

    /// Remove `rel` if it is there, never following the last component.
    pub fn unlink(&self, rel: &str) -> Result<bool> {
        let Ok((parent, name)) = self.parent(rel, false)? else {
            return Ok(false);
        };
        let Some(c) = CBuf::new(&name) else {
            return Ok(false);
        };
        match sys::unlinkat(parent.fd(), &c, 0) {
            Ok(_) => Ok(true),
            Err(e) if e == sys::ENOENT => Ok(false),
            Err(e) => Err(Error::Complete(format!(
                "cannot remove {rel}: {}",
                e.name()
            ))),
        }
    }
}
