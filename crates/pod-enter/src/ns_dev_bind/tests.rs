use super::*;

/// A regular file or no file takes the bind; a missing one first gets an
/// empty file as the mount point.
#[test]
fn a_regular_file_or_no_file_takes_the_bind() {
    assert_eq!(choose(Kind::Regular, true), Choice::Bind { create: false });
    assert_eq!(choose(Kind::CharDevice, true), Choice::Bind { create: false });
    assert_eq!(choose(Kind::Missing, true), Choice::Bind { create: true });
}

/// A link, a directory, another kind and an unreadable path take no bind,
/// and the line says why.
#[test]
fn a_link_a_directory_or_another_kind_takes_no_bind() {
    for (kind, why) in [
        (Kind::Link, "symbolic link"),
        (Kind::Directory, "directory"),
        (Kind::Other, "another kind"),
        (Kind::Unreadable, "cannot read"),
    ] {
        match choose(kind, true) {
            Choice::Skip(w) => assert!(w.contains(why), "{kind:?}: {w}"),
            c => panic!("{kind:?} gives {c:?}"),
        }
    }
}

/// Where the host's node is not a character device, no path takes it.
#[test]
fn no_bind_of_a_host_node_that_is_not_a_character_device() {
    for kind in [Kind::Regular, Kind::Missing, Kind::CharDevice] {
        assert_eq!(choose(kind, false), Choice::Skip("the host's node is not a character device"), "{kind:?}");
    }
}

/// The kinds of the modes of `stat`.
#[test]
fn the_kind_of_each_mode() {
    assert_eq!(kind_of(sys::S_IFREG | 0o644), Kind::Regular);
    assert_eq!(kind_of(sys::S_IFCHR as u32 | 0o666), Kind::CharDevice);
    assert_eq!(kind_of(sys::S_IFLNK | 0o777), Kind::Link);
    assert_eq!(kind_of(sys::S_IFDIR | 0o755), Kind::Directory);
    assert_eq!(kind_of(sys::S_IFIFO as u32 | 0o644), Kind::Other);
    assert_eq!(kind_of(0o060000 | 0o660), Kind::Other, "a block device");
}

/// The remount adds `nosuid` and `noexec`, never `nodev`, and keeps a
/// read-only node read-only.
#[test]
fn the_remount_keeps_the_devices_working() {
    let f = remount_flags(false);
    for flag in [sys::MS_REMOUNT, sys::MS_BIND, sys::MS_NOSUID, sys::MS_NOEXEC] {
        assert_eq!(f & flag, flag, "{flag:#x}");
    }
    assert_eq!(f & sys::MS_NODEV, 0, "a device node must work");
    assert_eq!(f & (sys::MS_RDONLY | sys::MS_REC), 0);
    assert_eq!(remount_flags(true) & sys::MS_RDONLY, sys::MS_RDONLY);
}

/// The path of a descriptor is its magic link, with its NUL.
#[test]
fn the_path_of_a_descriptor() {
    let mut buf = [0xffu8; 32];
    fd_path(1234, &mut buf);
    let end = buf.iter().position(|&b| b == 0).unwrap();
    assert_eq!(&buf[..end], b"/proc/self/fd/1234");
}

/// The steps have bytes of their own, after each step before them, and
/// names.
#[test]
fn the_steps_have_bytes_and_names_of_their_own() {
    let taken: Vec<u8> = (0..NS_DEV_POINT_STEP).collect();
    for b in [NS_DEV_POINT_STEP, NS_DEV_BIND_STEP, NS_DEV_REMOUNT_STEP] {
        assert!(!taken.contains(&b), "{b}");
        assert!(ns_setup_step(b).is_some(), "{b}");
    }
    assert!(crate::ns_setup_step(crate::NS_PROC_REMOUNT_STEP).is_some());
    let line = refused_start(&Names::DEFAULT, "urandom", NS_DEV_BIND_STEP);
    assert!(
        line.ends_with(
            "the namespace rung keeps the image's /dev/urandom: binding the host's device failed with errno "
        ),
        "{line}"
    );
}

