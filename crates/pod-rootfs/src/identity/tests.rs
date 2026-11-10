use super::*;
use crate::testing::{Scratch, no_in_root};

/// Six measured shapes of the `passwd` line in eleven images, through the
/// editor. Each string is what one `/etc/nsswitch.conf` says.
#[test]
fn every_measured_passwd_shape_ends_up_naming_files_first() {
    let measured = [
        "passwd: files systemd",
        "passwd:         compat systemd",
        "passwd:     files systemd",
        "passwd:     files sss systemd",
        "passwd: files",
        "passwd:\tfiles",
    ];
    for m in measured {
        let got = prepend_files(m).unwrap_or_else(|| m.to_string());
        let services = got.split_once(':').unwrap().1;
        assert_eq!(services.split_whitespace().next(), Some("files"), "{m} -> {got}");
        assert_eq!(services.split_whitespace().filter(|w| *w == "files").count(), 1, "{m} -> {got}");
    }
}

/// `compat` does NOT read the supplied file. Each other service of the line
/// stays.
#[test]
fn compat_gets_files_in_front_and_keeps_the_rest() {
    let got = prepend_files("passwd:         compat systemd\ngroup: compat\nhosts: dns\n").unwrap();
    assert!(got.contains("passwd:      files compat systemd\n"), "{got}");
    assert!(got.contains("group:      files compat\n"), "{got}");
    assert!(got.contains("hosts: dns\n"), "{got}");
}

#[test]
fn a_line_already_naming_files_first_is_left_byte_identical() {
    assert!(prepend_files("passwd:     files systemd\ngroup:      files\n").is_none());
}

#[test]
fn a_trailing_comment_survives_the_edit() {
    let got = prepend_files("passwd: sss # the site directory\n").unwrap();
    assert!(got.contains("files sss # the site directory"), "{got}");
}

/// A musl root gets no `nsswitch.conf`.
#[test]
fn a_musl_rootfs_gets_no_nsswitch_conf() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("id-musl");
    let fs = run_nsswitch(&d.root(), Libc::Musl, &Names::default()).unwrap();
    assert_eq!(fs[0].action, Action::Skipped);
    assert!(!d.at("etc/nsswitch.conf").exists());
}

/// A glibc root with no file gets one.
#[test]
fn a_glibc_rootfs_with_no_nsswitch_conf_gets_a_minimal_one() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("id-glibc");
    let fs = run_nsswitch(&d.root(), Libc::Glibc, &Names::default()).unwrap();
    assert_eq!(fs[0].action, Action::Created);
    let got = std::fs::read_to_string(d.at("etc/nsswitch.conf")).unwrap();
    assert!(got.contains("passwd:     files"), "{got}");
    assert!(got.contains("pod-rootfs"), "{got}");
}

/// A read-only `/etc/passwd` stops an install; the mode changes and the
/// content does not.
#[test]
fn a_read_only_passwd_is_made_writable_without_changing_a_byte() {
    use std::os::unix::fs::PermissionsExt;
    if no_in_root() {
        return;
    }
    let d = Scratch::new("id-ro");
    let p = d.at("etc/passwd");
    std::fs::write(&p, b"root:x:0:0:root:/root:/bin/sh\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o444)).unwrap();
    let fs = run_identity(&d.root()).unwrap();
    let f = fs.iter().find(|f| f.path == "etc/passwd").unwrap();
    assert_eq!(f.action, Action::Rewrote);
    assert_eq!(std::fs::read(&p).unwrap(), b"root:x:0:0:root:/root:/bin/sh\n");
    assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o200, 0o200);
}

#[test]
fn an_absent_passwd_is_synthesized_and_marked_degraded() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("id-nopasswd");
    let fs = run_identity(&d.root()).unwrap();
    assert!(fs.iter().all(|f| f.action == Action::Created));
    assert!(fs.iter().all(|f| f.degraded));
    let got = std::fs::read_to_string(d.at("etc/passwd")).unwrap();
    assert!(got.starts_with("root:x:0:0:"), "{got}");
}

