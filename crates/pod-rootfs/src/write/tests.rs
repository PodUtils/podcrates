use super::*;

fn scratch(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("podvm-complete-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("etc")).unwrap();
    p.to_string_lossy().to_string()
}

/// ⛔ T-0405's whole subject. The link points at an absolute path outside
/// the rootfs; the write must land inside it and the canary must survive.
#[test]
fn a_write_through_an_absolute_symlink_does_not_escape() {
    let root = scratch("escape");
    let outside = std::env::temp_dir().join(format!("podvm-canary-{}", std::process::id()));
    std::fs::write(&outside, b"canary\n").unwrap();
    std::os::unix::fs::symlink(&outside, format!("{root}/etc/mtab")).unwrap();

    let r = Root::open(&root).unwrap();
    assert!(matches!(r.kind("etc/mtab").unwrap(), Kind::Symlink(_)));
    assert_eq!(
        r.write("etc/mtab", b"inside\n", 0o644).unwrap(),
        Wrote::Replaced
    );

    assert_eq!(std::fs::read(&outside).unwrap(), b"canary\n");
    assert_eq!(
        std::fs::read(format!("{root}/etc/mtab")).unwrap(),
        b"inside\n"
    );
    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&root);
}

/// ⛔ The other half: a DIRECTORY component that is a symlink out. The
/// kernel refuses the resolution, so nothing is written at all.
#[test]
fn a_write_through_a_symlinked_directory_is_refused() {
    let root = scratch("direscape");
    let outside = std::env::temp_dir().join(format!("podvm-outdir-{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, format!("{root}/evil")).unwrap();

    let r = Root::open(&root).unwrap();
    let e = r.write("evil/passwd", b"x\n", 0o644).unwrap_err();
    assert!(format!("{e}").contains("will not write through"), "{e}");
    assert!(!outside.join("passwd").exists());
    let _ = std::fs::remove_dir_all(&outside);
    let _ = std::fs::remove_dir_all(&root);
}

/// ⭐ The three answers, and the door sweep of 2026-09-09 is what asked for
/// them. A `Kind::Dir` test answers "not a directory" to all three of these
/// and only ONE of them is quiet.
#[test]
fn reach_tells_absent_from_a_link_inside_from_a_link_out() {
    let d = scratch("reach");
    let root = Root::open(&d).unwrap();
    // 1. nothing there at all.
    assert!(matches!(root.reach("etc/pki").unwrap(), Reach::Absent));
    // 2. openSUSE's own shape: a symlink to a real directory INSIDE.
    std::fs::create_dir_all(format!("{d}/var/lib/certs")).unwrap();
    std::os::unix::fs::symlink("/var/lib/certs", format!("{d}/etc/certs")).unwrap();
    assert!(
        matches!(root.reach("etc/certs").unwrap(), Reach::Yes),
        "an internal directory link must be followed, or the fixup lands \
             where the payload does not read it"
    );
    // 3. ⛔ door D: a symlink OUT of the rootfs. Rebased by RESOLVE_IN_ROOT
    // it points at nothing, and podvm must SAY so rather than skip.
    let out = std::env::temp_dir().join(format!("podvm-reach-out-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&out);
    std::os::unix::fs::symlink(&out, format!("{d}/etc/away")).unwrap();
    match root.reach("etc/away").unwrap() {
        Reach::Unreachable(why) => {
            assert!(why.contains("etc/away"), "{why}");
            assert!(why.contains("symlink"), "{why}");
        }
        _ => panic!("a link out of the rootfs was not reported"),
    }
    // ⚠ And a plain file where a directory is wanted is the third shape of
    // the same answer.
    std::fs::write(format!("{d}/etc/afile"), b"x").unwrap();
    assert!(matches!(
        root.reach("etc/afile").unwrap(),
        Reach::Unreachable(_)
    ));
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_identical_write_reports_unchanged() {
    let root = scratch("idem");
    let r = Root::open(&root).unwrap();
    assert_eq!(r.write("etc/hosts", b"a\n", 0o644).unwrap(), Wrote::Created);
    assert_eq!(
        r.write("etc/hosts", b"a\n", 0o644).unwrap(),
        Wrote::Unchanged
    );
    assert_eq!(
        r.write("etc/hosts", b"b\n", 0o644).unwrap(),
        Wrote::Replaced
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// T-1523, first half (`85` clauses A through E). The out-link is not
/// merely written through: it is REPLACED by a regular file inside the
/// rootfs, so a later reader never follows it out again.
#[test]
fn a_write_replaces_an_out_link_with_a_regular_file() {
    let root = scratch("replace");
    let outside = std::env::temp_dir().join(format!("podvm-canary-r{}", std::process::id()));
    std::fs::write(&outside, b"canary\n").unwrap();
    std::os::unix::fs::symlink(&outside, format!("{root}/etc/mtab")).unwrap();

    let r = Root::open(&root).unwrap();
    r.write("etc/mtab", b"inside\n", 0o644).unwrap();
    assert!(
        !std::fs::symlink_metadata(format!("{root}/etc/mtab"))
            .unwrap()
            .is_symlink(),
        "etc/mtab is still a symlink after the write"
    );
    assert_eq!(
        std::fs::read(format!("{root}/etc/mtab")).unwrap(),
        b"inside\n"
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"canary\n");
    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&root);
}

/// T-1523, second half (`85` clause F, openSUSE's own shape). A directory
/// link that stays inside the rootfs is FOLLOWED: the fixup lands at its
/// target rather than refused or dropped.
#[test]
fn a_write_follows_a_directory_link_that_stays_inside() {
    let root = scratch("inside");
    std::fs::create_dir_all(format!("{root}/var/aptreal/apt.conf.d")).unwrap();
    std::os::unix::fs::symlink("/var/aptreal", format!("{root}/etc/apt")).unwrap();

    let r = Root::open(&root).unwrap();
    r.write("etc/apt/apt.conf.d/99podvm", b"APT{}\n", 0o644)
        .unwrap();
    assert_eq!(
        std::fs::read(format!("{root}/var/aptreal/apt.conf.d/99podvm")).unwrap(),
        b"APT{}\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}
