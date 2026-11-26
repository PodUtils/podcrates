use super::*;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-core-lock-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An exclusive lock excludes each other lock of the file until it drops;
/// shared locks share.
#[test]
fn an_exclusive_lock_excludes_and_shared_locks_share() {
    let dir = scratch("modes");
    let path = dir.join("lock");
    let held = FileLock::try_lock(&path, Mode::Exclusive).unwrap().expect("a free lock");
    assert_eq!((held.path(), held.mode()), (path.as_path(), Mode::Exclusive));
    assert!(FileLock::try_lock(&path, Mode::Exclusive).unwrap().is_none());
    assert!(FileLock::try_lock(&path, Mode::Shared).unwrap().is_none());
    drop(held);
    let a = FileLock::try_lock(&path, Mode::Shared).unwrap().expect("a shared lock");
    let b = FileLock::try_lock(&path, Mode::Shared).unwrap().expect("a second shared lock");
    assert!(FileLock::try_lock(&path, Mode::Exclusive).unwrap().is_none());
    drop((a, b));
    assert!(FileLock::try_lock(&path, Mode::Exclusive).unwrap().is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A bounded wait ends at its bound with a refusal that names the file, and
/// takes the lock where its holder releases it in time.
#[test]
fn a_bounded_wait_names_the_file_at_its_bound() {
    let dir = scratch("bound");
    let path = dir.join("lock");
    let held = FileLock::try_lock(&path, Mode::Exclusive).unwrap().unwrap();
    let start = Instant::now();
    let e = FileLock::lock_within(&path, Mode::Exclusive, Duration::from_millis(100)).expect_err("a held lock");
    assert!(start.elapsed() >= Duration::from_millis(100));
    assert_eq!(e.class, Class::Unavailable);
    let text = e.to_string();
    assert!(text.contains(&path.display().to_string()) && text.contains("another process"), "{text}");
    drop(held);
    assert!(FileLock::lock_within(&path, Mode::Exclusive, Duration::from_millis(100)).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link at the path of a lock is refused: the lock never lands on the
/// target of the link, which stays as it was.
#[cfg(unix)]
#[test]
fn a_link_at_the_path_is_refused() {
    let dir = scratch("link");
    let target = dir.join("target");
    std::fs::write(&target, b"kept").unwrap();
    std::os::unix::fs::symlink(&target, dir.join("lock")).unwrap();
    assert!(FileLock::try_lock(&dir.join("lock"), Mode::Exclusive).is_err());
    assert!(FileLock::try_lock(&target, Mode::Exclusive).unwrap().is_some(), "the target is not locked");
    assert_eq!(std::fs::read(&target).unwrap(), b"kept");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A wait with no bound ends with a refusal when the caller says stop, and
/// takes the lock where its holder releases it first.
#[test]
fn a_wait_with_no_bound_ends_at_a_stop() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = scratch("until");
    let path = dir.join("lock");
    let held = FileLock::try_lock(&path, Mode::Exclusive).unwrap().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    let setter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        signal.store(true, Ordering::SeqCst);
    });
    let e = FileLock::lock_until(&path, Mode::Shared, || stop.load(Ordering::SeqCst)).expect_err("a stop");
    setter.join().unwrap();
    assert_eq!(e.class, Class::TempFail);
    assert!(e.to_string().contains(&path.display().to_string()) && e.to_string().contains("stopped"), "{e}");
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        drop(held);
    });
    assert!(FileLock::lock_until(&path, Mode::Exclusive, || false).is_ok());
    releaser.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A probe of a lock makes no file: a missing file is `NotFound`, and a
/// file that exists is locked as by `try_lock`.
#[test]
fn a_probe_of_a_lock_makes_no_file() {
    let dir = scratch("existing");
    let path = dir.join("lock");
    let e = FileLock::try_lock_existing(&path, Mode::Shared).expect_err("no file");
    assert_eq!(e.class, Class::NotFound, "{e}");
    assert!(!path.exists(), "the probe made the file");
    let held = FileLock::try_lock(&path, Mode::Exclusive).unwrap().unwrap();
    assert!(FileLock::try_lock_existing(&path, Mode::Shared).unwrap().is_none());
    drop(held);
    assert!(FileLock::try_lock_existing(&path, Mode::Exclusive).unwrap().is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A lock file that the caller cannot write opens read only, and each mode
/// of the lock holds on it (a lock file of another user).
#[cfg(unix)]
#[test]
fn a_lock_file_that_the_caller_cannot_write_opens_read_only() {
    use std::os::unix::fs::PermissionsExt as _;
    // SAFETY: geteuid reads the effective user id; it has no failure.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root writes each file, so no write open fails");
        return;
    }
    let dir = scratch("readonly");
    let path = dir.join("lock");
    std::fs::write(&path, b"").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    for mode in [Mode::Exclusive, Mode::Shared] {
        let held = FileLock::try_lock(&path, mode).unwrap().expect("a lock on a read-only file");
        assert!(FileLock::try_lock(&path, Mode::Exclusive).unwrap().is_none(), "{mode:?}");
        drop(held);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A drop releases the lock also while another descriptor of the same open
/// file holds on: the copy of a child that a spawn makes before its exec.
/// A close alone keeps the lock for that copy.
#[test]
fn a_dropped_lock_is_free_while_a_copy_of_its_descriptor_is_open() {
    let dir = scratch("copy");
    let path = dir.join("lock");
    for mode in [Mode::Exclusive, Mode::Shared] {
        let held = FileLock::try_lock(&path, mode).unwrap().expect("a free lock");
        let copy = held.file().try_clone().unwrap();
        drop(held);
        let again = FileLock::try_lock(&path, Mode::Exclusive).unwrap();
        assert!(again.is_some(), "a {mode:?} lock stays held by a copy of its descriptor after the drop");
        drop((again, copy));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