/// An image whose `/dev` holds a regular file, a link and a directory, and
/// misses the others: the binds and the lines, read through the
/// descriptor of the root.
#[test]
fn the_binds_of_an_image() {
    let root = std::env::temp_dir().join(format!("pod-enter-dev-bind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("dev/random")).unwrap();
    std::fs::write(root.join("dev/urandom"), b"x").unwrap();
    std::os::unix::fs::symlink("/etc/hostname", root.join("dev/zero")).unwrap();
    let dir = crate::RootDir::open(root.to_str().unwrap()).unwrap();
    let d = devices(&dir, &Names::DEFAULT, false);
    let host = |n: &str| host_node(&CBuf::new(&format!("/dev/{n}")).unwrap()).is_some();
    let bound: Vec<&str> = d.binds.iter().map(|b| b.label).collect();
    let skipped: Vec<String> = d.skipped.iter().map(|l| String::from_utf8_lossy(l).into_owned()).collect();
    assert!(skipped.iter().any(|l| l.contains("/dev/zero: the image holds a symbolic link")), "{skipped:?}");
    assert!(skipped.iter().any(|l| l.contains("/dev/random: the image holds a directory")), "{skipped:?}");
    for n in ["null", "full", "urandom", "tty"] {
        assert_eq!(bound.contains(&n), host(n), "{n}: {skipped:?}");
    }
    for b in &d.binds {
        assert_eq!(b.create, b.label != "urandom");
        assert_eq!(b.remount & sys::MS_NODEV, 0);
    }
    // `/dev` as a link: one line, and no bind.
    std::fs::remove_dir_all(root.join("dev")).unwrap();
    std::os::unix::fs::symlink("/dev", root.join("dev")).unwrap();
    let d = devices(&dir, &Names::DEFAULT, false);
    assert!(d.binds.is_empty());
    assert_eq!(d.skipped, vec![no_dev_line(&Names::DEFAULT).into_bytes()]);
    // The tmpfs of a read-only root (T-1639): each path is missing there,
    // whatever the image holds, so each node of the host takes a bind.
    let d = devices(&dir, &Names::DEFAULT, true);
    assert!(d.skipped.iter().all(|l| String::from_utf8_lossy(l).contains("not a character device")));
    for b in &d.binds {
        assert!(b.create, "{}", b.label);
    }
    assert_eq!(d.binds.len(), DEVICES.iter().filter(|n| host(n)).count());
    let _ = std::fs::remove_dir_all(&root);
}

/// `/dev/ptmx` takes a bind only where the image has a `/dev/pts`
/// directory: of the `ptmx` of the devpts there, never of the host's
/// `/dev/ptmx`. A link at `/dev/ptmx` takes none.
#[test]
fn the_binds_of_ptmx() {
    let root = std::env::temp_dir().join(format!("pod-enter-dev-bind-ptmx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("dev")).unwrap();
    let dir = crate::RootDir::open(root.to_str().unwrap()).unwrap();
    assert!(devices(&dir, &Names::DEFAULT, false).ptmx.is_none(), "no /dev/pts: no bind of /dev/ptmx");
    std::fs::create_dir_all(root.join("dev/pts")).unwrap();
    let d = devices(&dir, &Names::DEFAULT, false);
    let p = d.ptmx.expect("a /dev/pts: a bind of /dev/ptmx");
    let own = p.as_ref().expect("the ptmx of the devpts on /dev/pts");
    assert_eq!(text(&own.source), format!("{}/dev/pts/ptmx", root.display()));
    assert!(own.create, "the image has no /dev/ptmx");
    assert_eq!(own.remount & (sys::MS_NODEV | sys::MS_RDONLY), 0);
    let line = String::from_utf8_lossy(&own.bind_refused).into_owned();
    assert!(line.contains("/dev/ptmx: binding the ptmx of the devpts on /dev/pts failed"), "{line}");
    std::os::unix::fs::symlink("pts/ptmx", root.join("dev/ptmx")).unwrap();
    let p = devices(&dir, &Names::DEFAULT, false).ptmx.unwrap();
    let line = String::from_utf8_lossy(p.as_ref().err().expect("no bind over a link")).into_owned();
    assert!(line.contains("/dev/ptmx: the image holds a symbolic link"), "{line}");
    let _ = std::fs::remove_dir_all(&root);
}

/// The text of a C string of a plan.
fn text(c: &CBuf) -> String {
    // SAFETY: a `CBuf` holds its NUL, and lives while the text is read.
    unsafe { std::ffi::CStr::from_ptr(c.ptr() as *const std::ffi::c_char) }.to_string_lossy().into_owned()
}
