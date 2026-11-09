//! An advisory lock (`flock`) that a value holds for as long as it lives.
//!
//! A launcher holds the lock of its container for its whole life, so a lock
//! that another process can take is a launcher that is gone
//! ([`crate::table::reconcile`]). A pid file cannot do this: it is stale
//! when its launcher is killed, and the check that clears a stale one is a
//! race.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_hostprobe::sys::{self, CBuf, Errno};

use crate::Result;

/// How many times [`Lock::acquire`] tries a lock that another process holds.
pub const ATTEMPTS: u32 = 100;
/// The pause between two tries of [`Lock::acquire`].
pub const PAUSE: Duration = Duration::from_millis(50);

/// An advisory lock held for as long as this value lives.
pub struct Lock {
    fd: i64,
    pub path: PathBuf,
    /// Set by [`Lock::hand_to_payload`]. It turns the explicit release in
    /// `Drop` off: a handed lock must outlive this process in the payload,
    /// and `LOCK_UN` would take it from the payload too. The payload holds a
    /// duplicate of this open file description, and a lock belongs to the
    /// description, not to a descriptor.
    handed: AtomicBool,
}

/// A failure of a system call on the lock at `path`.
fn errno_failure(what: &str, path: &Path, e: Errno) -> Failure {
    let cause = io::Error::from_raw_os_error(e.0);
    Failure::caused(
        Stage::Host,
        Class::of_io(cause.kind()),
        format!("{what} {}: {} ({})", path.display(), e.name(), e.0),
        cause,
    )
}

