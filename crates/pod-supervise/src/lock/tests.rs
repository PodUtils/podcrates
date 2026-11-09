use super::*;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-supervise-lock-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A held lock refuses a second holder in the same process (a second open
/// file description), and the note sees the refusal while it is true.
#[test]
fn a_held_lock_refuses_a_second_holder_and_the_note_sees_the_refusal() {
    let d = scratch("held");
    let path = d.join("sub").join("lock");
    let held = Lock::try_exclusive(&path).unwrap().expect("a free lock");
    let noted = std::cell::Cell::new(false);
    let second = Lock::try_acquire_noting(&path, sys::LOCK_EX, &|p, fd| {
        assert_eq!(p, path.as_path());
        assert!(fd >= 0);
        noted.set(true);
    })
    .unwrap();
    assert!(second.is_none(), "a second exclusive holder was let in");
    assert!(noted.get(), "the refusal was not noted");
    drop(held);
    assert!(Lock::try_exclusive(&path).unwrap().is_some(), "the drop did not free the lock");
    let _ = std::fs::remove_dir_all(&d);
}

/// The defect that the explicit release guards: a `close` frees an `flock`
/// only with the last reference to the open file description, and a fork
/// makes a second one. `dup` is the fork, with no second process.
#[test]
fn releasing_a_lock_frees_it_even_while_a_duplicate_descriptor_lives() {
    let d = scratch("dup");
    let path = d.join("lock");
    let held = Lock::exclusive(&path).unwrap();
    let twin = sys::dup_cloexec(held.fd()).expect("dup the lock descriptor");
    drop(held);
    let free = Lock::try_exclusive(&path).unwrap().is_some();
    let _ = sys::close(twin);
    assert!(free, "a duplicate of the description kept the lock after the drop");
    let _ = std::fs::remove_dir_all(&d);
}

/// A lock given to the payload stays held after this process drops its
/// value: the payload holds a duplicate of the description.
#[test]
fn a_lock_handed_to_the_payload_outlives_this_process_dropping_it() {
    let d = scratch("handed");
    let path = d.join("lock");
    let held = Lock::try_acquire(&path, sys::LOCK_SH).unwrap().expect("a free lock");
    let fd = held.hand_to_payload().expect("hand the lock to the payload");
    let flags = sys::fcntl(fd, sys::F_GETFD, 0).unwrap() as u64;
    assert_eq!(flags & sys::FD_CLOEXEC, 0, "the handed descriptor is still close-on-exec");
    let payload = sys::dup_cloexec(fd).expect("dup the handed descriptor");
    drop(held);
    let still_held = Lock::try_exclusive(&path).unwrap().is_none();
    let _ = sys::close(payload);
    assert!(still_held, "this process released a lock that it gave to the payload");
    assert!(Lock::try_exclusive(&path).unwrap().is_some(), "the last close did not free the lock");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_path_with_a_nul_is_a_usage_failure() {
    let f = match Lock::try_exclusive(Path::new("/tmp/a\0b")) {
        Err(f) => f,
        Ok(_) => panic!("a path with a NUL reached the kernel"),
    };
    assert_eq!((f.stage, f.class), (Stage::Host, Class::Usage));
    assert!(f.text().contains("NUL"), "{f}");
}

/// A directory that cannot be made keeps its I/O cause, so that a caller
/// maps the kind and the path.
#[test]
fn a_parent_that_cannot_be_made_keeps_its_io_cause() {
    let d = scratch("parent");
    let file = d.join("file");
    std::fs::write(&file, b"").unwrap();
    let f = match Lock::try_exclusive(&file.join("lock")) {
        Err(f) => f,
        Ok(_) => panic!("a lock under a regular file"),
    };
    assert!(f.io_cause().is_some(), "{f:?}");
    assert!(f.text().starts_with(&file.display().to_string()), "{f}");
    let _ = std::fs::remove_dir_all(&d);
}
