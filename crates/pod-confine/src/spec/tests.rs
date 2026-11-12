use super::*;

#[test]
fn an_access_set_names_its_rights() {
    let rx = Access::READ | Access::EXECUTE;
    assert!(rx.contains(Access::READ) && rx.contains(Access::EXECUTE) && !rx.contains(Access::WRITE));
    assert_eq!(rx.words(), "read+execute");
    assert_eq!(Access::NONE.words(), "none");
    assert!(Access::NONE.is_empty());
    let read = Access::READ.rights();
    assert_eq!(read, u64::from(k::LANDLOCK_ACCESS_FS_READ_FILE | k::LANDLOCK_ACCESS_FS_READ_DIR));
    let write = Access::WRITE.rights();
    for right in [
        k::LANDLOCK_ACCESS_FS_WRITE_FILE,
        k::LANDLOCK_ACCESS_FS_MAKE_REG,
        k::LANDLOCK_ACCESS_FS_REMOVE_FILE,
        k::LANDLOCK_ACCESS_FS_TRUNCATE,
        k::LANDLOCK_ACCESS_FS_REFER,
    ] {
        assert_ne!(write & u64::from(right), 0, "write lacks the right {right:#x}");
    }
    assert_eq!(write & read, 0, "write gives a read right");
    assert_eq!(Access::EXECUTE.rights(), u64::from(k::LANDLOCK_ACCESS_FS_EXECUTE));
}

/// The ruleset of each ABI handles each file right that the ABI knows,
/// and no right that it does not know (the kernel refuses an unknown
/// right with `EINVAL`).
#[test]
fn each_abi_handles_the_file_rights_that_it_knows() {
    assert_eq!(handled_fs(0), 0);
    assert_eq!(handled_fs(1), 0x1FFF);
    assert_eq!(handled_fs(2), 0x1FFF | u64::from(k::LANDLOCK_ACCESS_FS_REFER));
    assert_eq!(handled_fs(3), 0x7FFF);
    assert_eq!(handled_fs(4), handled_fs(3), "ABI 4 adds TCP rights only");
    assert_eq!(handled_fs(5), 0xFFFF);
    assert_eq!(handled_fs(8), handled_fs(5));
    assert_eq!(handled_fs(9), 0x1FFFF, "ABI 9 adds RESOLVE_UNIX");
    assert_eq!(handled_fs(12), handled_fs(9));
    // Each right of a full access set is handled from ABI 9; up to ABI 8
    // the compile masks off RESOLVE_UNIX.
    let all = (Access::READ | Access::WRITE | Access::EXECUTE).rights();
    assert_eq!(all & !handled_fs(9), 0);
    assert_eq!(FILE_RIGHTS & !handled_fs(9), 0);
    assert_eq!(all & !handled_fs(5), u64::from(k::LANDLOCK_ACCESS_FS_RESOLVE_UNIX));
}

#[test]
fn the_builder_adds_rules_and_restricts_the_tcp_calls_that_it_names() {
    let s = Spec::new().allow("/usr", Access::READ | Access::EXECUTE).connect_to(443).connect_to(80);
    assert_eq!(s.paths, vec![PathRule { path: "/usr".into(), access: Access::READ | Access::EXECUTE }]);
    assert_eq!(s.connect, Some(vec![443, 80]));
    assert_eq!(s.bind, None, "bind stays free");
    assert_eq!(s.deny, crate::Deny::defaults());
    assert_eq!(Spec::default(), Spec::new());
}

/// The scopes are on and UDP is free in a new spec; the builder turns each
/// one.
#[test]
fn a_new_spec_has_the_scopes_and_a_free_udp() {
    let s = Spec::new();
    assert!(s.scope && s.udp);
    let s = s.close_udp().no_scope();
    assert!(!s.scope && !s.udp);
    assert_eq!(SCOPES, 3, "LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET | LANDLOCK_SCOPE_SIGNAL");
}
