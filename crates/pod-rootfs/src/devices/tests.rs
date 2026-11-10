use super::*;
use crate::testing::{Scratch, no_in_root};

fn zeros() -> Vec<u8> {
    vec![0u8; FILLED_BYTES]
}

fn settings() -> Settings {
    Settings::default()
}

/// The failure of a `/dev/null` that grew, on the SHIM arm: `shim_for`, not
/// `run`. `run` asks the kernel, and on a host that permits `mknod` there is
/// no shim to check.
#[test]
fn a_grown_dev_null_is_truncated_and_the_growth_is_reported() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("grown");
    std::fs::create_dir_all(d.at("dev")).unwrap();
    std::fs::write(d.at("dev/null"), vec![b'x'; 4096]).unwrap();
    let fs = shim_for(&d.root(), "dev/null", "EPERM", None, &zeros(), &zeros()).unwrap();
    assert_eq!(fs[0].action, Action::Rewrote);
    assert!(fs[0].detail.contains("4096 bytes"), "{}", fs[0].detail);
    assert!(fs[0].detail.contains("EPERM"), "{}", fs[0].detail);
    assert_eq!(std::fs::metadata(d.at("dev/null")).unwrap().len(), 0);
}

/// Each shim is degraded, and each row names the errno of `mknod`.
#[test]
fn every_shim_is_reported_as_a_degradation_and_names_the_errno() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("degraded");
    let root = d.root();
    root.mkdirs("dev").unwrap();
    let (z, r) = (zeros(), zeros());
    for (path, ..) in DEVICES {
        let fs = shim_for(&root, path, "EPERM", None, &z, &r).unwrap();
        assert_eq!(fs.len(), 1, "{path}");
        assert!(fs[0].degraded, "{path}: {:?}", fs[0]);
        assert!(fs[0].detail.contains("EPERM"), "{path}: {:?}", fs[0]);
    }
    assert_eq!(std::fs::metadata(d.at("dev/zero")).unwrap().len() as usize, FILLED_BYTES);
}

/// The second pass does not write a megabyte again.
#[test]
fn a_second_pass_leaves_the_filled_shims_alone() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("idem");
    let root = d.root();
    root.mkdirs("dev").unwrap();
    let (z, r) = (zeros(), zeros());
    for p in ["dev/zero", "dev/urandom", "dev/random"] {
        shim_for(&root, p, "EPERM", None, &z, &r).unwrap();
        let fs = shim_for(&root, p, "EPERM", None, &z, &r).unwrap();
        assert_eq!(fs[0].action, Action::Unchanged, "{p}");
    }
}

/// True on each host, whichever arm it takes: a row claims a real device
/// only where a real device is on disk, and a row that is a file is
/// degraded.
#[test]
fn what_each_row_claims_is_what_is_on_disk() {
    use std::os::unix::fs::FileTypeExt;
    if no_in_root() {
        return;
    }
    let d = Scratch::new("claims");
    let root = d.root();
    let fs = run(&root, &settings()).unwrap();
    assert_eq!(fs.len(), DEVICES.len() + FD_LINKS.len(), "{fs:#?}");
    for f in &fs {
        let md = std::fs::symlink_metadata(d.at(&f.path)).unwrap();
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
            "dev-fd-link" => {
                let want = FD_LINKS.iter().find(|(p, _)| *p == f.path).unwrap().1;
                assert_eq!(std::fs::read_link(d.at(&f.path)).unwrap(), std::path::PathBuf::from(want), "{f:?}");
                assert!(!f.degraded, "{f:?} is a usual link and is marked degraded");
            }
            other => panic!("unknown fixup id {other:?}"),
        }
    }
    // A second run changes nothing, on either arm.
    for f in run(&root, &settings()).unwrap() {
        assert_eq!(f.action, Action::Unchanged, "{f:?}");
    }
}

/// A directory where a node belongs is a failed row that names it, and
/// the directory stays.
#[test]
fn a_directory_at_a_device_path_is_a_named_failure() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("dirshape");
    std::fs::create_dir_all(d.at("dev/null")).unwrap();
    let fs = run(&d.root(), &settings()).unwrap();
    let f = fs.iter().find(|f| f.id == "dev-shim" && f.path == "dev/null").unwrap();
    assert_eq!(f.action, Action::Failed, "{f:?}");
    assert!(f.degraded, "{f:?}");
    assert!(f.detail.contains("dev/null"), "{}", f.detail);
    assert!(f.detail.contains("directory"), "{}", f.detail);
    assert!(std::fs::metadata(d.at("dev/null")).unwrap().is_dir());
}

