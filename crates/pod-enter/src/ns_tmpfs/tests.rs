//! The checks of a tmpfs of a plan before the fork, the check of its limit,
//! and the refusals that name it (PodUtils/podvm#667). The mounts and the
//! writes run in `tests/userns/tmpfs.rs`.

use std::fs;
use std::num::NonZeroU64;
use std::path::PathBuf;

use pod_core::Class;

use super::*;
use crate::{Fds, Names, RootDir, VolumeBind};

fn plan(tmpfs: Vec<TmpfsMount>, volumes: Vec<VolumeBind>) -> Plan {
    Plan {
        argv: vec!["/bin/true".to_string()],
        env: Vec::new(),
        working_dir: "/".to_string(),
        fds: Fds::default(),
        banner: "the banner\n".to_string(),
        path_dirs: vec!["/bin".to_string()],
        names: Names::DEFAULT,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
        confine: None,
        hostname: None,
        volumes,
        userns_ids: None,
        pid_namespace: false,
        tmpfs,
        relay_title: None,
        read_only_root: false,
    }
}

fn tmpfs(guest: &str, bytes: u64) -> TmpfsMount {
    TmpfsMount { guest: guest.to_string(), bytes: NonZeroU64::new(bytes).unwrap() }
}

fn vol(guest: &str) -> VolumeBind {
    VolumeBind { host: "/".to_string(), guest: guest.to_string(), read_only: false, host_base: None }
}

/// A fresh directory of this test.
fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-enter-ns-tmpfs-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// The bytes of a C string of the child, with no NUL.
fn bytes(c: &CBuf) -> Vec<u8> {
    // SAFETY: a `CBuf` holds one NUL-ended string while it lives.
    unsafe { std::ffi::CStr::from_ptr(c.ptr() as *const std::ffi::c_char) }.to_bytes().to_vec()
}

fn class_of(e: &Error) -> Option<Class> {
    match e {
        Error::Runtime(f) => Some(f.class),
        _ => None,
    }
}

/// The checks before the fork: each refusal names the tmpfs.
#[test]
fn a_tmpfs_is_checked_before_the_fork() {
    let refused = |t: Vec<TmpfsMount>, v: Vec<VolumeBind>| prepare(&plan(t, v)).err().unwrap().to_string();
    let e = refused(vec![tmpfs("work", 1)], Vec::new());
    assert!(e.contains("the tmpfs work:size=1 has a guest path that is not an absolute path"), "{e}");
    let e = refused(vec![tmpfs("/", 1)], Vec::new());
    assert!(e.contains("a mount cannot replace the root"), "{e}");
    let e = refused(vec![tmpfs("/a/../b", 1)], Vec::new());
    assert!(e.contains("not a normal path"), "{e}");
    let e = refused(vec![tmpfs("/w", 1), tmpfs("/w/", 2)], Vec::new());
    assert!(e.contains("the tmpfs /w/:size=2 repeats the guest path of another tmpfs"), "{e}");
    let e = refused(vec![tmpfs("/data", 1)], vec![vol("/data")]);
    assert!(e.contains("is at or under the guest path of the volume /:/data"), "{e}");
    let e = refused(vec![tmpfs("/data/in", 1)], vec![vol("/data")]);
    assert!(e.contains("is at or under the guest path of the volume /:/data"), "{e}");
    let e = refused(vec![tmpfs("/w", 1); MOST + 1], Vec::new());
    assert!(e.contains(&format!("a plan takes at most {MOST}")), "{e}");
    let ok = prepare(&plan(vec![tmpfs("/w", 1 << 20), tmpfs("/tmp", 4096)], vec![vol("/w/src")])).unwrap();
    let kinds: Vec<(usize, Vec<u8>, u64)> = ok.list.iter().map(|p| (p.parts.len(), bytes(&p.data), p.bytes)).collect();
    assert_eq!(kinds, vec![(1, b"size=1048576".to_vec(), 1 << 20), (1, b"size=4096".to_vec(), 4096)]);
    assert!(prepare(&plan(Vec::new(), Vec::new())).unwrap().is_empty());
}

/// The limit holds where the tmpfs has the limit in whole blocks, and on
/// no other file system. A size near `u64::MAX` wraps to a tmpfs with no
/// limit in the kernel (no blocks): the check refuses it.
#[test]
fn the_limit_holds_only_on_a_tmpfs_of_that_size() {
    let fs = |f_type: u64, f_bsize: i64, f_blocks: u64| sys::Statfs { f_type, f_bsize, f_blocks, ..Default::default() };
    assert!(tmpfs_enforced(&fs(TMPFS_MAGIC, 4096, 256), 1 << 20));
    assert!(tmpfs_enforced(&fs(TMPFS_MAGIC, 4096, 1), 1), "the kernel rounds up to a page");
    assert!(tmpfs_enforced(&fs(TMPFS_MAGIC, 65536, 16), 1 << 20), "a page of 64 KiB");
    assert!(!tmpfs_enforced(&fs(TMPFS_MAGIC, 4096, 0), u64::MAX), "no limit");
    assert!(!tmpfs_enforced(&fs(TMPFS_MAGIC, 4096, 4_000_000), 1 << 20), "the default limit");
    assert!(!tmpfs_enforced(&fs(sys::OVERLAYFS_SUPER_MAGIC, 4096, 256), 1 << 20), "not a tmpfs");
    assert!(!tmpfs_enforced(&fs(TMPFS_MAGIC, 0, 256), 1 << 20), "no block size");
    assert!(!tmpfs_enforced(&fs(TMPFS_MAGIC, -1, 256), 1 << 20), "a negative block size");
}