impl Lock {
    fn open(path: &Path) -> Result<i64> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::io_at(parent, e))?;
        }
        let Some(c) = CBuf::new(&path.to_string_lossy()) else {
            return Err(Failure::new(
                Stage::Host,
                Class::Usage,
                format!("{} contains a NUL and cannot reach the kernel", path.display()),
            ));
        };
        // `O_CLOEXEC`: an inheritable lock is inherited by each unrelated
        // fork, and the child then holds the `flock` for its own life.
        // [`Lock::hand_to_payload`] is the one deliberate exception, made one
        // call before the `execve` that it is for.
        let flags = sys::O_RDWR | sys::O_CREAT | sys::O_CLOEXEC;
        sys::open(&c, flags, 0o644).map_err(|e| errno_failure("opening the lock", path, e))
    }

    /// An exclusive lock, or `None` when another process holds it. Always
    /// `LOCK_NB`: a blocking `flock` on a lock held through an exec is a wait
    /// with no bound.
    pub fn try_exclusive(path: &Path) -> Result<Option<Lock>> {
        Lock::try_acquire(path, sys::LOCK_EX)
    }

    /// An exclusive lock, with the bounded number of tries of
    /// [`Lock::acquire`].
    pub fn exclusive(path: &Path) -> Result<Lock> {
        Lock::acquire(path, sys::LOCK_EX)
    }

    /// The lock of `op` (`sys::LOCK_EX` or `sys::LOCK_SH`), or `None` when
    /// another process holds it.
    pub fn try_acquire(path: &Path, op: u64) -> Result<Option<Lock>> {
        Lock::try_acquire_noting(path, op, &|_, _| {})
    }

    /// [`Lock::try_acquire`], and `note(path, fd)` at the refusal, before
    /// the descriptor closes: an instrument that looks for the holder must
    /// look while the refusal is true.
    ///
    /// Each lock is registered here to be closed in each child that
    /// `clone_fork` makes, and this is the only place that builds a `Lock`,
    /// so a caller cannot forget it. An unregistered lock is inherited by
    /// each concurrent fork, and the child then holds the `flock`: the lock
    /// reads as held after its holder released it.
    pub fn try_acquire_noting(path: &Path, op: u64, note: &dyn Fn(&Path, i64)) -> Result<Option<Lock>> {
        let fd = Lock::open(path)?;
        match sys::flock(fd, op | sys::LOCK_NB) {
            Ok(_) => {
                // The `Lock` exists before the registration is checked, so
                // its own `Drop` closes the descriptor on the refusal.
                let lock = Lock { fd, path: path.to_path_buf(), handed: AtomicBool::new(false) };
                if !sys::close_in_children(fd) {
                    return Err(Failure::new(
                        Stage::Host,
                        Class::Unavailable,
                        format!(
                            "this process already holds {} locks, which is each slot for the \
                             descriptors that a fork must close. One more would leak into each \
                             child, so the lock is refused",
                            sys::FORK_CLOSE_SLOTS
                        ),
                    ));
                }
                Ok(Some(lock))
            }
            Err(sys::EWOULDBLOCK) => {
                note(path, fd);
                let _ = sys::close(fd);
                Ok(None)
            }
            Err(e) => {
                let _ = sys::close(fd);
                Err(errno_failure("flock", path, e))
            }
        }
    }

    /// The lock of `op`, tried [`ATTEMPTS`] times with [`PAUSE`] between.
    pub fn acquire(path: &Path, op: u64) -> Result<Lock> {
        Lock::acquire_noting(path, op, &|_, _| {})
    }

    /// [`Lock::acquire`], with the `note` of [`Lock::try_acquire_noting`].
    pub fn acquire_noting(path: &Path, op: u64, note: &dyn Fn(&Path, i64)) -> Result<Lock> {
        for _ in 0..ATTEMPTS {
            if let Some(l) = Lock::try_acquire_noting(path, op, note)? {
                return Ok(l);
            }
            std::thread::sleep(PAUSE);
        }
        Err(Failure::new(
            Stage::Host,
            Class::TempFail,
            format!(
                "{} is held by another process after {:?}. The wait has a bound and does not \
                 block: try again when that process ends",
                path.display(),
                PAUSE * ATTEMPTS
            ),
        ))
    }

    /// The descriptor of the lock.
    pub fn fd(&self) -> i64 {
        self.fd
    }

    /// Give this lock to the payload, and to nothing else.
    ///
    /// The lock must outlive the caller when the payload is the process
    /// that uses what the lock guards (a root file system that the payload
    /// runs from). This clears `FD_CLOEXEC` and stops the close in each
    /// child, so the next `fork` and `execve` carry the lock into the
    /// payload.
    ///
    /// Call it immediately before the fork that leads to that `execve`,
    /// never at open time: a lock that is inheritable for its whole life is
    /// inherited by each unrelated fork in that time, and each child holds
    /// the `flock` for its own life.
    ///
    /// The descriptor that it returns is raw: its user is the code between
    /// `fork` and `execve`, which must not allocate.
    pub fn hand_to_payload(&self) -> Result<i64> {
        // The order is necessary. The descriptor stops being closed in each
        // child only after `FD_CLOEXEC` is clear, so a fork in between closes
        // it or inherits a descriptor that is still close-on-exec. Neither
        // leaks a lock.
        let flags = sys::fcntl(self.fd, sys::F_GETFD, 0)
            .map_err(|e| errno_failure("reading the descriptor flags of", &self.path, e))?;
        let cleared = (flags as u64) & !sys::FD_CLOEXEC;
        sys::fcntl(self.fd, sys::F_SETFD, cleared)
            .map_err(|e| errno_failure("clearing FD_CLOEXEC on", &self.path, e))?;
        sys::stop_closing_in_children(self.fd);
        // `Drop` does not release this lock: the payload holds a duplicate
        // of this open file description, and a lock belongs to the
        // description.
        self.handed.store(true, Ordering::Release);
        Ok(self.fd)
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Out of the list before the close, so that a fork in between never
        // closes a descriptor number that the kernel gave to another thread.
        sys::stop_closing_in_children(self.fd);

        // The release is explicit: `close` alone is not enough. A `close`
        // releases the lock only when it drops the last reference to the
        // open file description, and a fork makes a second reference. The
        // release then comes later, when the copy of the child goes. A
        // `flock` can then fail with no row in `/proc/locks` and no
        // descriptor on the inode in any process. `LOCK_UN` removes the
        // record here, in this thread, whatever else holds a reference.
        if !self.handed.load(Ordering::Acquire) {
            let _ = sys::flock(self.fd, sys::LOCK_UN);
        }

        // The close still matters: it releases the lock at an unexpected
        // death, where no `Drop` runs.
        let _ = sys::close(self.fd);
    }
}

#[cfg(test)]
mod tests;