/// A symlink where a node belongs is replaced, and the row names it. Only
/// the link goes: a target outside the root stays.
#[test]
fn a_symlink_at_a_device_path_is_replaced_and_named() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("symshape");
    std::fs::create_dir_all(d.at("dev")).unwrap();
    std::fs::write(d.at("outside"), b"stay").unwrap();
    std::os::unix::fs::symlink(d.at("outside"), d.at("dev/null")).unwrap();
    let fs = run(&d.root(), &settings()).unwrap();
    let f = fs.iter().find(|f| f.path == "dev/null" && f.id != "dev-fd-link").unwrap();
    // The host decides the arm: a real node where `mknod` is permitted,
    // else a shim; each row names the link that it replaced.
    assert_eq!(f.action, Action::Created, "{f:?}");
    assert!(f.detail.contains("symlink"), "{}", f.detail);
    assert!(f.detail.contains("outside"), "{}", f.detail);
    let meta = std::fs::symlink_metadata(d.at("dev/null")).unwrap();
    if f.id == "dev-node" {
        use std::os::unix::fs::FileTypeExt;
        assert!(!f.degraded, "{f:?}");
        assert!(meta.file_type().is_char_device(), "{meta:?}");
    } else {
        assert!(f.degraded, "{f:?}");
        assert!(meta.is_file(), "{meta:?}");
        assert_eq!(meta.len(), 0);
    }
    assert_eq!(std::fs::read(d.at("outside")).unwrap(), b"stay");
}

/// A fifo where a node belongs is replaced, and the row names it: a fifo
/// stops a writer with no reader.
#[test]
fn a_fifo_at_a_device_path_is_replaced_and_named() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("fifoshape");
    std::fs::create_dir_all(d.at("dev")).unwrap();
    crate::testing::mkfifo(&d.at("dev/zero"));
    let fs = run(&d.root(), &settings()).unwrap();
    let f = fs.iter().find(|f| f.path == "dev/zero").unwrap();
    assert_eq!(f.action, Action::Rewrote, "{f:?}");
    assert!(f.detail.contains("a fifo"), "{}", f.detail);
    let meta = std::fs::symlink_metadata(d.at("dev/zero")).unwrap();
    if f.id == "dev-node" {
        use std::os::unix::fs::FileTypeExt;
        assert!(meta.file_type().is_char_device(), "{meta:?}");
    } else {
        assert_eq!(meta.len() as usize, FILLED_BYTES);
    }
}

/// The refusal of [`displace_link`] when the unlink fails. A directory is
/// what each user cannot unlink (root passes the permission bits). The
/// refusal names the path, the target and the risk of `mknod`.
#[test]
fn an_unremovable_link_is_a_named_refusal() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("unstated");
    std::fs::create_dir_all(d.at("dev/null")).unwrap();
    let err = displace_link(&d.root(), "dev/null", "/outside").unwrap_err();
    assert!(err.contains("dev/null"), "{err}");
    assert!(err.contains("/outside"), "{err}");
    assert!(err.contains("mknod"), "{err}");
    assert!(err.contains("refused"), "{err}");
}

/// The other arm of the same helper: the link alone goes, and its target
/// outside the root stays.
#[test]
fn a_removable_link_is_displaced_and_named() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("displaced");
    std::fs::create_dir_all(d.at("dev")).unwrap();
    std::fs::write(d.at("outside"), b"stay").unwrap();
    std::os::unix::fs::symlink(d.at("outside"), d.at("dev/null")).unwrap();
    let target = d.at("outside").to_string_lossy().to_string();
    let displaced = displace_link(&d.root(), "dev/null", &target).unwrap();
    assert!(displaced.contains("symlink"), "{displaced}");
    assert!(displaced.contains("outside"), "{displaced}");
    assert!(std::fs::symlink_metadata(d.at("dev/null")).is_err());
    assert_eq!(std::fs::read(d.at("outside")).unwrap(), b"stay");
}

/// The absent `/dev` spellings become the usual links, a new link is not
/// degraded, and a second pass leaves each one alone.
#[test]
fn absent_fd_spellings_are_staged_as_conventional_links() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("fdlinks");
    std::fs::create_dir_all(d.at("dev")).unwrap();
    let root = d.root();
    let mut out = Vec::new();
    fd_links(&root, &mut out).unwrap();
    assert_eq!(out.len(), 4, "{out:?}");
    for f in &out {
        assert_eq!(f.id, "dev-fd-link", "{f:?}");
        assert_eq!(f.action, Action::Created, "{f:?}");
        assert!(!f.degraded, "{f:?}");
    }
    assert_eq!(std::fs::read_link(d.at("dev/stdin")).unwrap(), std::path::PathBuf::from("/proc/self/fd/0"));
    assert_eq!(std::fs::read_link(d.at("dev/fd")).unwrap(), std::path::PathBuf::from("/proc/self/fd"));
    let mut again = Vec::new();
    fd_links(&root, &mut again).unwrap();
    assert!(again.iter().all(|f| f.action == Action::Unchanged), "{again:?}");
}

