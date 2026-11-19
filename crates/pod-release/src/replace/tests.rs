use super::*;

fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary directory")
}

fn passes(_: &Path) -> Result<String, String> {
    Ok("podssh 9.9.9".into())
}

#[test]
fn the_file_becomes_the_new_bytes_with_its_mode_and_the_old_one_is_kept_beside_it() {
    let dir = scratch();
    let dir = dir.path();
    let exe = dir.join(if cfg!(windows) { "podssh.exe" } else { "podssh" });
    fs::write(&exe, b"old").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o751)).unwrap();
    }
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&exe).unwrap());
    let replaced = replace(&exe, b"new", &passes).unwrap();
    assert_eq!(replaced.said, "podssh 9.9.9");
    assert_eq!(fs::read(&exe).unwrap(), b"new");
    let prev = replaced.prev.unwrap();
    assert_eq!(prev, dir.join(if cfg!(windows) { "podssh.prev.exe" } else { "podssh.prev" }));
    assert_eq!(fs::read(&prev).unwrap(), b"old");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let now = fs::metadata(&exe).unwrap();
        assert_eq!(now.permissions().mode() & 0o777, 0o751);
        // A new file renamed over the old one: a running binary keeps its
        // own, where a write in place would change the program it runs.
        assert_ne!(now.ino(), inode, "the binary was written in place");
    }
    let left: Vec<_> = fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left.len(), 2, "the binary and the one before it: {left:?}");
    // Again: the kept one is the one before the last update.
    replace(&exe, b"newer", &passes).unwrap();
    assert_eq!(fs::read(&prev).unwrap(), b"new");
    assert_eq!(fs::read_dir(dir).unwrap().count(), 2);
}

/// A new file that its trial refuses never takes the old one's place,
/// and leaves nothing beside it, no kept one either.
#[test]
fn a_new_file_that_its_trial_refuses_leaves_the_old_binary_alone() {
    let dir = scratch();
    let exe = dir.path().join("podssh");
    fs::write(&exe, b"old").unwrap();
    let refused = |_: &Path| -> Result<String, String> { Err("it ended with exit status: 1".to_string()) };
    match replace(&exe, b"new", &refused) {
        Err(Stopped::Trial(why)) => assert!(why.contains("exit status: 1"), "{why}"),
        other => panic!("refused by its trial, not {other:?}"),
    }
    assert_eq!(fs::read(&exe).unwrap(), b"old");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

/// podnet's issue 236: a file put at the name of the new one after its
/// trial run is not installed, and the binary in place does not change.
#[cfg(unix)]
#[test]
fn a_file_swapped_after_the_trial_is_not_installed() {
    let dir = scratch();
    let exe = dir.path().join("podnet");
    fs::write(&exe, b"old").unwrap();
    let swaps = |temp: &Path| -> Result<String, String> {
        let other = temp.with_file_name("swapped");
        fs::write(&other, b"swapped").unwrap();
        fs::rename(&other, temp).unwrap();
        Ok("podnet 9.9.9".into())
    };
    match replace(&exe, b"new", &swaps) {
        Err(Stopped::Trial(why)) => assert!(why.contains("changed after its trial run"), "{why}"),
        other => panic!("refused after its trial, not {other:?}"),
    }
    assert_eq!(fs::read(&exe).unwrap(), b"old");
    assert!(!keep::prev_of(&exe).exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

/// A mode of the old binary that others can write is not copied: the new
/// file is not writable by the group or others, and has no set-user-ID bit.
#[cfg(unix)]
#[test]
fn the_new_file_takes_no_write_of_others_and_no_set_user_id() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch();
    let exe = dir.path().join("podnet");
    fs::write(&exe, b"old").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o4777)).unwrap();
    replace(&exe, b"new", &passes).unwrap();
    assert_eq!(fs::metadata(&exe).unwrap().permissions().mode() & 0o7777, 0o755);
}

#[test]
fn a_path_is_shown_without_the_prefix_of_a_canonical_drive_path() {
    assert_eq!(shown(Path::new(r"\\?\D:\a\podssh.exe")), r"D:\a\podssh.exe");
    assert_eq!(shown(Path::new(r"\\?\UNC\host\share\podssh.exe")), r"\\?\UNC\host\share\podssh.exe");
    assert_eq!(shown(Path::new("/usr/local/bin/podssh")), "/usr/local/bin/podssh");
}

#[test]
fn the_kept_and_the_temporary_names_keep_the_extension() {
    assert_eq!(keep::prev_of(Path::new("/b/podssh")), Path::new("/b/podssh.prev"));
    assert_eq!(keep::prev_of(Path::new("/b/podssh.exe")), Path::new("/b/podssh.prev.exe"));
    assert_eq!(keep::prev_of(Path::new("/b/podssh-1.0.0")), Path::new("/b/podssh-1.0.0.prev"));
    for (exe, ext) in [("/b/podssh.exe", true), ("/b/podssh", false)] {
        let temp = beside(Path::new(exe), "update").unwrap();
        assert_eq!(temp.parent(), Some(Path::new("/b")));
        let name = temp.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with(".podssh.update-") && name.ends_with(".exe") == ext, "{name}");
    }
}

#[test]
fn a_binary_that_is_not_there_is_left_alone_and_nothing_is_written() {
    let dir = scratch();
    let exe = dir.path().join("podssh");
    match replace(&exe, b"new", &passes) {
        Err(Stopped::Disk(why)) => assert!(why.contains("podssh"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// A directory that refuses a new file leaves the old binary as it was.
#[cfg(unix)]
#[test]
fn a_directory_that_refuses_a_new_file_leaves_the_old_binary() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch();
    let exe = dir.path().join("podssh");
    fs::write(&exe, b"old").unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let result = replace(&exe, b"new", &passes);
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    // root writes in any directory: the check holds where it can.
    if let Err(Stopped::Disk(why)) = result {
        assert!(why.contains("a new file in"), "{why}");
        assert_eq!(fs::read(&exe).unwrap(), b"old");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
