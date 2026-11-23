use std::fs;
use std::path::PathBuf;

use pod_core::Class;
use pod_hostprobe::sys::{self, CBuf};

use super::*;
use crate::{Fds, Names};

fn plan(volumes: Vec<VolumeBind>) -> Plan {
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
        relay_title: None,
    }
}

fn vol(host: &str, guest: &str, read_only: bool) -> VolumeBind {
    VolumeBind { host: host.to_string(), guest: guest.to_string(), read_only, host_base: None }
}

/// A fresh directory of this test.
fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-enter-ns-volume-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn class_of(e: &Error) -> Option<Class> {
    match e {
        Error::Runtime(f) => Some(f.class),
        _ => None,
    }
}

/// A guest path is absolute, not `/`, and normal; a trailing `/` goes.
#[test]
fn a_guest_path_is_an_absolute_normal_path_that_is_not_the_root() {
    assert_eq!(guest_parts("/data"), Ok(vec!["data"]));
    assert_eq!(guest_parts("/a/b/"), Ok(vec!["a", "b"]));
    for bad in ["data", "/", "//", "/a//b", "/a/./b", "/a/../b", ""] {
        assert!(guest_parts(bad).is_err(), "{bad:?}");
    }
}

/// The read-only remount gives each flag of the mount again; with no
/// `ST_VALID` it gives none.
#[test]
fn the_read_only_remount_keeps_each_flag_of_the_mount() {
    let base = sys::MS_REMOUNT | sys::MS_BIND | sys::MS_RDONLY;
    let all = sys::ST_VALID
        | sys::ST_NOSUID
        | sys::ST_NODEV
        | sys::ST_NOEXEC
        | sys::ST_NOATIME
        | sys::ST_NODIRATIME
        | sys::ST_NOSYMFOLLOW;
    let want = base
        | sys::MS_NOSUID
        | sys::MS_NODEV
        | sys::MS_NOEXEC
        | sys::MS_NOATIME
        | sys::MS_NODIRATIME
        | sys::MS_NOSYMFOLLOW;
    assert_eq!(remount_flags(all), want);
    assert_eq!(remount_flags(sys::ST_VALID | sys::ST_RELATIME), base | sys::MS_RELATIME);
    assert_eq!(remount_flags(sys::ST_NOSUID), base, "no ST_VALID: the flags tell nothing");
    assert_eq!(remount_flags(sys::ST_VALID), base);
}

/// The checks before the fork: each refusal names the volume.
#[test]
fn a_volume_is_checked_before_the_fork() {
    let d = scratch("prepare");
    let root = RootDir::open(d.to_str().unwrap()).unwrap();
    let host = d.to_str().unwrap().to_string();
    let missing = format!("{host}/missing");
    let refused = |v: Vec<VolumeBind>, without: Option<&str>| prepare(&plan(v), &root, without).err().unwrap();
    let e = refused(vec![vol(&host, "/data", false)], Some("the chroot rung"));
    assert!(e.to_string().contains("needs the namespace rung; the chroot rung"), "{e}");
    assert_eq!(class_of(&e), Some(Class::Unsupported));
    let e = refused(vec![vol(&missing, "/data", true)], None);
    assert!(e.to_string().contains(&format!("the host path of the volume {missing}:/data:ro does not exist")), "{e}");
    assert_eq!(class_of(&e), Some(Class::NotFound));
    let e = refused(vec![vol("relative", "/data", false)], None);
    assert!(e.to_string().contains("not an absolute path"), "{e}");
    let e = refused(vec![vol(&host, "/a/../b", false)], None);
    assert!(e.to_string().contains("not a normal path"), "{e}");
    let e = refused(vec![vol(&host, "/a", false), vol(&host, "/a/", false)], None);
    assert!(e.to_string().contains("repeats the guest path"), "{e}");
    let e = refused(vec![vol(&host, "/a", false); MOST + 1], None);
    assert!(e.to_string().contains(&format!("a plan takes at most {MOST}")), "{e}");
    fs::write(d.join("file"), "f").unwrap();
    let file = format!("{host}/file");
    let ok = prepare(&plan(vec![vol(&host, "/a/b", false), vol(&file, "/c", true)]), &root, None).unwrap();
    let kinds: Vec<(bool, bool, usize)> = ok.list.iter().map(|p| (p.dir, p.read_only, p.parts.len())).collect();
    assert_eq!(kinds, vec![(true, false, 2), (false, true, 1)]);
    assert!(prepare(&plan(Vec::new()), &root, Some("the chroot rung")).unwrap().list.is_empty());
    let _ = fs::remove_dir_all(&d);
}

