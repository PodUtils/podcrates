//! PodUtils/podnet@T-173 and PodUtils/podnet#413, ask 9: another owner is
//! a `Config` failure (exit 78) with no consent, and is read with the
//! consent of the caller; the mode is checked in each case.

use std::os::unix::fs::PermissionsExt as _;

use super::*;

fn mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn a_foreign_owner_is_a_config_error() {
    let dir = crate::test_dir("foreign");
    let path = dir.join("warp.json");
    std::fs::write(&path, "{}").unwrap();
    mode(&path, 0o600);
    let me = this_uid();
    let err = check_private_as(&path, me + 1, Foreign::Refuse).unwrap_err();
    assert_eq!(err.class.exit_code(), 78);
    assert!(err.to_string().contains("not to this user"), "{err}");
    assert!(!check_private_as(&path, me, Foreign::Refuse).unwrap());
    // The mode of an own file is still checked.
    mode(&path, 0o644);
    assert!(check_private_as(&path, me, Foreign::Refuse).is_err());
}

/// With the consent of the caller, a file of another user reads; the check
/// of the mode stays, and the answer says that the file is foreign.
#[test]
fn a_foreign_owner_reads_with_the_consent_of_the_caller() {
    let dir = crate::test_dir("foreign-allow");
    let path = dir.join("warp.json");
    std::fs::write(&path, "{}").unwrap();
    mode(&path, 0o600);
    let other = this_uid() + 1;
    assert!(check_private_as(&path, other, Foreign::Allow).unwrap(), "a foreign file");
    mode(&path, 0o640);
    let err = check_private_as(&path, other, Foreign::Allow).unwrap_err();
    assert!(err.to_string().contains("mode 0640"), "{err}");
    assert_eq!(err.class, Class::Config);
}

#[test]
fn a_private_file_has_mode_0600_and_a_loose_one_is_refused() {
    let dir = crate::test_dir("private");
    let path = dir.join("state/podnet/warp.json");
    write_private(&path, b"{}").unwrap();
    let mode_of = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode_of(&path), 0o600);
    assert_eq!(mode_of(path.parent().unwrap()), 0o700);
    check_private(&path, Foreign::Refuse).unwrap();
    // A second write replaces the file and leaves no temporary file.
    write_private(&path, b"{\"a\":1}").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"{\"a\":1}");
    assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    mode(&path, 0o644);
    let err = check_private(&path, Foreign::Refuse).unwrap_err().to_string();
    assert!(err.contains("mode 0644") && err.contains("chmod 600"), "{err}");
    // PodUtils/podnet issue 253: a directory is not a loose file.
    let err = check_private(&dir, Foreign::Refuse).unwrap_err();
    assert_eq!(err.class.exit_code(), 64);
    let text = err.to_string();
    assert!(text.contains("is a directory") && !text.contains("chmod"), "{text}");
}

#[tokio::test]
async fn a_held_lock_gives_exit_75_after_the_wait() {
    let dir = crate::test_dir("lock");
    let path = dir.join("warp.json");
    let held = lock(&path, Duration::from_secs(1)).await.unwrap();
    let err = lock(&path, Duration::from_millis(200)).await.unwrap_err();
    assert_eq!(err.class.exit_code(), 75, "{err}");
    assert!(err.to_string().contains("another process"), "{err}");
    drop(held);
    lock(&path, Duration::from_millis(200)).await.expect("the lock is free again");
}
