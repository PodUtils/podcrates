use super::*;

/// What [`Store::delete`] does about an image a holder is using.
///
/// ⛔ Two behaviours and one check, because the check has to happen under the
/// index lock (invariant I4) and only the caller knows whether being held is a
/// refusal (`rmi`, which names one image) or a skip (`prune`, which sweeps).
/// Public for the same reason `delete` is: the CLI's record-gate
/// (TODO/image.md T-1322) deletes the unreferenced remainder through it.
#[derive(Debug, Clone, Copy)]
pub enum Held {
    Refuse,
    Skip,
}

#[derive(Debug)]
pub struct Removed {
    pub untagged: Vec<String>,
    pub deleted: Vec<String>,
    pub freed_bytes: u64,
    /// ⛔ Named, never silent. T-0204: `prune` and `rmi` skip anything locked
    /// **and say which**.
    pub skipped: Vec<String>,
}

/// A staging file, held exclusively for as long as this value lives.
///
/// ⛔ **Invariant I5's mechanism.** [`Store::sweep_staging`] decides what is
/// abandoned by trying to lock each `*.partial`, so a file being written has to
/// be locked or the sweep would delete it out from under its writer. The lock
/// goes when this value does, however the process ends.
///
/// ⚠ **Two open file descriptions on one inode, not one.** `flock` is held on
/// the description [`Lock`] opened, and the writes go through a second one this
/// type opens after it. They are independent: closing the writer releases no
/// lock, and the lock's own `Drop` is the only thing that does. ⛔ So a
/// `StagedFile` costs two descriptors, and a reader counting descriptors while
/// chasing a lock that outlived its holder has to know that
/// ([`TODO/image.md`](../../../TODO/image.md) T-0215).
pub struct StagedFile {
    file: std::fs::File,
    /// ⚠ Held for its `Drop`, and never read. The lock is the point.
    _lock: Lock,
}

impl StagedFile {
    pub(super) fn create(path: &Path) -> Result<StagedFile> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(parent.display().to_string(), e))?;
        }
        // ⛔ The lock FIRST. Creating the file and locking it afterwards leaves
        // a window in which a sweep sees an unlocked `*.partial` and removes it.
        let lock = match Lock::try_acquire(path, sys::LOCK_EX)? {
            Some(l) => l,
            None => {
                return Err(Error::Store(format!(
                    "{} is already being written by another podvm. Invariant I6 \
                     makes this name unique per call, so two writers on one name is \
                     a defect rather than contention",
                    path.display()
                )))
            }
        };
        let file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(|e| Error::io(path.display().to_string(), e))?;
        Ok(StagedFile { file, _lock: lock })
    }

    pub fn sync_all(&self) -> std::io::Result<()> {
        self.file.sync_all()
    }
}

impl Write for StagedFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// ⭐ [`TODO/image.md`](../../../TODO/image.md) T-0214. A transfer that was cut
/// short starts again on this same file, under the same lock and at the same
/// name: the alternative is a second staging path per attempt, and a sweep that
/// has to tell them apart.
///
/// ⛔ Truncate AND rewind. `set_len(0)` alone leaves the offset where the failed
/// attempt left it, so the next byte lands two megabytes in and the file is a
/// hole followed by the retry.
impl crate::registry::Restart for StagedFile {
    fn restart(&mut self) -> std::io::Result<()> {
        use std::io::Seek;
        self.file.set_len(0)?;
        self.file.seek(std::io::SeekFrom::Start(0))?;
        Ok(())
    }
}

/// An advisory lock held for as long as this value lives.
pub struct Lock {
    pub(super) fd: i64,
    pub path: PathBuf,
    /// ⛔ Set by [`Lock::hand_to_payload`], and it turns the explicit release in
    /// `Drop` OFF. A handed lock is meant to outlive this process in the
    /// payload, and `LOCK_UN` would take it from the payload as well: the
    /// payload holds a duplicate of THIS open file description, and a lock
    /// belongs to the description rather than to a descriptor.
    handed: std::sync::atomic::AtomicBool,
}

