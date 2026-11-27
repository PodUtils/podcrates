//! The clamp mode of the walk (PodUtils/podvm#684): a `..` at the guest
//! root, also in the target of a link, stays at the root, as
//! `openat2(RESOLVE_IN_ROOT)` and `docker cp` resolve it. The strict mode
//! refuses the same paths with `Escapes`. No row resolves to a path
//! outside the root.

use std::path::{Path, PathBuf};

use super::*;

/// The tree: `etc/x` (a file), `a/`, `b/`, `run/`, `tmp/up -> ../../..`,
/// `tmp/far -> ../../../etc/x`, `var/run -> /run` and `abs -> /../../etc`.
fn tree(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-enter-clamp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for dir in ["etc", "a", "b", "run", "tmp", "var"] {
        std::fs::create_dir_all(d.join(dir)).unwrap();
    }
    std::fs::write(d.join("etc/x"), "x").unwrap();
    std::os::unix::fs::symlink("../../..", d.join("tmp/up")).unwrap();
    std::os::unix::fs::symlink("../../../etc/x", d.join("tmp/far")).unwrap();
    std::os::unix::fs::symlink("/run", d.join("var/run")).unwrap();
    std::os::unix::fs::symlink("/../../etc", d.join("abs")).unwrap();
    d
}

/// What a row gives: a file or a directory under the root (the path
/// relative to the root), or a refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    File(&'static str),
    Dir(&'static str),
    Refused(ResolveKind),
}

/// Each row: the guest path, the result in the clamp mode, and the result
/// in the strict mode.
const ROWS: [(&str, Want, Want); 10] = [
    ("/../../etc/x", Want::File("etc/x"), Want::Refused(ResolveKind::Escapes)),
    ("tmp/up/etc/x", Want::File("etc/x"), Want::Refused(ResolveKind::Escapes)),
    ("/tmp/far", Want::File("etc/x"), Want::Refused(ResolveKind::Escapes)),
    ("/a/../b", Want::Dir("b"), Want::Dir("b")),
    ("/etc/../../../etc/x", Want::File("etc/x"), Want::Refused(ResolveKind::Escapes)),
    ("/tmp/up", Want::Dir(""), Want::Refused(ResolveKind::Escapes)),
    ("/..", Want::Dir(""), Want::Refused(ResolveKind::Escapes)),
    ("/abs/x", Want::File("etc/x"), Want::Refused(ResolveKind::Escapes)),
    ("/var/run/../etc", Want::Dir("etc"), Want::Dir("etc")),
    ("/../nothing", Want::Refused(ResolveKind::Absent), Want::Refused(ResolveKind::Escapes)),
];

/// The result of `guest` in one mode, as a [`Want`] relative to `root`.
fn got(root: &Path, guest: &str, want: Want, clamp: bool) -> Result<PathBuf, ResolveKind> {
    let dir = matches!(want, Want::Dir(_));
    match (clamp, dir) {
        (true, true) => resolve_dir_in_clamped(root, guest),
        (true, false) => resolve_in_clamped(root, guest),
        (false, true) => resolve_dir_in(root, guest),
        (false, false) => resolve_in(root, guest),
    }
}

fn check(root: &Path, guest: &str, want: Want, clamp: bool) {
    let r = got(root, guest, want, clamp);
    let mode = if clamp { "clamp" } else { "strict" };
    match (want, &r) {
        (Want::File(rel) | Want::Dir(rel), Ok(host)) => {
            let expected = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
            assert_eq!(host, &expected, "{guest} ({mode})");
        }
        (Want::Refused(kind), Err(k)) => assert_eq!(*k, kind, "{guest} ({mode})"),
        _ => panic!("{guest} ({mode}): {r:?}, not {want:?}"),
    }
    // No row names a path outside the root.
    if let Ok(host) = r {
        assert!(host.starts_with(root), "{guest} ({mode}) left the root: {}", host.display());
    }
}

#[test]
fn a_clamped_walk_keeps_dot_dot_at_the_root() {
    let root = tree("rows");
    for (guest, clamp, strict) in ROWS {
        check(&root, guest, clamp, true);
        check(&root, guest, strict, false);
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// With a volume, a `..` at the root of the guest stays at the guest root:
/// it never reads the host parent of the side of the volume.
#[test]
fn a_clamped_walk_never_reads_the_host_parent_of_a_volume() {
    let root = tree("vol-root");
    let host = tree("vol-host");
    std::fs::write(host.join("a/secret"), "host").unwrap();
    let bind = |guest: &str| crate::VolumeBind {
        host: host.join("a").to_str().unwrap().to_string(),
        guest: guest.to_string(),
        ..Default::default()
    };
    let maps = crate::userland::Maps::of_binds(root.to_str().unwrap(), &[bind("/data")]);
    assert_eq!(resolve_in_clamped(&maps, "/data/secret"), Ok(host.join("a/secret")));
    assert_eq!(resolve_in_clamped(&maps, "/data/../../../etc/x"), Ok(root.join("etc/x")));
    assert_eq!(resolve_in_clamped(&maps, "/data/../../secret"), Err(ResolveKind::Absent));
    assert_eq!(resolve_in(&maps, "/data/../../../etc/x"), Err(ResolveKind::Escapes));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&host);
}