/// The chroot rung refuses a plan with a volume before the banner: the
/// payload never runs with a volume missing.
#[test]
fn the_chroot_rung_refuses_a_volume_before_the_banner() {
    let d = scratch("chroot");
    let root = RootDir::open(d.to_str().unwrap()).unwrap();
    let mut err = Vec::new();
    let e = crate::spawn(&root, &plan(vec![vol("/", "/data", false)]), &mut err).err().unwrap();
    assert!(e.to_string().contains("the volume /:/data needs the namespace rung; the chroot rung"), "{e}");
    assert!(err.is_empty(), "{}", String::from_utf8_lossy(&err));
    let _ = fs::remove_dir_all(&d);
}

/// The parent names the volume and the step; a step of another kind is
/// not a volume.
#[test]
fn a_refused_volume_names_the_volume_and_the_step() {
    let p = plan(vec![vol("/srv/a", "/a", false), vol("/srv/b", "/b", true)]);
    let e = failure(&p, NS_VOLUME_LINK_STEP, sys::ELOOP, 1).unwrap();
    assert_eq!(class_of(&e), Some(Class::Policy));
    let text = e.to_string();
    assert!(
        text.contains("the volume /srv/b:/b:ro is refused: the guest path of a volume crosses a symbolic link"),
        "{text}"
    );
    assert!(text.contains("ELOOP") && text.contains("the payload did not run"), "{text}");
    let e = failure(&p, NS_VOLUME_BIND_STEP, sys::EPERM, 0).unwrap().to_string();
    assert!(
        e.contains("the volume /srv/a:/a could not be bound: binding the host path of a volume failed with EPERM"),
        "{e}"
    );
    let e = failure(&p, NS_VOLUME_REMOUNT_STEP, sys::EPERM, 1).unwrap().to_string();
    assert!(e.contains("remounting the bind of a read-only volume read-only failed"), "{e}");
    assert!(failure(&p, 7, sys::EPERM, 0).is_none());
    assert!(failure(&p, crate::NS_DEV_BIND_STEP, sys::EPERM, 0).is_none());
    for step in NS_VOLUME_POINT_STEP..=NS_VOLUME_REMOUNT_STEP {
        assert!(crate::ns_setup_step(step).is_some(), "step {step} has no words");
    }
}

/// The walk of the child, with no mount: each missing directory is made,
/// an empty file where the host side is a file, and a link of the image is
/// `ELOOP` at the step of a link, at the end of the path or inside it.
#[test]
fn the_walk_makes_the_mount_point_and_refuses_a_link() {
    let d = scratch("walk");
    fs::create_dir_all(d.join("etc")).unwrap();
    std::os::unix::fs::symlink("/etc", d.join("lnk")).unwrap();
    let root = sys::open(&CBuf::new(d.to_str().unwrap()).unwrap(), sys::O_PATH | sys::O_DIRECTORY, 0).unwrap();
    let prepared = |guest: &str, dir: bool| Prepared {
        host: sys::cempty(),
        parts: guest_parts(guest).unwrap().iter().map(|p| CBuf::new(p).unwrap()).collect(),
        dir,
        read_only: false,
        beneath: None,
    };
    let walk = |guest: &str, dir: bool| {
        point(root, &prepared(guest, dir)).map(|fd| {
            let _ = sys::close(fd);
        })
    };
    assert_eq!(walk("/new/deep/dir", true), Ok(()));
    assert!(d.join("new/deep/dir").is_dir());
    assert_eq!(walk("/etc/volume.conf", false), Ok(()));
    assert_eq!(fs::read(d.join("etc/volume.conf")).unwrap(), b"");
    assert_eq!(walk("/etc/volume.conf", false), Ok(()), "a second walk opens what the first made");
    assert_eq!(walk("/lnk/x", true), Err((NS_VOLUME_LINK_STEP, sys::ELOOP)));
    assert_eq!(walk("/lnk", true), Err((NS_VOLUME_LINK_STEP, sys::ELOOP)));
    assert!(!d.join("etc/x").exists(), "the walk made nothing through the link");
    assert_eq!(walk("/etc/volume.conf", true), Err((NS_VOLUME_POINT_STEP, sys::ENOTDIR)));
    let _ = sys::close(root);
    let _ = fs::remove_dir_all(&d);
}

