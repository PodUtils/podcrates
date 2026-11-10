use super::*;
use crate::testing::{Scratch, no_in_root};

/// The link points at an absolute path outside the root; the write lands
/// inside, and the canary stays.
#[test]
fn a_write_through_an_absolute_symlink_does_not_escape() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("w-escape");
    let outside = Scratch::new("w-escape-canary");
    let canary = outside.at("canary");
    std::fs::write(&canary, b"canary\n").unwrap();
    std::os::unix::fs::symlink(&canary, d.at("etc/mtab")).unwrap();
    let r = d.root();
    assert!(matches!(r.kind("etc/mtab").unwrap(), Kind::Symlink(_)));
    assert_eq!(r.write("etc/mtab", b"inside\n", 0o644).unwrap(), Wrote::Replaced);
    assert_eq!(std::fs::read(&canary).unwrap(), b"canary\n");
    assert_eq!(std::fs::read(d.at("etc/mtab")).unwrap(), b"inside\n");
}

/// A DIRECTORY component that is a symlink out: the kernel refuses the
/// resolution, and nothing is written.
#[test]
fn a_write_through_a_symlinked_directory_is_refused() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("w-direscape");
    let outside = Scratch::new("w-outdir");
    std::os::unix::fs::symlink(outside.path(), d.at("evil")).unwrap();
    let e = d.root().write("evil/passwd", b"x\n", 0o644).unwrap_err();
    assert!(format!("{e}").contains("does not write through"), "{e}");
    assert!(!outside.at("passwd").exists());
}

/// The three answers: absent, a link inside, a link out.
#[test]
fn reach_tells_absent_from_a_link_inside_from_a_link_out() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("w-reach");
    let root = d.root();
    assert_eq!(root.reach("etc/pki").unwrap(), Reach::Absent);
    // The shape of openSUSE: a symlink to a real directory INSIDE.
    std::fs::create_dir_all(d.at("var/lib/certs")).unwrap();
    std::os::unix::fs::symlink("/var/lib/certs", d.at("etc/certs")).unwrap();
    assert_eq!(root.reach("etc/certs").unwrap(), Reach::Yes, "an internal directory link must be followed");
    // A symlink OUT of the root: rebased, it points at nothing, and the
    // answer says so.
    let out = Scratch::new("w-reach-out");
    std::os::unix::fs::symlink(out.path(), d.at("etc/away")).unwrap();
    match root.reach("etc/away").unwrap() {
        Reach::Unreachable(why) => {
            assert!(why.contains("etc/away"), "{why}");
            assert!(why.contains("symlink"), "{why}");
        }
        other => panic!("a link out of the root was not reported: {other:?}"),
    }
    // A plain file where a directory is wanted.
    std::fs::write(d.at("etc/afile"), b"x").unwrap();
    assert!(matches!(root.reach("etc/afile").unwrap(), Reach::Unreachable(_)));
}

#[test]
fn an_identical_write_reports_unchanged() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("w-idem");
    let r = d.root();
    assert_eq!(r.write("etc/hosts", b"a\n", 0o644).unwrap(), Wrote::Created);
    assert_eq!(r.write("etc/hosts", b"a\n", 0o644).unwrap(), Wrote::Unchanged);
    assert_eq!(r.write("etc/hosts", b"b\n", 0o644).unwrap(), Wrote::Replaced);
}

/// The link out is REPLACED by a regular file inside the root, so a later
/// reader does not follow it out again.
#[test]
fn a_write_replaces_an_out_link_with_a_regular_file() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("w-replace");
    let outside = Scratch::new("w-replace-canary");
    let canary = outside.at("canary");
    std::fs::write(&canary, b"canary\n").unwrap();
    std::os::unix::fs::symlink(&canary, d.at("etc/mtab")).unwrap();
    d.root().write("etc/mtab", b"inside\n", 0o644).unwrap();
    assert!(!std::fs::symlink_metadata(d.at("etc/mtab")).unwrap().is_symlink(), "etc/mtab is still a symlink");
    assert_eq!(std::fs::read(d.at("etc/mtab")).unwrap(), b"inside\n");
    assert_eq!(std::fs::read(&canary).unwrap(), b"canary\n");
}

/// A directory link that stays inside the root is FOLLOWED: the write
/// lands at its target.
#[test]
fn a_write_follows_a_directory_link_that_stays_inside() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("w-inside");
    std::fs::create_dir_all(d.at("var/aptreal/apt.conf.d")).unwrap();
    std::os::unix::fs::symlink("/var/aptreal", d.at("etc/apt")).unwrap();
    d.root().write("etc/apt/apt.conf.d/99drop-in", b"APT{}\n", 0o644).unwrap();
    assert_eq!(std::fs::read(d.at("var/aptreal/apt.conf.d/99drop-in")).unwrap(), b"APT{}\n");
}

/// A fifo at a path that the crate reads, directly or through a link: the
/// read gives `None` at once. podvm's code opened it with no `O_NONBLOCK`
/// and waited for a writer for ever, so an image stopped the completion.
#[test]
fn a_fifo_reads_as_no_file_and_does_not_wait() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("w-fifo");
    crate::testing::mkfifo(&d.at("etc/pacman.conf"));
    std::os::unix::fs::symlink("/etc/pacman.conf", d.at("etc/cert.pem")).unwrap();
    let path = d.path().to_path_buf();
    let got = crate::testing::bounded(move || {
        let root = Root::open(&path).unwrap();
        (
            root.read("etc/pacman.conf").unwrap(),
            root.read_following("etc/cert.pem").unwrap(),
            root.write_following("etc/cert.pem", b"x", 0o644).unwrap(),
        )
    });
    let (direct, followed, written) = got.expect("a read of a fifo waited for a writer");
    assert_eq!(direct, None);
    assert_eq!(followed, None);
    assert!(written.is_err(), "{written:?}");
}

/// The mode of a file is set through a descriptor that opened with
/// `O_NOFOLLOW`. podvm's code called `fchmodat` by name, which follows a
/// symlink that replaced the file, also to a file of the host.
#[test]
fn a_mode_change_does_not_follow_a_link() {
    use std::os::unix::fs::PermissionsExt;
    if no_in_root() {
        return;
    }
    let d = Scratch::new("w-mode");
    let outside = Scratch::new("w-mode-host");
    let host = outside.at("shadow");
    std::fs::write(&host, b"secret\n").unwrap();
    std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink(&host, d.at("etc/passwd")).unwrap();
    assert!(d.root().set_mode("etc/passwd", 0o666).is_err());
    assert_eq!(std::fs::metadata(&host).unwrap().permissions().mode() & 0o777, 0o600);
}

/// A file larger than the limit is a failure, not a long read.
#[test]
fn a_file_over_the_limit_is_refused() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("w-limit");
    std::fs::write(d.at("etc/big"), vec![b'x'; READ_LIMIT + 1]).unwrap();
    let e = d.root().read("etc/big").unwrap_err();
    assert_eq!(e.class, pod_core::Class::Policy, "{e}");
}

/// The device numbers of the kernel, in its wide encoding.
#[test]
fn a_device_number_splits_into_its_major_and_minor() {
    use pod_hostprobe::sys::makedev;
    for (major, minor) in [(1, 3), (1, 9), (259, 0), (8, 300)] {
        assert_eq!(split_dev(makedev(major, minor)), (major, minor));
    }
}
