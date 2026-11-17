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