/// The C library of each architecture of the nightly. podvm's probe knew
/// the paths of x86_64 and aarch64 only, so a glibc root of armv7, i686,
/// riscv64, powerpc64le, s390x or loongarch64 read as `unknown`, and a root
/// with no `nsswitch.conf` got none.
#[test]
fn the_c_library_is_found_on_each_architecture() {
    if no_in_root() {
        return;
    }
    let cases = [
        ("usr/lib/arm-linux-gnueabihf/libc.so.6", Libc::Glibc),
        ("lib/i386-linux-gnu/libc.so.6", Libc::Glibc),
        ("usr/lib/riscv64-linux-gnu/libc.so.6", Libc::Glibc),
        ("usr/lib/powerpc64le-linux-gnu/libc.so.6", Libc::Glibc),
        ("lib/s390x-linux-gnu/libc.so.6", Libc::Glibc),
        ("usr/lib64/libc.so.6", Libc::Glibc),
        ("lib/ld-musl-riscv64.so.1", Libc::Musl),
        ("lib/ld-musl-armhf.so.1", Libc::Musl),
        ("usr/lib/ld-musl-loongarch64.so.1", Libc::Musl),
        ("lib/libc.musl-x86.so.1", Libc::Musl),
        ("usr/lib/libfoo.so.1", Libc::Unknown),
    ];
    for (i, (file, want)) in cases.iter().enumerate() {
        let d = Scratch::new(&format!("id-libc-{i}"));
        let at = d.at(file);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(&at, b"\x7fELF").unwrap();
        assert_eq!(probe_libc(&d.root()).unwrap(), *want, "{file}");
    }
}

/// `/lib -> usr/lib` (debian 12, void): the probe reads through the link.
#[test]
fn the_c_library_is_found_through_a_lib_link() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("id-libc-link");
    std::fs::create_dir_all(d.at("usr/lib/x86_64-linux-gnu")).unwrap();
    std::fs::write(d.at("usr/lib/x86_64-linux-gnu/libc.so.6"), b"\x7fELF").unwrap();
    std::os::unix::fs::symlink("usr/lib", d.at("lib")).unwrap();
    assert_eq!(probe_libc(&d.root()).unwrap(), Libc::Glibc);
}

/// A `/etc/passwd`, `/etc/group` or `/etc/nsswitch.conf` of the image behind
/// a symlink inside the root is the file of the image. podvm's code read it
/// with no follow, took it as absent, and replaced the users and the
/// services of the image with its minimal files.
#[test]
fn a_file_of_the_image_behind_a_link_is_kept() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("id-linked");
    std::fs::create_dir_all(d.at("usr/share/base")).unwrap();
    let users = "root:x:0:0:root:/root:/bin/sh\napp:x:1000:1000::/home/app:/bin/sh\n";
    std::fs::write(d.at("usr/share/base/passwd"), users).unwrap();
    std::fs::write(d.at("usr/share/base/group"), "root:x:0:\napp:x:1000:\n").unwrap();
    let services = "passwd: files\ngroup: files\nhosts: files mdns4_minimal [NOTFOUND=return] dns\n";
    std::fs::write(d.at("usr/share/base/nsswitch.conf"), services).unwrap();
    for name in ["passwd", "group", "nsswitch.conf"] {
        std::os::unix::fs::symlink(format!("/usr/share/base/{name}"), d.at(&format!("etc/{name}"))).unwrap();
    }
    let root = d.root();
    let fs = run_identity(&root).unwrap();
    assert!(fs.iter().all(|f| f.action == Action::Unchanged && !f.degraded), "{fs:#?}");
    // The links are absolute, so the host reads their targets in the root.
    assert!(std::fs::symlink_metadata(d.at("etc/passwd")).unwrap().is_symlink());
    assert_eq!(std::fs::read_to_string(d.at("usr/share/base/passwd")).unwrap(), users);
    let fs = run_nsswitch(&root, Libc::Glibc, &Names::default()).unwrap();
    assert_eq!(fs[0].action, Action::Unchanged, "{fs:#?}");
    assert!(std::fs::symlink_metadata(d.at("etc/nsswitch.conf")).unwrap().is_symlink());
    assert_eq!(std::fs::read_to_string(d.at("usr/share/base/nsswitch.conf")).unwrap(), services);
}

/// A fifo at `/etc/nsswitch.conf` does not stop the completion. podvm's
/// code opened it with no `O_NONBLOCK` and waited for a writer for ever.
#[test]
fn a_fifo_at_a_configuration_path_does_not_stop_the_completion() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("id-fifo");
    crate::testing::mkfifo(&d.at("etc/nsswitch.conf"));
    let path = d.path().to_path_buf();
    let done = crate::testing::bounded(move || {
        let root = crate::write::Root::open(&path).unwrap();
        run_nsswitch(&root, Libc::Glibc, &Names::default()).map(|fs| fs[0].action)
    });
    assert_eq!(done.expect("the completion waited on the fifo").unwrap(), Action::Rewrote);
    assert!(d.at("etc/nsswitch.conf").is_file());
}
