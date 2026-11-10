use super::*;

#[test]
fn absolute_and_dotdot_are_refused_lexically() {
    assert_eq!(components("/etc/passwd"), Err(Refusal::Absolute));
    assert_eq!(components("a/../../etc/passwd"), Err(Refusal::Component("..".into())));
    assert_eq!(components("").unwrap_err(), Refusal::Empty);
    assert_eq!(components("./").unwrap_err(), Refusal::Empty);
    // The caller asks `is_archive_root` FIRST, so the root of the archive
    // never gets that refusal (`apply`).
    assert!(is_archive_root("./"));
    assert!(is_archive_root("."));
    assert!(is_archive_root("/"));
    assert!(!is_archive_root(""));
    assert!(!is_archive_root("./etc"));
}

/// A real OCI layer writes `bin/` and `bin`, with no `./` prefix, and a
/// trailing slash on a directory. Neither is a defect.
#[test]
fn ordinary_layer_paths_survive() {
    assert_eq!(components("bin/").unwrap(), vec!["bin"]);
    assert_eq!(components("./etc/shadow").unwrap(), vec!["etc", "shadow"]);
    assert_eq!(components("usr/lib/x.so").unwrap(), vec!["usr", "lib", "x.so"]);
}

/// The link of a real void layer.
#[test]
fn absolute_symlink_target_is_rebased_not_refused() {
    let got = rebase_symlink_target(&["var", "cache"], "/var/cache/xbps").unwrap();
    assert_eq!(got, vec!["var", "cache", "xbps"]);
}

#[test]
fn relative_symlink_target_resolves_from_its_own_directory() {
    // /bin -> usr/bin
    assert_eq!(rebase_symlink_target(&[], "usr/bin").unwrap(), vec!["usr", "bin"]);
    // /etc/mtab -> /proc/self/mounts
    assert_eq!(rebase_symlink_target(&["etc"], "/proc/self/mounts").unwrap(), vec!["proc", "self", "mounts"]);
    // a legitimate climb: /usr/lib/x -> ../share/y
    assert_eq!(rebase_symlink_target(&["usr", "lib"], "../share/y").unwrap(), vec!["usr", "share", "y"]);
}

#[test]
fn a_target_that_still_escapes_after_rebasing_is_refused() {
    let r = rebase_symlink_target(&["etc"], "../../../../etc/passwd");
    assert!(matches!(r, Err(Refusal::Escapes(_))), "got {r:?}");
}

/// **BOTH MECHANISMS, ON ONE KERNEL, AGAINST THE SAME ATTACK.**
///
/// A current kernel has `openat2`, so an `Auto` run never goes into the
/// `O_NOFOLLOW` walk, and the walk would ship with no refusal seen. Each
/// kernel before Linux 5.6 runs only the walk.
///
/// The test asserts the SAME answer from both. A fallback that is only
/// present is not a fallback.
#[test]
fn the_walk_and_openat2_refuse_the_same_traversal() {
    let base = std::env::temp_dir().join(format!("pod-extract-safety-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("real")).unwrap();
    // `evil -> /etc`, the first entry of the crafted layer.
    std::os::unix::fs::symlink("/etc", base.join("evil")).unwrap();
    let root = Dir::open(&base.to_string_lossy()).unwrap();

    for how in [Resolve::Auto, Resolve::Walk] {
        let r = open_parent_with(&root, &["evil", "passwd"], false, how).unwrap();
        match r {
            Err(Refusal::Escapes(_)) => {}
            Err(other) => panic!("{how:?} refused for the wrong reason: {other:?}"),
            Ok(_) => panic!("{how:?} DID NOT REFUSE the traversal"),
        }
        // A legal path still resolves with the same mechanism, so the refusal
        // above is a decision and not a "no" to each path.
        let ok = open_parent_with(&root, &["real", "f"], false, how).unwrap();
        assert!(ok.is_ok(), "{how:?} refused a legitimate path");
    }
    drop(root);
    let _ = std::fs::remove_dir_all(&base);
}

/// The claim of the test above, asserted: with no `openat2` the test above
/// runs only one arm. An emulator that does not translate the call has none:
/// under an emulator the test says so and stops.
#[test]
fn this_host_has_openat2_so_the_walk_needs_forcing_to_be_exercised() {
    let set = |v: &str| std::env::var_os(v).is_some();
    if !have_openat2() && pod_hostprobe::interp::detect(&set).is_emulated() {
        pod_testkit::host::skip_line("an emulator runs this test, and it does not translate openat2(2)");
        return;
    }
    assert!(
        have_openat2(),
        "this kernel has no openat2, so Resolve::Auto already IS the walk \
             and the two arms above are the same arm"
    );
}
