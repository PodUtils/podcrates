use super::*;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-core-replace-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    out.sort();
    out
}

#[test]
fn a_replace_puts_the_new_file_in_place_and_leaves_no_temporary_file() {
    let dir = dir("new");
    let path = dir.join("key");
    std::fs::write(&path, b"old").unwrap();
    let commit = replace_private(&path, b"new", 0o600, false).unwrap();
    assert!(matches!(commit, Commit::Durable), "{commit:?}");
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
    assert_eq!(names(&dir), ["key"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn the_file_has_the_mode_that_was_asked_whatever_the_umask() {
    use std::os::unix::fs::PermissionsExt;
    let dir = dir("mode");
    for mode in [0o600, 0o644] {
        let path = dir.join(format!("f{mode:o}"));
        replace_private(&path, b"x", mode, false).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, mode);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_symlink_at_the_path_is_replaced_not_followed() {
    let dir = dir("link");
    let (target, link) = (dir.join("target"), dir.join("link"));
    std::fs::write(&target, b"kept").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    replace_private(&link, b"new", 0o600, false).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"kept", "the target of the link is not written");
    assert!(!std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read(&link).unwrap(), b"new");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_rename_changes_nothing_and_leaves_no_temporary_file() {
    let dir = dir("fail");
    // A directory at the path: a file cannot be renamed over it.
    let path = dir.join("taken");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("inside"), b"x").unwrap();
    assert!(replace_private(&path, b"new", 0o600, false).is_err());
    assert_eq!(names(&dir), ["taken"]);
    assert!(path.is_dir());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_path_with_no_directory_or_name_is_refused() {
    assert!(parts(Path::new("/")).is_err());
    assert_eq!(parts(Path::new("bare")).unwrap(), (PathBuf::from("."), "bare".to_string()));
}

fn busy(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::ResourceBusy, "busy"))
}

/// A bind-mounted file cannot be renamed over: the fallback writes it in
/// place, and says that the commit is unconfirmed; with no fallback,
/// nothing changes.
#[test]
fn a_rename_that_fails_writes_in_place_only_with_the_fallback() {
    let dir = dir("inplace");
    let path = dir.join("resolv.conf");
    std::fs::write(&path, b"a much longer old body").unwrap();
    assert!(replace_with(&path, b"new", 0o644, false, busy).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"a much longer old body");
    let commit = replace_with(&path, b"new", 0o644, true, busy).unwrap();
    assert!(matches!(&commit, Commit::InPlace(e) if e.kind() == io::ErrorKind::ResourceBusy), "{commit:?}");
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
    assert_eq!(names(&dir), ["resolv.conf"], "no temporary file stays");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A reader holds the old file open: the rename with POSIX semantics
/// replaces the name, and the reader keeps the old body.
#[cfg(windows)]
#[test]
fn on_windows_a_replace_takes_a_name_that_a_reader_holds() {
    use std::io::Read;
    let dir = dir("held");
    let path = dir.join("key");
    std::fs::write(&path, b"old").unwrap();
    let mut reader = std::fs::File::open(&path).unwrap();
    replace_private(&path, b"new", 0o600, false).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
    let mut old = Vec::new();
    reader.read_to_end(&mut old).unwrap();
    assert_eq!(old, b"old");
    drop(reader);
    assert_eq!(names(&dir), ["key"]);
    let _ = std::fs::remove_dir_all(&dir);
}
