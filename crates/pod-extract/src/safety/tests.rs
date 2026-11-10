use super::*;

#[test]
fn absolute_and_dotdot_are_refused_lexically() {
    assert_eq!(components("/etc/passwd"), Err(Refusal::Absolute));
    assert_eq!(
        components("a/../../etc/passwd"),
        Err(Refusal::Component("..".into()))
    );
    assert_eq!(components("").unwrap_err(), Refusal::Empty);
    assert_eq!(components("./").unwrap_err(), Refusal::Empty);
    // ⭐ And the caller asks `is_archive_root` FIRST, so that refusal is
    // never reached for the archive's own root. See `apply`.
    assert!(is_archive_root("./"));
    assert!(is_archive_root("."));
    assert!(is_archive_root("/"));
    assert!(!is_archive_root(""));
    assert!(!is_archive_root("./etc"));
}

/// ⚠ A real OCI layer writes `bin/` and `bin`, with no `./` prefix, and a
/// trailing slash on a directory. Neither is a defect.
/// `experiments/results/whiteout-contract.txt` check A is where that was
/// measured.
#[test]
fn ordinary_layer_paths_survive() {
    assert_eq!(components("bin/").unwrap(), vec!["bin"]);
    assert_eq!(components("./etc/shadow").unwrap(), vec!["etc", "shadow"]);
    assert_eq!(
        components("usr/lib/x.so").unwrap(),
        vec!["usr", "lib", "x.so"]
    );
}

/// ⭐ T-0305, and the exact link
/// `experiments/results/whiteout-contract.txt` check C read out of a real
/// voidlinux layer.
#[test]
fn absolute_symlink_target_is_rebased_not_refused() {
    let got = rebase_symlink_target(&["var", "cache"], "/var/cache/xbps").unwrap();
    assert_eq!(got, vec!["var", "cache", "xbps"]);
}

#[test]
fn relative_symlink_target_resolves_from_its_own_directory() {
    // /bin -> usr/bin
    assert_eq!(
        rebase_symlink_target(&[], "usr/bin").unwrap(),
        vec!["usr", "bin"]
    );
    // /etc/mtab -> /proc/self/mounts
    assert_eq!(
        rebase_symlink_target(&["etc"], "/proc/self/mounts").unwrap(),
        vec!["proc", "self", "mounts"]
    );
    // a legitimate climb: /usr/lib/x -> ../share/y
    assert_eq!(
        rebase_symlink_target(&["usr", "lib"], "../share/y").unwrap(),
        vec!["usr", "share", "y"]
    );
}

#[test]
fn a_target_that_still_escapes_after_rebasing_is_refused() {
    let r = rebase_symlink_target(&["etc"], "../../../../etc/passwd");
    assert!(matches!(r, Err(Refusal::Escapes(_))), "got {r:?}");
}

/// ⭐ **BOTH MECHANISMS, ON ONE KERNEL, AGAINST THE SAME ATTACK.**
///
/// This host has `openat2`, measured on 2026-09-08 against kernel
/// 6.18.44, so an `Auto` run never touches the `O_NOFOLLOW` walk and the
/// walk would ship having never refused anything. Every kernel before
/// Linux 5.6 runs only the walk.
///
/// ⛔ The two are asserted to give the SAME answer. A fallback that is
/// merely present is not a fallback.
#[test]
fn the_walk_and_openat2_refuse_the_same_traversal() {
    let base = std::env::temp_dir().join(format!("podvm-safety-{}", std::process::id()));
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
        // And a legitimate path still resolves under the same mechanism, so
        // the refusal above is discrimination rather than a blanket no.
        let ok = open_parent_with(&root, &["real", "f"], false, how).unwrap();
        assert!(ok.is_ok(), "{how:?} refused a legitimate path");
    }
    drop(root);
    let _ = std::fs::remove_dir_all(&base);
}

/// ⚠ The claim in this module's header, asserted rather than assumed: if
/// this stops being true the test above is silently only running one arm.
#[test]
fn this_host_has_openat2_so_the_walk_needs_forcing_to_be_exercised() {
    assert!(
        have_openat2(),
        "this kernel has no openat2, so Resolve::Auto already IS the walk \
             and the two arms above are the same arm"
    );
}
