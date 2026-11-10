use super::*;

fn scratch(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("podvm-dev-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p.to_string_lossy().to_string()
}

fn zeros() -> Vec<u8> {
    vec![0u8; FILLED_BYTES]
}

/// ⭐ The failure T-0401 is for, driven end to end against the SHIM ARM.
///
/// ⛔ [`shim_for`] rather than [`run`], deliberately: `run` asks the kernel
/// whether `mknod` works, and on a machine that permits it there is no shim
/// to assert on. A test that drove `run` here would measure the host and
/// report the host's answer as the code's.
#[test]
fn a_grown_dev_null_is_truncated_and_the_growth_is_reported() {
    let d = scratch("grown");
    std::fs::create_dir_all(format!("{d}/dev")).unwrap();
    std::fs::write(format!("{d}/dev/null"), vec![b'x'; 4096]).unwrap();
    let root = Root::open(&d).unwrap();
    let fs = shim_for(&root, "dev/null", "EPERM", None, &zeros(), &zeros()).unwrap();
    assert_eq!(fs[0].action, Action::Rewrote);
    assert!(fs[0].detail.contains("4096 bytes"), "{}", fs[0].detail);
    assert!(fs[0].detail.contains("EPERM"), "{}", fs[0].detail);
    assert_eq!(std::fs::metadata(format!("{d}/dev/null")).unwrap().len(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

/// ⛔ Every shim is marked degraded, without exception, and every shim's
/// line carries the errno `mknod` refused with. A shim that reads as
/// `Native` is the class of lie this project exists to refuse.
#[test]
fn every_shim_is_reported_as_a_degradation_and_names_the_errno() {
    let d = scratch("degraded");
    let root = Root::open(&d).unwrap();
    root.mkdirs("dev").unwrap();
    let (z, r) = (zeros(), zeros());
    for (path, ..) in DEVICES {
        let fs = shim_for(&root, path, "EPERM", None, &z, &r).unwrap();
        assert_eq!(fs.len(), 1, "{path}");
        assert!(fs[0].degraded, "{path}: {:?}", fs[0]);
        assert!(fs[0].detail.contains("EPERM"), "{path}: {:?}", fs[0]);
    }
    assert_eq!(
        std::fs::metadata(format!("{d}/dev/zero")).unwrap().len() as usize,
        FILLED_BYTES
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// ⚠ The second pass must not rewrite a megabyte for nothing.
#[test]
fn a_second_pass_leaves_the_filled_shims_alone() {
    let d = scratch("idem");
    let root = Root::open(&d).unwrap();
    root.mkdirs("dev").unwrap();
    let (z, r) = (zeros(), zeros());
    for p in ["dev/zero", "dev/urandom", "dev/random"] {
        shim_for(&root, p, "EPERM", None, &z, &r).unwrap();
        let fs = shim_for(&root, p, "EPERM", None, &z, &r).unwrap();
        assert_eq!(fs[0].action, Action::Unchanged, "{p}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// ⭐ **The property that holds on EVERY machine**, whichever arm this one
/// takes: a row claims a real device only where a real device is on disk,
/// and a row that is a file is marked degraded. ⛔ This is the assertion
/// that catches the two ways T-0401 can be wrong -- a shim presented as a
/// device, and a device presented as a shim -- and it needs no knowledge of
/// whether this host permits `mknod`.
#[test]
fn what_each_row_claims_is_what_is_on_disk() {
    use std::os::unix::fs::FileTypeExt;
    let d = scratch("claims");
    let root = Root::open(&d).unwrap();
    let fs = run(&root).unwrap();
    // The four device rows plus the four T-0413 `/dev` link rows.
    assert_eq!(fs.len(), DEVICES.len() + FD_LINKS.len(), "{fs:#?}");
    for f in &fs {
        let md = std::fs::symlink_metadata(format!("{d}/{}", f.path)).unwrap();
        let is_dev = md.file_type().is_char_device();
        match f.id {
            "dev-node" => {
                assert!(is_dev, "{f:?} claims a node and {} is not one", f.path);
                assert!(!f.degraded, "{f:?} is a real device and is marked degraded");
            }
            "dev-shim" => {
                assert!(!is_dev, "{f:?} claims a shim and {} is a device", f.path);
                assert!(f.degraded, "{f:?} is a shim and is not marked degraded");
            }
            // T-0413: a staged link reads back its conventional
            // target, and a link is the real content, never a
            // degradation.
            "dev-fd-link" => {
                let want = FD_LINKS
                    .iter()
                    .find(|(p, _)| *p == f.path)
                    .unwrap_or_else(|| panic!("unknown link path {:?}", f.path))
                    .1;
                assert_eq!(
                    std::fs::read_link(format!("{d}/{}", f.path)).unwrap(),
                    std::path::PathBuf::from(want),
                    "{f:?}"
                );
                assert!(
                    !f.degraded,
                    "{f:?} is a conventional link and is marked degraded"
                );
            }
            other => panic!("unknown fixup id {other:?}"),
        }
    }
    // ⚠ And running it again changes nothing, on either arm.
    for f in run(&root).unwrap() {
        assert_eq!(f.action, Action::Unchanged, "{f:?}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// T-0415: a directory where a node belongs is refused with its path
/// named, and the directory is left alone. The row is degraded, so
/// `--strict` refuses the run through the tested wiring in
/// `podvm/src/complete.rs`.
#[test]
fn a_directory_at_a_device_path_is_a_named_failure() {
    let d = scratch("dirshape");
    std::fs::create_dir_all(format!("{d}/dev/null")).unwrap();
    let root = Root::open(&d).unwrap();
    let fs = run(&root).unwrap();
    let f = fs
        .iter()
        .find(|f| f.entry == "T-0401" && f.path == "dev/null")
        .unwrap();
    assert_eq!(f.action, Action::Failed, "{f:?}");
    assert!(f.degraded, "{f:?}");
    assert!(f.detail.contains("dev/null"), "{}", f.detail);
    assert!(f.detail.contains("directory"), "{}", f.detail);
    assert!(std::fs::metadata(format!("{d}/dev/null")).unwrap().is_dir());
    let _ = std::fs::remove_dir_all(&d);
}

/// T-0415: a symlink where a node belongs is replaced by the shim, and
/// the row says what it replaced. Replacing the link removes the link
/// alone, so a target outside the rootfs stays untouched.
#[test]
fn a_symlink_at_a_device_path_is_replaced_and_named() {
    let d = scratch("symshape");
    std::fs::create_dir_all(format!("{d}/dev")).unwrap();
    std::fs::write(format!("{d}/outside"), b"stay").unwrap();
    std::os::unix::fs::symlink(format!("{d}/outside"), format!("{d}/dev/null")).unwrap();
    let root = Root::open(&d).unwrap();
    let fs = run(&root).unwrap();
    let f = fs
        .iter()
        .find(|f| f.entry == "T-0401" && f.path == "dev/null")
        .unwrap();
    // T-1686: the host decides the path. Where `mknod` is permitted the row
    // is a real node, else a shim; either row names the link it replaced.
    assert_eq!(f.action, Action::Created, "{f:?}");
    assert!(f.detail.contains("symlink"), "{}", f.detail);
    assert!(f.detail.contains("outside"), "{}", f.detail);
    let meta = std::fs::symlink_metadata(format!("{d}/dev/null")).unwrap();
    if f.id == "dev-node" {
        use std::os::unix::fs::FileTypeExt;
        assert!(!f.degraded, "{f:?}");
        assert!(meta.file_type().is_char_device(), "{meta:?}");
    } else {
        assert!(f.degraded, "{f:?}");
        assert!(meta.is_file(), "{meta:?}");
        assert_eq!(meta.len(), 0);
    }
    assert_eq!(std::fs::read(format!("{d}/outside")).unwrap(), b"stay");
    let _ = std::fs::remove_dir_all(&d);
}

/// T-0415: a fifo where a node belongs is replaced by the shim, and the
/// row says what it replaced. A fifo would hang a writer with no
/// reader, so leaving it is not an option.
#[test]
fn a_fifo_at_a_device_path_is_replaced_and_named() {
    let d = scratch("fifoshape");
    std::fs::create_dir_all(format!("{d}/dev")).unwrap();
    assert!(std::process::Command::new("mkfifo")
        .arg(format!("{d}/dev/zero"))
        .status()
        .unwrap()
        .success());
    let root = Root::open(&d).unwrap();
    let fs = run(&root).unwrap();
    let f = fs
        .iter()
        .find(|f| f.entry == "T-0401" && f.path == "dev/zero")
        .unwrap();
    // T-1686: a real node where `mknod` is permitted, else a shim; either
    // row names the fifo it replaced.
    assert_eq!(f.action, Action::Rewrote, "{f:?}");
    assert!(f.detail.contains("a fifo"), "{}", f.detail);
    let meta = std::fs::symlink_metadata(format!("{d}/dev/zero")).unwrap();
    if f.id == "dev-node" {
        use std::os::unix::fs::FileTypeExt;
        assert!(meta.file_type().is_char_device(), "{meta:?}");
    } else {
        assert_eq!(meta.len() as usize, FILLED_BYTES);
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// T-0415: the unlink-failure refusal arm of [`displace_link`].
/// `remove_file` refuses a directory on every user, and root
/// bypasses permission bits, so a read-only parent cannot stage
/// the failure: a directory is what this arm needs, any path
/// `remove_file` will not take. The refusal names the path, the
/// link target, the `mknod` risk and the `--strict` gate.
#[test]
fn an_unremovable_link_is_a_named_refusal() {
    let d = scratch("unstated");
    std::fs::create_dir_all(format!("{d}/dev/null")).unwrap();
    let root = Root::open(&d).unwrap();
    let err = displace_link(&root, "dev/null", "/outside").unwrap_err();
    assert!(err.contains("dev/null"), "{err}");
    assert!(err.contains("/outside"), "{err}");
    assert!(err.contains("mknod"), "{err}");
    assert!(err.contains("--strict"), "{err}");
    let _ = std::fs::remove_dir_all(&d);
}

/// T-0415: the removable-link arm through the same helper: the
/// link alone goes, and a target outside the rootfs stays
/// untouched.
#[test]
fn a_removable_link_is_displaced_and_named() {
    let d = scratch("displaced");
    std::fs::create_dir_all(format!("{d}/dev")).unwrap();
    std::fs::write(format!("{d}/outside"), b"stay").unwrap();
    std::os::unix::fs::symlink(format!("{d}/outside"), format!("{d}/dev/null")).unwrap();
    let root = Root::open(&d).unwrap();
    let displaced = displace_link(&root, "dev/null", &format!("{d}/outside")).unwrap();
    assert!(displaced.contains("symlink"), "{displaced}");
    assert!(displaced.contains("outside"), "{displaced}");
    assert!(std::fs::symlink_metadata(format!("{d}/dev/null")).is_err());
    assert_eq!(std::fs::read(format!("{d}/outside")).unwrap(), b"stay");
    let _ = std::fs::remove_dir_all(&d);
}

/// T-0413: absent `/dev` spellings are staged as the conventional
/// links, a created link is not a degradation, and a second pass
/// leaves every one of them alone.
#[test]
fn absent_fd_spellings_are_staged_as_conventional_links() {
    let d = scratch("fdlinks");
    std::fs::create_dir_all(format!("{d}/dev")).unwrap();
    let root = Root::open(&d).unwrap();
    let mut out = Vec::new();
    fd_links(&root, &mut out).unwrap();
    assert_eq!(out.len(), 4, "{out:?}");
    for f in &out {
        assert_eq!(f.entry, "T-0413", "{f:?}");
        assert_eq!(f.action, Action::Created, "{f:?}");
        assert!(!f.degraded, "{f:?}");
    }
    assert_eq!(
        std::fs::read_link(format!("{d}/dev/stdin")).unwrap(),
        std::path::PathBuf::from("/proc/self/fd/0")
    );
    assert_eq!(
        std::fs::read_link(format!("{d}/dev/fd")).unwrap(),
        std::path::PathBuf::from("/proc/self/fd")
    );
    let mut again = Vec::new();
    fd_links(&root, &mut again).unwrap();
    assert!(
        again.iter().all(|f| f.action == Action::Unchanged),
        "{again:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// T-0413: the image's own link is never retargeted, and anything
/// else at the path refuses naming the shape and the `--strict`
/// gate.
#[test]
fn foreign_shapes_at_fd_spellings_refuse_by_name() {
    let d = scratch("fdforeign");
    std::fs::create_dir_all(format!("{d}/dev")).unwrap();
    std::os::unix::fs::symlink("/elsewhere", format!("{d}/dev/stdin")).unwrap();
    std::fs::create_dir_all(format!("{d}/dev/fd")).unwrap();
    let root = Root::open(&d).unwrap();
    let mut out = Vec::new();
    fd_links(&root, &mut out).unwrap();
    assert_eq!(
        std::fs::read_link(format!("{d}/dev/stdin")).unwrap(),
        std::path::PathBuf::from("/elsewhere")
    );
    let kept = out.iter().find(|f| f.path == "dev/stdin").unwrap();
    assert_eq!(kept.action, Action::Unchanged, "{kept:?}");
    assert!(kept.detail.contains("/elsewhere"), "{}", kept.detail);
    let refused = out.iter().find(|f| f.path == "dev/fd").unwrap();
    assert_eq!(refused.action, Action::Failed, "{refused:?}");
    assert!(refused.degraded, "{refused:?}");
    assert!(refused.detail.contains("directory"), "{}", refused.detail);
    assert!(refused.detail.contains("--strict"), "{}", refused.detail);
    let _ = std::fs::remove_dir_all(&d);
}