/// The parent names the tmpfs and the step; a step of another kind is not
/// a tmpfs, and each step has its words.
#[test]
fn a_refused_tmpfs_names_the_tmpfs_and_the_step() {
    let p = plan(vec![tmpfs("/a", 4096), tmpfs("/b", 1 << 20)], Vec::new());
    let e = failure(&p, NS_TMPFS_LIMIT_STEP, Errno(0), 1).unwrap();
    assert_eq!(class_of(&e), Some(Class::Unsupported));
    let text = e.to_string();
    assert!(text.contains("the tmpfs /b:size=1048576 is refused: the kernel did not enforce its byte limit"), "{text}");
    assert!(text.contains("the payload did not run"), "{text}");
    let e = failure(&p, NS_TMPFS_MOUNT_STEP, sys::EPERM, 0).unwrap();
    assert_eq!(class_of(&e), Some(Class::Refused));
    let text = e.to_string();
    assert!(
        text.contains(
            "the tmpfs /a:size=4096 could not be mounted: mounting a tmpfs with a byte limit failed with EPERM"
        ),
        "{text}"
    );
    let e = failure(&p, NS_TMPFS_LINK_STEP, sys::ELOOP, 0).unwrap();
    assert_eq!(class_of(&e), Some(Class::Policy));
    assert!(e.to_string().contains("crosses a symbolic link of the image (ELOOP"), "{e}");
    let e = failure(&p, NS_TMPFS_LIMIT_STEP, sys::ENOENT, 0).unwrap().to_string();
    assert!(e.contains("reading back the byte limit of a tmpfs failed with ENOENT"), "{e}");
    assert!(failure(&p, crate::NS_VOLUME_BIND_STEP, sys::EPERM, 0).is_none());
    assert!(crate::ns_volume::failure(&p, NS_TMPFS_MOUNT_STEP, sys::EPERM, 0).is_some(), "the parent reads it");
    for step in NS_TMPFS_POINT_STEP..=NS_TMPFS_LIMIT_STEP {
        assert_eq!(crate::ns_setup_step(step), step_name(step), "step {step}");
        assert!(step_name(step).is_some(), "step {step} has no words");
    }
    assert_eq!(walk_step(NS_VOLUME_LINK_STEP), NS_TMPFS_LINK_STEP);
    assert_eq!(walk_step(crate::NS_VOLUME_POINT_STEP), NS_TMPFS_POINT_STEP);
}

/// A working directory at or under a tmpfs, and in no volume, needs no
/// directory of the image: the child makes it in the tmpfs.
#[test]
fn a_working_directory_in_a_tmpfs_is_made_by_the_child() {
    let d = scratch("workdir");
    let root = RootDir::open(d.to_str().unwrap()).unwrap();
    let mut p = plan(vec![tmpfs("/work", 1 << 20)], vec![vol("/work/src")]);
    for (guest, held) in [("/work", true), ("/work/a/b", true), ("/work/src", false), ("/work/src/x", false)] {
        assert_eq!(holds(&p, guest), held, "{guest}");
    }
    assert!(!holds(&p, "/workshop") && !holds(&p, "/") && !holds(&p, "work") && !holds(&p, "/work/../etc"));
    p.working_dir = "/work/a/b".to_string();
    assert!(crate::ns_volume::check_workdir(&root, &p, None).is_ok());
    let parts: Vec<Vec<u8>> = crate::ns_volume::tmp_workdir_parts(&p).iter().map(bytes).collect();
    assert_eq!(parts, vec![b"work".to_vec(), b"a".to_vec(), b"b".to_vec()]);
    p.working_dir = "/elsewhere".to_string();
    assert!(crate::ns_volume::check_workdir(&root, &p, None).is_err(), "not in the image, not in the tmpfs");
    assert!(crate::ns_volume::tmp_workdir_parts(&p).is_empty());
    let _ = fs::remove_dir_all(&d);
}

/// An entry with no mount namespace refuses a tmpfs before the banner: the
/// chroot rung, and the namespace rung where it cannot mount (an image
/// with no `/tmp` makes it fall back to chroot). The payload never runs
/// with no limit.
#[test]
fn an_entry_that_cannot_mount_refuses_a_tmpfs_by_name() {
    let d = scratch("chroot");
    let root = RootDir::open(d.to_str().unwrap()).unwrap();
    let p = plan(vec![tmpfs("/work", 1 << 20)], Vec::new());
    let mut err = Vec::new();
    let e = crate::spawn(&root, &p, &mut err).err().unwrap();
    assert_eq!(class_of(&e), Some(Class::Unsupported));
    let text = e.to_string();
    assert!(text.contains("the tmpfs /work:size=1048576 needs the namespace rung; the chroot rung"), "{text}");
    assert!(err.is_empty(), "{}", String::from_utf8_lossy(&err));
    let e = crate::spawn_namespace(&root, &p, &mut err).err().unwrap();
    assert_eq!(class_of(&e), Some(Class::Unsupported));
    let text = e.to_string();
    assert!(
        text.contains("the tmpfs /work:size=1048576 needs the namespace rung, and the image holds no /tmp"),
        "{text}"
    );
    assert!(err.is_empty(), "{}", String::from_utf8_lossy(&err));
    let _ = fs::remove_dir_all(&d);
}
