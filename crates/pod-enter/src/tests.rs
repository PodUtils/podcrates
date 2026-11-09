use super::*;

#[test]
fn a_wait_status_becomes_dockers_exit_code() {
    // exited(0), exited(3), and SIGKILL(9).
    assert_eq!(exit_status(0), 0);
    assert_eq!(exit_status(3 << 8), 3);
    assert_eq!(exit_status(9), 137, "a SIGKILLed payload is 128+9");
    assert_eq!(exit_status(15), 143, "a SIGTERMed payload is 128+15");
}

#[test]
fn forwarded_signals_are_named_and_unknown_ones_numbered() {
    assert_eq!(signal_name(2), "SIGINT");
    assert_eq!(signal_name(15), "SIGTERM");
    assert_eq!(signal_name(9), "signal 9");
}

#[test]
fn the_shutdown_mask_holds_exactly_the_forwarded_pair() {
    // ⛔ Signal NUMBER n is bit index (n-1): SIGINT is bit 1 (value 2)
    // and SIGTERM is bit 14 (value 16384), so the word is 16386. The
    // lookalike `(1 << SIGINT) | (1 << SIGTERM)` names bits 2 and 15
    // (SIGQUIT and SIGSTKFLT): a mask with the wrong bits blocks
    // nothing that arrives, and the waiter dies on the real signal.
    // Measured 2026-09-23: the wrong word made the lane prove fail
    // every forwarded clause with empty stderr (T-1335).
    assert_eq!(sys::SHUTDOWN_SIGNALS, 2 | 16384);
}

