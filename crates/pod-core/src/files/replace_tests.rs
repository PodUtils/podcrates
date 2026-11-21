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

/// The temporary name of a test that plants a file there first.
#[cfg(unix)]
fn fixed_temp(dir: &Path, _name: &str) -> PathBuf {
    dir.join(".fixed.tmp")
}

/// A write that puts half of the body, then fails as a full disk does.
fn half_write(file: &mut std::fs::File, body: &[u8]) -> io::Result<()> {
    file.write_all(&body[..body.len() / 2])?;
    Err(io::Error::new(io::ErrorKind::StorageFull, "the disk is full"))
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
#[test]
fn a_link_at_the_path_is_followed_only_with_the_option() {
    let dir = dir("follow");
    let (target, link) = (dir.join("dotfiles-config"), dir.join("config"));
    std::fs::write(&target, b"old").unwrap();
    // A relative link, as a dotfiles tool makes.
    std::os::unix::fs::symlink("dotfiles-config", &link).unwrap();
    replace(&link, b"followed", Mode::Private).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"old", "with no option the target is not written");
    assert!(!std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link is replaced");
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("dotfiles-config", &link).unwrap();
    Replace::new(Mode::Private).follow_link(true).write(&link, b"new").unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link stays");
    assert_eq!(std::fs::read(&target).unwrap(), b"new");
    // A dangling link gives its target a new file.
    let (missing, dangling) = (dir.join("missing"), dir.join("dangling"));
    std::os::unix::fs::symlink(&missing, &dangling).unwrap();
    Replace::new(Mode::Private).follow_link(true).write(&dangling, b"made").unwrap();
    assert_eq!(std::fs::read(&missing).unwrap(), b"made");
    assert_eq!(names(&dir), ["config", "dangling", "dotfiles-config", "missing"], "no temporary file stays");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_loop_of_links_is_refused_at_the_follow() {
    let dir = dir("loop");
    let (a, b) = (dir.join("a"), dir.join("b"));
    std::os::unix::fs::symlink(&b, &a).unwrap();
    std::os::unix::fs::symlink(&a, &b).unwrap();
    let e = Replace::new(Mode::Private).follow_link(true).write(&a, b"x").unwrap_err();
    assert_eq!(Failed::of(&e).unwrap().step(), Step::FollowLink, "{e}");
    assert_eq!(names(&dir), ["a", "b"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_link_at_the_temporary_name_is_never_followed_or_removed() {
    let dir = dir("tmplink");
    let (path, victim, planted) = (dir.join("config"), dir.join("victim"), dir.join(".fixed.tmp"));
    std::fs::write(&path, b"old").unwrap();
    std::fs::write(&victim, b"victim").unwrap();
    std::os::unix::fs::symlink(&victim, &planted).unwrap();
    let hooks = Hooks { temp: fixed_temp, ..REAL };
    let failed = Replace::new(Mode::Private).run(&path, b"new", hooks).unwrap_err();
    assert_eq!(failed.step(), Step::Create);
    assert_eq!(failed.cause().kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&victim).unwrap(), b"victim", "the link is not followed");
    assert!(std::fs::symlink_metadata(&planted).unwrap().file_type().is_symlink(), "the link is not removed");
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn mode_keep_keeps_0644_and_gives_0600_to_a_new_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = dir("keep");
    let path = dir.join("config");
    std::fs::write(&path, b"old").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    replace(&path, b"new", Mode::Keep).unwrap();
    assert_eq!(mode_of(&path), 0o644);
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
    replace(&path, b"private", Mode::Private).unwrap();
    assert_eq!(mode_of(&path), 0o600);
    let new = dir.join("new");
    replace(&new, b"x", Mode::Keep).unwrap();
    assert_eq!(mode_of(&new), 0o600);
    // Through a followed link: the mode of the target.
    let (target, link) = (dir.join("target"), dir.join("link"));
    std::fs::write(&target, b"old").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    Replace::new(Mode::Keep).follow_link(true).write(&link, b"new").unwrap();
    assert_eq!(mode_of(&target), 0o640);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_write_leaves_the_old_file_and_no_temporary_file() {
    let dir = dir("halfwrite");
    let path = dir.join("config");
    std::fs::write(&path, b"old").unwrap();
    let hooks = Hooks { write: half_write, ..REAL };
    let failed = Replace::new(Mode::Private).run(&path, b"a new body", hooks).unwrap_err();
    assert_eq!(failed.step(), Step::Write);
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    assert_eq!(names(&dir), ["config"], "no temporary file stays");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_error_names_the_step_and_the_path() {
    let dir = dir("named");
    let path = dir.join("no-such-dir").join("config");
    let e = replace(&path, b"x", Mode::Private).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
    let text = e.to_string();
    let want = format!("replace {}: the create of the temporary file: ", path.display());
    assert!(text.starts_with(&want), "{text}");
    let failed = Failed::of(&e).unwrap();
    assert_eq!((failed.step(), failed.path()), (Step::Create, path.as_path()));
    let busy_path = dir.join("busy");
    let hooks = Hooks { rename: busy, ..REAL };
    let e = io::Error::from(Replace::new(Mode::Private).run(&busy_path, b"x", hooks).unwrap_err());
    assert_eq!(e.kind(), io::ErrorKind::ResourceBusy);
    let want = format!("replace {}: the rename of the temporary file onto it: busy", busy_path.display());
    assert_eq!(e.to_string(), want);
    assert_eq!(names(&dir), Vec::<String>::new(), "no temporary file stays");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_directory_is_made_only_when_asked() {
    let dir = dir("makedir");
    let path = dir.join("ssh").join("deep").join("config");
    assert!(replace(&path, b"x", Mode::Private).is_err());
    assert!(!dir.join("ssh").exists());
    let commit = Replace::new(Mode::Private).make_dir(true).write(&path, b"made").unwrap();
    assert!(matches!(commit, Commit::Durable), "{commit:?}");
    assert_eq!(std::fs::read(&path).unwrap(), b"made");
    #[cfg(unix)]
    for made in [dir.join("ssh"), dir.join("ssh").join("deep")] {
        assert_eq!(mode_of(&made), 0o700, "{}", made.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