/// The link of the image is never changed, and each other thing at the path
/// is a failed row that names its shape.
#[test]
fn foreign_shapes_at_fd_spellings_refuse_by_name() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("fdforeign");
    std::fs::create_dir_all(d.at("dev")).unwrap();
    std::os::unix::fs::symlink("/elsewhere", d.at("dev/stdin")).unwrap();
    std::fs::create_dir_all(d.at("dev/fd")).unwrap();
    let mut out = Vec::new();
    fd_links(&d.root(), &mut out).unwrap();
    assert_eq!(std::fs::read_link(d.at("dev/stdin")).unwrap(), std::path::PathBuf::from("/elsewhere"));
    let kept = out.iter().find(|f| f.path == "dev/stdin").unwrap();
    assert_eq!(kept.action, Action::Unchanged, "{kept:?}");
    assert!(kept.detail.contains("/elsewhere"), "{}", kept.detail);
    let refused = out.iter().find(|f| f.path == "dev/fd").unwrap();
    assert_eq!(refused.action, Action::Failed, "{refused:?}");
    assert!(refused.degraded, "{refused:?}");
    assert!(refused.detail.contains("directory"), "{}", refused.detail);
}

/// An image whose `dev` is a symlink to an absolute path outside the root.
/// `RESOLVE_IN_ROOT` rebases the link onto the root, so the completion works
/// in `<root>/<that path>`; a host path through `<root>/dev` reaches the
/// HOST directory. podvm's code removed a symlink and made the `/dev`
/// links through such a host path: it removed `null` and made `stdin` in a
/// host directory that the image named.
#[test]
fn a_dev_that_links_out_of_the_root_changes_nothing_on_the_host() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("devout");
    let host = Scratch::new("devout-host");
    std::fs::write(host.at("null"), b"a file of the host").unwrap();
    let host_dir = host.path().to_str().unwrap().trim_start_matches('/').to_string();
    // The same path inside the root, with a symlink at `null`: the image
    // shape that sends the unlink to the host.
    std::fs::create_dir_all(d.at(&host_dir)).unwrap();
    std::os::unix::fs::symlink("/nowhere", d.path().join(&host_dir).join("null")).unwrap();
    std::os::unix::fs::symlink(host.path(), d.at("dev")).unwrap();

    let fs = run(&d.root(), &settings()).unwrap();
    assert_eq!(std::fs::read(host.at("null")).unwrap(), b"a file of the host", "the host file was removed: {fs:#?}");
    for (path, _) in FD_LINKS {
        let name = path.trim_start_matches("dev/");
        assert!(std::fs::symlink_metadata(host.at(name)).is_err(), "{name} was made on the host: {fs:#?}");
        assert!(std::fs::symlink_metadata(d.path().join(&host_dir).join(name)).is_ok(), "{name} is not in the root");
    }
}

/// A root on a file system mounted `nodev`: `mknod` makes the node, and each
/// open of it gives `EACCES`, so `cmd >/dev/null` fails in the payload.
/// podvm's code reported such a node as a real device. The test mounts a
/// `nodev` tmpfs in a mount namespace of its own thread, so it needs root.
#[test]
fn a_node_that_does_not_open_is_replaced_by_a_shim() {
    use pod_hostprobe::sys;
    const MS_NODEV: u64 = 4;
    pod_testkit::need_root!();
    if no_in_root() {
        return;
    }
    let d = Scratch::new("nodev");
    let path = d.path().to_path_buf();
    let got = std::thread::spawn(move || -> std::result::Result<Vec<Fixup>, String> {
        let c = |s: &str| sys::CBuf::new(s).unwrap();
        let at = c(path.to_str().unwrap());
        // The thread gets a mount namespace of its own; no mount of the host
        // changes.
        sys::unshare(sys::CLONE_NEWNS).map_err(|e| format!("this host refuses unshare(CLONE_NEWNS): {}", e.name()))?;
        sys::mount(&c("none"), &c("/"), &c(""), sys::MS_REC | sys::MS_PRIVATE)
            .map_err(|e| format!("this host refuses a private /: {}", e.name()))?;
        sys::mount(&c("tmpfs"), &at, &c("tmpfs"), MS_NODEV)
            .map_err(|e| format!("this host refuses a nodev tmpfs: {}", e.name()))?;
        let fs = run(&Root::open(&path).unwrap(), &Settings::default()).map_err(|e| format!("the run failed: {e}"));
        let _ = sys::umount(&at);
        fs.map_err(|e| format!("FAIL {e}"))
    })
    .join()
    .unwrap();
    let fs = match got {
        Ok(fs) => fs,
        Err(why) if !why.starts_with("FAIL") => return pod_testkit::host::skip_line(&why),
        Err(why) => panic!("{why}"),
    };
    for (path, ..) in DEVICES {
        let f = fs.iter().find(|f| f.path == *path).unwrap();
        assert_eq!(f.id, "dev-shim", "{f:?}");
        assert!(f.degraded, "{f:?}");
        assert!(f.detail.contains("nodev"), "{f:?}");
    }
}