impl Lock {
    fn open(path: &Path) -> Result<i64> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(parent.display().to_string(), e))?;
        }
        let Some(c) = CBuf::new(&path.to_string_lossy()) else {
            return Err(Error::Store(format!(
                "{} contains a NUL and cannot reach the kernel",
                path.display()
            )));
        };
        // ⛔ `O_CLOEXEC`, like every other fd in this tree. T-0204's mechanism
        // wants the image lock to survive **one** exec, and T-0211 is what it
        // cost to get that by leaving the fd inheritable from the moment it was
        // opened: every unrelated fork in between inherits it too.
        // [`Lock::hand_to_payload`] is the deliberate act, made one call before
        // the `execve` it is for.
        let flags = sys::O_RDWR | sys::O_CREAT | sys::O_CLOEXEC;
        sys::open(&c, flags, 0o644).map_err(|e| {
            Error::Store(format!(
                "opening the lock {}: {} ({})",
                path.display(),
                e.name(),
                e.0
            ))
        })
    }

    /// `None` where somebody else holds it. ⛔ Always `LOCK_NB`: a blocking
    /// `flock` on a lock held through an exec is an unbounded wait, which
    /// `RULES.md` section 8 forbids.
    /// An exclusive lock, or `None` where somebody holds it.
    ///
    /// ⚠ Public because `podvm-supervise` reconciles its container table with
    /// exactly this question ([`TODO/supervise.md`](../../../TODO/supervise.md)
    /// T-0604): a launcher holds its container's lock for its whole life, so a
    /// lock another process can take is a launcher that is gone. ⛔ The same
    /// mechanism and not a second one: a pid file would be stale the moment a
    /// launcher is killed, and the check that clears a stale one is the race.
    pub fn try_exclusive(path: &Path) -> Result<Option<Lock>> {
        Lock::try_acquire(path, sys::LOCK_EX)
    }

    /// Take an exclusive lock, waiting the bounded number of attempts.
    pub fn exclusive(path: &Path) -> Result<Lock> {
        Lock::acquire(path, sys::LOCK_EX)
    }

    /// ⛔ **Every lock this type takes is registered for shedding here, and
    /// this is the only place a `Lock` is built.** A caller cannot forget,
    /// because a caller is not asked.
    /// [`TODO/image.md`](../../../TODO/image.md) T-0215 is what the previous
    /// shape cost: only [`Store::hold`] registered, so a staging lock, an index
    /// lock and a container lock were each inherited by every child a
    /// concurrent `clone_fork` made, and the child then held the `flock` for
    /// its own lifetime. The image read as in use after its holder released it.
    ///
    /// ⚠ The `Lock` is built BEFORE the registration is checked, so the fd is
    /// closed by its own `Drop` on the refusal path rather than leaked.
    pub(super) fn try_acquire(path: &Path, op: u64) -> Result<Option<Lock>> {
        let fd = Lock::open(path)?;
        match sys::flock(fd, op | sys::LOCK_NB) {
            Ok(_) => {
                let lock = Lock {
                    fd,
                    path: path.to_path_buf(),
                    handed: std::sync::atomic::AtomicBool::new(false),
                };
                if !sys::close_in_children(fd) {
                    return Err(Error::Store(format!(
                        "this process already holds {} locks, which is every slot \
                         podvm has for fds a fork must shed. One more would leak \
                         into every child forked from here (T-0211), so it is \
                         refused rather than held unsafely",
                        sys::FORK_CLOSE_SLOTS
                    )));
                }
                Ok(Some(lock))
            }
            Err(sys::EWOULDBLOCK) => {
                // ⛔ **THE ONLY MOMENT A HOLDER WOULD BE THERE.**
                // [`TODO/image.md`](../../../TODO/image.md) T-0215: every
                // instrument that ran from the failing assertion arrived
                // microseconds late and reported "nobody". This runs at the
                // `EWOULDBLOCK` itself. ⚠ Test builds only, and silent unless
                // `PODVM_T0215_CAPTURE` is set.
                #[cfg(test)]
                tests::note_the_refusal(path, fd);
                let _ = sys::close(fd);
                Ok(None)
            }
            Err(e) => {
                let _ = sys::close(fd);
                Err(Error::Store(format!(
                    "flock({}): {} ({})",
                    path.display(),
                    e.name(),
                    e.0
                )))
            }
        }
    }

    pub(super) fn acquire(path: &Path, op: u64) -> Result<Lock> {
        for _ in 0..LOCK_ATTEMPTS {
            if let Some(l) = Lock::try_acquire(path, op)? {
                return Ok(l);
            }
            std::thread::sleep(LOCK_SLEEP);
        }
        Err(Error::Store(format!(
            "{} is held by another podvm after {:?}. Another pull or prune is \
             running; podvm waits a bounded time and then says so rather than \
             blocking (RULES.md section 8)",
            path.display(),
            LOCK_SLEEP * LOCK_ATTEMPTS
        )))
    }

    /// Hand this one lock to the payload, and to nothing else.
    ///
    /// ⭐ T-0204 needs the image lock to outlive `podvm` itself: the guard is
    /// what stops a concurrent `rmi` or `prune` deleting a rootfs a running
    /// container is executing out of, and podvm is not the process that holds
    /// the container open. This undoes both of [`Store::hold`]'s defences for
    /// this one descriptor, it stops being shed by `clone_fork` and its
    /// `FD_CLOEXEC` is cleared, so the very next `fork` and `execve` carry it
    /// into the payload.
    ///
    /// ⛔ Called immediately before the fork that leads to that `execve`, never
    /// at open time. T-0211 is what the second shape costs: a lock that is
    /// inheritable for its whole life is inherited by every unrelated `fork` in
    /// that window, in this tree, by the fifty short-lived children one
    /// `podvm probe` makes, and each one holds the `flock` open for its own
    /// lifetime. The image then reads as in use after its holder released it,
    /// and `rmi` refuses an image nothing is using.
    ///
    /// ⚠ The returned fd is deliberately raw: its consumer is the code between
    /// `fork` and `execve`, where allocating is not allowed.
    pub fn hand_to_payload(&self) -> Result<i64> {
        // ⛔ Order matters. The fd stops being shed only after `FD_CLOEXEC` is
        // cleared, so a fork racing this call either sheds it or inherits a
        // descriptor that is still close-on-exec. Neither outcome leaks a lock.
        let flags = sys::fcntl(self.fd, sys::F_GETFD, 0).map_err(|e| {
            Error::Store(format!(
                "reading the descriptor flags of {}: {} ({})",
                self.path.display(),
                e.name(),
                e.0
            ))
        })?;
        let cleared = (flags as u64) & !sys::FD_CLOEXEC;
        sys::fcntl(self.fd, sys::F_SETFD, cleared).map_err(|e| {
            Error::Store(format!(
                "clearing FD_CLOEXEC on {}: {} ({})",
                self.path.display(),
                e.name(),
                e.0
            ))
        })?;
        sys::stop_closing_in_children(self.fd);
        // ⛔ And this one is never released explicitly. `Drop` calls `LOCK_UN`
        // on every other lock so the release finishes in the releasing thread
        // (T-0215), and doing it here would take the lock away from the payload
        // that is meant to keep it: the payload holds a duplicate of THIS open
        // file description, and a lock belongs to the description.
        self.handed
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(self.fd)
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // ⛔ Deregistered before the close, so a fork racing this never sheds a
        // descriptor number that has already been handed back to the kernel and
        // reused by another thread.
        sys::stop_closing_in_children(self.fd);

        // ⛔ **THE RELEASE IS EXPLICIT, AND `close` ALONE WAS THE DEFECT.**
        // [`TODO/image.md`](../../../TODO/image.md) T-0215. A `close` releases
        // the lock only when it drops the LAST reference to the open file
        // description. A `fork` makes a second reference, so after one the
        // holder's own `close` no longer completes the release: the lock record
        // is taken away later, when the child's copy goes, and that happens
        // asynchronously. ⚠ Measured on 2026-09-12: a `flock` refused with NO
        // row in `/proc/locks` and NO descriptor on the inode in any process on
        // the host, and the SAME descriptor succeeded on the next attempt, 1 us
        // later. The worst transient ran 1190 us. `in_use` read those as an
        // image in use after its holder released it.
        // ⭐ `LOCK_UN` removes the record HERE, in this thread, whatever else
        // holds a reference, so the release is finished when this line is.
        if !self.handed.load(std::sync::atomic::Ordering::Acquire) {
            let _ = sys::flock(self.fd, sys::LOCK_UN);
        }

        // ⚠ The close still matters and is not replaced: it is what makes the
        // lock correct across an unexpected death, where no `Drop` runs at all.
        let _ = sys::close(self.fd);
    }
}

/// Build the record a completed pull writes.
#[allow(clippy::too_many_arguments)]
pub fn record_of(
    reference: &Reference,
    resolved: &Digest,
    resolved_media_type: &str,
    manifest_digest: &Digest,
    manifest: &oci::Manifest,
    config: &oci::Config,
    platform: &str,
) -> Result<Record> {
    Ok(Record {
        repository: reference.canonical_repository(),
        tag: reference.tag.clone(),
        digest: resolved.to_string(),
        digest_media_type: resolved_media_type.to_string(),
        manifest_digest: manifest_digest.to_string(),
        config_digest: manifest.config.parsed_digest()?.to_string(),
        platform: platform.to_string(),
        layers: manifest.layers.iter().map(|l| l.digest.clone()).collect(),
        stored_bytes: manifest.stored_bytes(),
        architecture: config.architecture.clone(),
        os: config.os.clone(),
        created: config.created.clone(),
        pulled_at: clock::now(),
    })
}
