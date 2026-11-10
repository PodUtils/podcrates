use super::*;
use crate::status::signal_name;

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
    // Signal number n is bit n-1: SIGINT is bit 1 (value 2) and SIGTERM
    // is bit 14 (value 16384), so the word is 16386. The similar
    // `(1 << SIGINT) | (1 << SIGTERM)` names bits 2 and 15 (SIGQUIT and
    // SIGSTKFLT): a mask with the wrong bits blocks no signal that
    // arrives, and the waiter dies on the real signal.
    assert_eq!(sys::SHUTDOWN_SIGNALS, 2 | 16384);
}

#[test]
fn a_symlinked_rootfs_is_refused_and_names_its_target() {
    let dir = std::env::temp_dir().join(format!("pod-enter-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("real")).unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(dir.join("real"), &link).unwrap();

    let e = RootDir::open(link.to_str().unwrap()).unwrap_err();
    let text = format!("{e}");
    assert!(text.contains("is a symlink"), "{text}");
    assert!(text.contains("did not choose"), "{text}");
    // The real directory is accepted, so the check is about the link and
    // not about the path.
    assert!(RootDir::open(dir.join("real").to_str().unwrap()).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_rootfs_that_is_not_a_directory_is_refused() {
    let dir = std::env::temp_dir().join(format!("pod-enter-f-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("file");
    std::fs::write(&f, b"x").unwrap();
    let e = RootDir::open(f.to_str().unwrap()).unwrap_err();
    assert!(format!("{e}").contains("not a directory"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The entered rung follows the selection: namespace enters namespace,
/// interpose enters userland, and each other selection the chroot
/// sequence.
#[test]
fn the_entered_rung_follows_the_selection() {
    use pod_hostprobe::select::Rung as R;
    assert_eq!(entered_rung(R::Namespace), R::Namespace);
    assert_eq!(entered_rung(R::Interpose), R::Userland);
    assert_eq!(entered_rung(R::Chroot), R::Chroot);
    assert_eq!(entered_word(R::Namespace), "namespace");
    assert_eq!(entered_word(R::Chroot), "chroot");
}

/// The /tmp mount point is a real directory. Missing, a file, or a link
/// (whose target is outside the image) is no mount point, and the entry
/// falls back.
#[test]
fn the_tmp_mount_point_is_a_real_directory() {
    let dir = std::env::temp_dir().join(format!("pod-enter-ns-{}", std::process::id()));
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

/// The fallback names the cause and the rung that the entry entered.
#[test]
fn the_fallback_line_names_the_cause_and_the_rung() {
    let line = ns_fallback_line(&Names::DEFAULT, "the namespace rung refused at unshare(CLONE_NEWNS) (EPERM (1))");
    assert!(line.contains("unshare(CLONE_NEWNS)"), "{line}");
    assert!(line.contains("entered chroot instead"), "{line}");
    assert!(line.starts_with("pod-enter: "), "{line}");
}

/// Each setup step has a name for the fallback line, and no other byte
/// reads as one. The parent maps
/// through this function, so a step it cannot name cannot arrive
/// unnamed on the banner.
#[test]
fn the_setup_steps_name_themselves() {
    assert_eq!(ns_setup_step(7), Some("unshare(CLONE_NEWNS)"));
    assert_eq!(ns_setup_step(8), Some("remounting / recursively private"));
    assert_eq!(ns_setup_step(9), Some("mounting tmpfs on /tmp"));
    assert_eq!(ns_setup_step(10), Some("verifying the chroot landed on the opened rootfs"));
    assert_eq!(ns_setup_step(0), None);
    assert_eq!(ns_setup_step(1), None);
    assert_eq!(ns_setup_step(2), None);
    assert_eq!(ns_setup_step(11), None);
}

/// The guard compares the device and the inode.
/// Same device and file lands; either differing refuses.
#[test]
fn the_chroot_guard_compares_device_and_file() {
    let landed = sys::Stat { st_dev: 2096, st_ino: 39833, ..Default::default() };
    assert!(chroot_landed(&landed, &landed));
    let elsewhere = sys::Stat { st_dev: 2096, st_ino: 39834, ..Default::default() };
    assert!(!chroot_landed(&landed, &elsewhere));
    let other_fs = sys::Stat { st_dev: 146, st_ino: 39833, ..Default::default() };
    assert!(!chroot_landed(&landed, &other_fs));
}

/// A missing working directory is refused with the path and the image:
/// docker creates it, and the entry refuses it instead.
#[test]
fn a_missing_workdir_is_a_sentence_naming_path_and_image() {
    let dir = std::env::temp_dir().join(format!("pod-enter-workdir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    let root = dir.to_str().unwrap();
    let e = check_workdir(root, "/no/such/dir", "img").unwrap_err();
    let text = format!("{e}");
    assert!(text.contains("/no/such/dir"), "{text}");
    assert!(text.contains("img"), "{text}");
    assert!(text.contains("docker creates"), "{text}");
    assert!(text.contains("the entry refuses"), "{text}");
    assert_eq!(e.payload_code(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The control that passes. `/`, the empty default
/// and an image-held `/tmp` all pass, so the refusal above is about the
/// missing directory rather than about the flag.
#[test]
fn root_empty_and_tmp_workdirs_pass() {
    let dir = std::env::temp_dir().join(format!("pod-enter-workdir-ok-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    let root = dir.to_str().unwrap();
    assert!(check_workdir(root, "/", "img").is_ok());
    assert!(check_workdir(root, "", "img").is_ok());
    assert!(check_workdir(root, "/tmp", "img").is_ok());
    assert!(check_workdir(root, "/tmp/", "img").is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file where a directory belongs refuses like
/// a missing one, and a `..` escaping the image refuses as an escape
/// rather than resolving onto the host.
#[test]
fn a_file_and_an_escape_are_no_workdir() {
    let dir = std::env::temp_dir().join(format!("pod-enter-workdir-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    std::fs::write(dir.join("file"), b"x").unwrap();
    let root = dir.to_str().unwrap();
    let e = check_workdir(root, "/file", "img").unwrap_err();
    assert!(format!("{e}").contains("/file"), "{e}");
    let e = check_workdir(root, "/../outside", "img").unwrap_err();
    let text = format!("{e}");
    assert!(text.contains("escapes"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link to a directory inside the image resolves
/// the way the guest kernel resolves it, so it passes.
#[test]
#[cfg(unix)]
fn a_link_to_a_directory_passes() {
    let dir = std::env::temp_dir().join(format!("pod-enter-workdir-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("real")).unwrap();
    std::os::unix::fs::symlink("real", dir.join("link")).unwrap();
    let root = dir.to_str().unwrap();
    assert!(check_workdir(root, "/link", "img").is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A NUL byte is its own refusal rather than a
/// workdir sentence, because it never reaches the kernel.
#[test]
fn a_nul_workdir_is_its_own_refusal() {
    let dir = std::env::temp_dir().join(format!("pod-enter-workdir-nul-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    let root = dir.to_str().unwrap();
    let e = check_workdir(root, "/tmp\0x", "img").unwrap_err();
    assert!(format!("{e}").contains("NUL"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A missing working directory inside a volume names the volume
/// (`HOST:GUEST`), and one in the image names the image.
#[test]
fn a_missing_workdir_in_a_volume_names_the_volume() {
    use crate::userland::{Maps, Volume};
    let dir = std::env::temp_dir().join(format!("pod-enter-workdir-vol-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("rootfs/tmp")).unwrap();
    std::fs::create_dir_all(dir.join("vol/there")).unwrap();
    let host = dir.join("vol").to_str().unwrap().to_string();
    let volume = Volume { guest: "/data".to_string(), host: host.clone() };
    let maps = Maps::new(&Names::DEFAULT, dir.join("rootfs").to_str().unwrap(), &[volume]).unwrap();
    assert!(check_workdir(&maps, "/data/there", "img").is_ok());
    let text = format!("{}", check_workdir(&maps, "/data/gone", "img").unwrap_err());
    // The message cuts each side of the volume to 64 bytes: a long
    // temporary directory gives a cut host.
    let shown = if host.len() > 64 { format!("{}...", &host[..host.floor_char_boundary(64)]) } else { host.clone() };
    assert!(text.contains(&format!("in the volume {shown}:/data is not a directory in the volume")), "{text}");
    assert!(!text.contains("img"), "{text}");
    let text = format!("{}", check_workdir(&maps, "/data/../nowhere", "img").unwrap_err());
    assert!(text.contains("img") && text.contains("in the image"), "{text}");
    let text = format!("{}", check_workdir(&maps, "/nowhere", "img").unwrap_err());
    assert!(text.contains("for image \"img\" is not a directory in the image"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