/// The working directory in a volume resolves in its host side, and a
/// missing one names the volume.
#[test]
fn a_working_directory_in_a_volume_resolves_in_the_volume() {
    let d = scratch("workdir");
    let (image, host) = (d.join("image"), d.join("host"));
    fs::create_dir_all(&image).unwrap();
    fs::create_dir_all(host.join("sub")).unwrap();
    let root = RootDir::open(image.to_str().unwrap()).unwrap();
    let mut p = plan(vec![vol(host.to_str().unwrap(), "/work/v", false)]);
    p.working_dir = "/work/v/sub".to_string();
    assert!(check_workdir(&root, &p, None).is_ok());
    p.working_dir = "/work/v/none".to_string();
    let e = check_workdir(&root, &p, None).err().unwrap().to_string();
    // A long host path is shortened in the text (a long TMPDIR): the test
    // reads the volume by its guest side.
    assert!(e.contains("in the volume ") && e.contains(":/work/v is not a directory in the volume"), "{e}");
    p.volumes.clear();
    p.working_dir = "/work/v/sub".to_string();
    let e = check_workdir(&root, &p, None).err().unwrap().to_string();
    assert!(e.contains("is not a directory in the image"), "{e}");
    let _ = fs::remove_dir_all(&d);
}

/// A volume with `host_base` opens its host path under the base with no
/// link: a link anywhere under the base is a refusal that names the
/// volume, and a path out of the base is a usage error
/// (PodUtils/podrunner#277).
#[test]
fn a_host_path_under_its_base_holds_no_link() {
    let d = scratch("beneath");
    fs::create_dir_all(d.join("base/job/data")).unwrap();
    fs::create_dir_all(d.join("img")).unwrap();
    std::os::unix::fs::symlink("/etc", d.join("base/job/out")).unwrap();
    let root = RootDir::open(d.join("img").to_str().unwrap()).unwrap();
    let text = |p: std::path::PathBuf| p.to_str().unwrap().to_string();
    let with = |host: std::path::PathBuf| {
        let mut v = vol(&text(host), "/v", false);
        v.host_base = Some(text(d.join("base")));
        prepare(&plan(vec![v]), &root, None)
    };
    assert!(with(d.join("base/job/data")).is_ok());
    assert!(with(d.join("base")).is_ok(), "the base itself");
    let linked = with(d.join("base/job/out")).err().expect("a link under the base");
    assert_eq!(class_of(&linked), Some(Class::Policy), "{linked}");
    assert!(linked.to_string().contains("holds a symbolic link under its base"), "{linked}");
    let through = with(d.join("base/job/out/hostname")).err().expect("a path through a link");
    assert_eq!(class_of(&through), Some(Class::Policy), "{through}");
    let out = with(d.join("img")).err().expect("a path out of the base");
    assert!(out.to_string().contains("not under its host base"), "{out}");
    let above = with(d.join("base/../img")).err().expect("a path above the base");
    assert_eq!(class_of(&above), Some(Class::Policy), "{above}");
    assert!(above.to_string().contains("goes above its base by `..`"), "{above}");
    let _ = fs::remove_dir_all(&d);
}
