use super::*;

const ORIGINAL: &str = "nameserver 10.0.0.2\nsearch corp\n";

fn setup(dir: &Path) -> Paths {
    let paths = Paths::in_dir(dir.join("resolv.conf"), dir);
    std::fs::write(&paths.resolv, ORIGINAL).unwrap();
    paths
}

fn ours() -> String {
    format!("{MARKER}\nnameserver 1.1.1.1\n")
}

/// PodUtils/podnet issue 278: the file is replaced by a rename: a link is
/// replaced, not written through, and a failed write keeps the old file.
#[test]
fn the_resolv_conf_is_replaced_and_never_removed_first() {
    let dir = crate::test_dir("resolv-replace");
    let target = dir.join("stub.conf");
    std::fs::write(&target, "nameserver 127.0.0.53\n").unwrap();
    let path = dir.join("resolv.conf");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    replace_file(&path, "nameserver 1.1.1.1\n", MODE).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "nameserver 1.1.1.1\n");
    assert!(!std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "nameserver 127.0.0.53\n");
    let missing = dir.join("no-such-dir").join("resolv.conf");
    assert!(replace_file(&missing, "x", MODE).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "nameserver 1.1.1.1\n");
}

/// The mode of the new file is the mode that the caller gives. The umask
/// of the process is not changed (the tests share the process): a mode
/// with bits that the usual umask clears proves it.
#[test]
fn replace_file_sets_the_mode() {
    let dir = crate::test_dir("resolv-mode");
    for mode in [0o644, 0o666, 0o600] {
        let path = dir.join(format!("resolv-{mode:o}"));
        replace_file(&path, "x\n", mode).unwrap();
        let got = std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(got, mode);
    }
}

#[test]
fn resolv_backup_survives_a_kill() {
    let dir = crate::test_dir("resolv-kill");
    let paths = setup(&dir);
    // A kill: the value goes away with no restore; the lock is free.
    drop(set_file(&paths, &ours()).unwrap());
    assert!(std::fs::read_to_string(&paths.resolv).unwrap().starts_with(MARKER));
    let mode = std::fs::metadata(&paths.record).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    // The second start does not record the marker file as the original.
    let dns = set_file(&paths, &ours()).unwrap();
    let record: Record = serde_json::from_str(&std::fs::read_to_string(&paths.record).unwrap()).unwrap();
    assert_eq!(record.text.as_deref(), Some(ORIGINAL));
    drop(dns);
    reconcile(&paths).unwrap();
    assert_eq!(std::fs::read_to_string(&paths.resolv).unwrap(), ORIGINAL);
    assert!(!paths.record.exists());
}

#[test]
fn resolv_backup_restores_a_symlink_and_an_absent_file() {
    let dir = crate::test_dir("resolv-link");
    let paths = Paths::in_dir(dir.join("resolv.conf"), &dir);
    std::os::unix::fs::symlink("stub.conf", &paths.resolv).unwrap();
    drop(set_file(&paths, &ours()).unwrap());
    reconcile(&paths).unwrap();
    assert_eq!(std::fs::read_link(&paths.resolv).unwrap(), PathBuf::from("stub.conf"));
    std::fs::remove_file(&paths.resolv).unwrap();
    drop(set_file(&paths, &ours()).unwrap());
    reconcile(&paths).unwrap();
    assert!(std::fs::symlink_metadata(&paths.resolv).is_err());
}

#[test]
fn resolv_marker_guards_the_restore() {
    let dir = crate::test_dir("resolv-marker");
    let paths = setup(&dir);
    drop(set_file(&paths, &ours()).unwrap());
    std::fs::write(&paths.resolv, "nameserver 9.9.9.9\n").unwrap();
    reconcile(&paths).unwrap();
    assert_eq!(std::fs::read_to_string(&paths.resolv).unwrap(), "nameserver 9.9.9.9\n");
    assert!(!paths.record.exists());
}

/// The marker of the caller (podnet's own line) guards the restore too.
#[test]
fn the_marker_of_the_caller_guards_the_restore() {
    let dir = crate::test_dir("resolv-own-marker");
    let mut paths = setup(&dir);
    paths.marker = "# podnet warp tun --default-route: DNS through WARP".into();
    let text = crate::route::resolv_text(&paths.marker, &["192.0.2.53".parse().unwrap()]);
    drop(set_file(&paths, &text).unwrap());
    reconcile(&paths).unwrap();
    assert_eq!(std::fs::read_to_string(&paths.resolv).unwrap(), ORIGINAL);
}

#[test]
fn resolv_record_of_another_version_is_not_applied() {
    let dir = crate::test_dir("resolv-version");
    let paths = setup(&dir);
    drop(set_file(&paths, &ours()).unwrap());
    let text = std::fs::read_to_string(&paths.record).unwrap();
    std::fs::write(&paths.record, text.replace("\"version\":1", "\"version\":2")).unwrap();
    let err = reconcile(&paths).unwrap_err();
    assert!(err.text().contains("version 2"), "{}", err.chain());
    assert_eq!(err.class, Class::Config);
    assert!(paths.record.exists());
    assert!(std::fs::read_to_string(&paths.resolv).unwrap().starts_with(MARKER));
}

#[test]
fn a_live_tun_blocks_a_second_one() {
    let dir = crate::test_dir("resolv-live");
    let paths = setup(&dir);
    let _live = set_file(&paths, &ours()).unwrap();
    let err = set_file(&paths, &ours()).unwrap_err();
    assert!(err.text().contains("another TUN holds the resolver file"), "{}", err.chain());
    assert_eq!(err.class, Class::TempFail);
    reconcile(&paths).unwrap();
    assert!(std::fs::read_to_string(&paths.resolv).unwrap().starts_with(MARKER));
}