#[test]
fn a_symlinked_rootfs_is_refused_and_names_its_target() {
    let dir = std::env::temp_dir().join(format!("podvm-enter-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("real")).unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(dir.join("real"), &link).unwrap();

    let e = RootDir::open(link.to_str().unwrap()).unwrap_err();
    let text = format!("{e}");
    assert!(text.contains("is a symlink"), "{text}");
    assert!(text.contains("T-0504"), "{text}");
    // ⛔ And the real directory is accepted, so the check is about the
    // symlink and not about the path being rejected wholesale.
    assert!(RootDir::open(dir.join("real").to_str().unwrap()).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_rootfs_that_is_not_a_directory_is_refused() {
    let dir = std::env::temp_dir().join(format!("podvm-enter-f-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("file");
    std::fs::write(&f, b"x").unwrap();
    let e = RootDir::open(f.to_str().unwrap()).unwrap_err();
    assert!(format!("{e}").contains("not a directory"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// TODO/enter.md T-1339: the entered rung follows the selection.
/// Namespace enters namespace now, not chroot; interpose still
/// enters userland and everything else the chroot sequence.
#[test]
fn the_entered_rung_follows_the_selection() {
    use podvm_probe::select::Rung as R;
    assert_eq!(entered_rung(R::Namespace), R::Namespace);
    assert_eq!(entered_rung(R::Interpose), R::Userland);
    assert_eq!(entered_rung(R::Chroot), R::Chroot);
    assert_eq!(entered_word(R::Namespace), "namespace");
    assert_eq!(entered_word(R::Chroot), "chroot");
}

/// TODO/enter.md T-1339: the /tmp mount point is a real directory.
/// Missing, a file, or a symlink (whose target is outside the
/// image) is no mount point, and the entry falls back.
#[test]
fn the_tmp_mount_point_is_a_real_directory() {
    let dir = std::env::temp_dir().join(format!("podvm-ns-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.to_str().unwrap();
    assert!(ns_tmp_target(root).is_none(), "no /tmp yet");
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    assert_eq!(ns_tmp_target(root), Some(format!("{root}/tmp")));
    std::fs::remove_dir(dir.join("tmp")).unwrap();
    std::fs::write(dir.join("tmp"), b"x").unwrap();
    assert!(ns_tmp_target(root).is_none(), "a file is no mount point");
    std::fs::remove_file(dir.join("tmp")).unwrap();
    std::os::unix::fs::symlink("/tmp", dir.join("tmp")).unwrap();
    assert!(ns_tmp_target(root).is_none(), "a link is no mount point");
    let _ = std::fs::remove_dir_all(&dir);
}

/// TODO/enter.md T-1339: the fallback names the cause and the rung
/// actually entered, in the words the drive asserts.
#[test]
fn the_fallback_line_names_the_cause_and_the_rung() {
    let line = ns_fallback_line("the namespace rung refused at unshare(CLONE_NEWNS) (EPERM (1))");
    assert!(line.contains("unshare(CLONE_NEWNS)"), "{line}");
    assert!(line.contains("entered chroot instead"), "{line}");
    assert!(line.contains("T-1339"), "{line}");
}

/// TODO/enter.md T-1339: every setup step has a name for the
/// fallback line, and no other byte reads as one. The parent maps
/// through this function, so a step it cannot name cannot arrive
/// unnamed on the banner.
#[test]
fn the_setup_steps_name_themselves() {
    assert_eq!(ns_setup_step(7), Some("unshare(CLONE_NEWNS)"));
    assert_eq!(ns_setup_step(8), Some("remounting / recursively private"));
    assert_eq!(ns_setup_step(9), Some("mounting tmpfs on /tmp"));
    assert_eq!(
        ns_setup_step(10),
        Some("verifying the chroot landed on the opened rootfs")
    );
    assert_eq!(ns_setup_step(0), None);
    assert_eq!(ns_setup_step(1), None);
    assert_eq!(ns_setup_step(2), None);
    assert_eq!(ns_setup_step(11), None);
}

/// TODO/enter.md T-1339: the (d,i) guard compares device and file.
/// Same device and file lands; either differing refuses.
#[test]
fn the_chroot_guard_compares_device_and_file() {
    let landed = sys::Stat {
        st_dev: 2096,
        st_ino: 39833,
        ..Default::default()
    };
    assert!(chroot_landed(&landed, &landed));
    let elsewhere = sys::Stat {
        st_dev: 2096,
        st_ino: 39834,
        ..Default::default()
    };
    assert!(!chroot_landed(&landed, &elsewhere));
    let other_fs = sys::Stat {
        st_dev: 146,
        st_ino: 39833,
        ..Default::default()
    };
    assert!(!chroot_landed(&landed, &other_fs));
}

/// TODO/enter.md T-1412: a missing workdir refuses naming the path and
/// the image, saying docker creates it and podvm refuses it instead.
#[test]
fn a_missing_workdir_is_a_sentence_naming_path_and_image() {
    let dir = std::env::temp_dir().join(format!("podvm-workdir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    let root = dir.to_str().unwrap();
    let e = check_workdir(root, "/no/such/dir", "img").unwrap_err();
    let text = format!("{e}");
    assert!(text.contains("/no/such/dir"), "{text}");
    assert!(text.contains("img"), "{text}");
    assert!(text.contains("docker creates"), "{text}");
    assert!(text.contains("podvm refuses"), "{text}");
    assert!(text.contains("T-1412"), "{text}");
    assert_eq!(e.exit_code(), EXIT_RUNTIME_ERROR);
    let _ = std::fs::remove_dir_all(&dir);
}

/// TODO/enter.md T-1412: the passing control. `/`, the empty default
/// and an image-held `/tmp` all pass, so the refusal above is about the
/// missing directory rather than about the flag.
#[test]
fn root_empty_and_tmp_workdirs_pass() {
    let dir = std::env::temp_dir().join(format!("podvm-workdir-ok-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    let root = dir.to_str().unwrap();
    assert!(check_workdir(root, "/", "img").is_ok());
    assert!(check_workdir(root, "", "img").is_ok());
    assert!(check_workdir(root, "/tmp", "img").is_ok());
    assert!(check_workdir(root, "/tmp/", "img").is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

/// TODO/enter.md T-1412: a file where a directory belongs refuses like
/// a missing one, and a `..` escaping the image refuses as an escape
/// rather than resolving onto the host.
#[test]
fn a_file_and_an_escape_are_no_workdir() {
    let dir = std::env::temp_dir().join(format!("podvm-workdir-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    std::fs::write(dir.join("file"), b"x").unwrap();
    let root = dir.to_str().unwrap();
    let e = check_workdir(root, "/file", "img").unwrap_err();
    assert!(format!("{e}").contains("/file"), "{e}");
    let e = check_workdir(root, "/../outside", "img").unwrap_err();
    let text = format!("{e}");
    assert!(text.contains("escapes"), "{text}");
    assert!(text.contains("T-1412"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// TODO/enter.md T-1412: a link to a directory inside the image resolves
/// the way the guest kernel resolves it, so it passes.
#[test]
#[cfg(unix)]
fn a_link_to_a_directory_passes() {
    let dir = std::env::temp_dir().join(format!("podvm-workdir-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("real")).unwrap();
    std::os::unix::fs::symlink("real", dir.join("link")).unwrap();
    let root = dir.to_str().unwrap();
    assert!(check_workdir(root, "/link", "img").is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

/// TODO/enter.md T-1412: a NUL byte is its own refusal rather than a
/// workdir sentence, because it never reaches the kernel.
#[test]
fn a_nul_workdir_is_its_own_refusal() {
    let dir = std::env::temp_dir().join(format!("podvm-workdir-nul-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    let root = dir.to_str().unwrap();
    let e = check_workdir(root, "/tmp\0x", "img").unwrap_err();
    assert!(format!("{e}").contains("NUL"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
