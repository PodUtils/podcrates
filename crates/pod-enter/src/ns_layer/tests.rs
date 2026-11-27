//! The checks of an overlay volume before the fork, its step bytes and the
//! refusals that name it (PodUtils/podvm#689). The mounts run in
//! `tests/locked/layer.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::{Fds, Names, Plan, RootDir};

fn plan(volumes: Vec<VolumeBind>) -> Plan {
    Plan {
        argv: vec!["/bin/true".to_string()],
        env: Vec::new(),
        working_dir: "/".to_string(),
        fds: Fds::default(),
        banner: String::new(),
        path_dirs: Vec::new(),
        names: Names::DEFAULT,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
        confine: None,
        hostname: None,
        volumes,
        userns_ids: None,
        pid_namespace: false,
        tmpfs: Vec::new(),
        relay_title: None,
        read_only_root: false,
    }
}

fn text(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

/// A fresh directory of this test with `lower/`, `upper/`, `work/`, the
/// file `file` and the image `image/`.
fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-enter-ns-layer-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    for sub in ["lower", "upper", "work", "image/tmp"] {
        fs::create_dir_all(d.join(sub)).unwrap();
    }
    fs::write(d.join("file"), "f").unwrap();
    d
}

fn overlay(d: &Path, host: &str, upper: &str, work: &str) -> VolumeBind {
    VolumeBind {
        host: text(&d.join(host)),
        guest: "/v".to_string(),
        overlay: Some(OverlayLayers { upper: upper.to_string(), work: work.to_string() }),
        ..VolumeBind::default()
    }
}

/// The checks before the fork: each refusal names the volume, before the
/// banner. A volume that passes holds the options of the mount, with
/// `userxattr` for a user namespace.
#[test]
fn an_overlay_volume_is_checked_before_the_fork() {
    let d = scratch("checks");
    let root = RootDir::open(&text(&d.join("image"))).unwrap();
    let (upper, work) = (text(&d.join("upper")), text(&d.join("work")));
    let refused = |v: VolumeBind| {
        let mut err = Vec::new();
        let e = crate::spawn_namespace(&root, &plan(vec![v]), &mut err).err().unwrap().to_string();
        assert!(err.is_empty(), "the refusal came after the banner: {}", String::from_utf8_lossy(&err));
        e
    };
    let label = format!("the volume {}:/v:overlay", text(&d.join("lower")));
    let e = refused(VolumeBind { read_only: true, ..overlay(&d, "lower", &upper, &work) });
    assert!(e.contains(&format!("{label} is read-only and an overlay")), "{e}");
    let e = refused(overlay(&d, "file", &upper, &work));
    assert!(e.contains("has a host path that is not a directory"), "{e}");
    let e = refused(overlay(&d, "lower", "upper", &work));
    assert!(e.contains(&format!("{label} has an upper directory \"upper\" that is not an absolute path")), "{e}");
    let e = refused(overlay(&d, "lower", &upper, &text(&d.join("missing"))));
    assert!(e.contains("has a work directory") && e.contains("cannot be read (ENOENT)"), "{e}");
    let e = refused(overlay(&d, "lower", &upper, &text(&d.join("file"))));
    assert!(e.contains("that is not a directory"), "{e}");
    // Layers that hold each other: the kernel takes an upper directory in
    // the host path with no `index`, and the writes would reach it.
    fs::create_dir_all(d.join("lower/in")).unwrap();
    let e = refused(overlay(&d, "lower", &text(&d.join("lower/in")), &work));
    assert!(e.contains("has layers that overlap: the upper directory is at or under the host path"), "{e}");
    let e = refused(overlay(&d, "lower", &upper, &upper));
    assert!(e.contains("the upper directory is at or under the work directory"), "{e}");
    fs::create_dir_all(d.join("upper/low")).unwrap();
    let e = refused(overlay(&d, "upper/low", &upper, &work));
    assert!(e.contains("the host path is at or under the upper directory"), "{e}");
    // Each path fits in a path of the kernel, and the options do not fit in
    // one page.
    let (long_upper, long_work) = (format!("{}{upper}", "/".repeat(2048)), format!("{}{work}", "/".repeat(2048)));
    let e = refused(overlay(&d, "lower", &long_upper, &long_work));
    assert!(e.contains("has overlay options of") && e.contains("mount(2) takes at most 4095"), "{e}");
    let layers = OverlayLayers { upper: upper.clone(), work: work.clone() };
    let ok = prepare(&overlay(&d, "lower", &upper, &work), &layers, true).unwrap();
    let c =
        |b: &CBuf| unsafe { std::ffi::CStr::from_ptr(b.ptr() as *const std::ffi::c_char) }.to_str().unwrap().to_owned();
    assert_eq!(c(ok.options(false)), format!("lowerdir=.,upperdir={upper},workdir={work}"));
    assert_eq!(c(ok.options(true)), format!("lowerdir=.,upperdir={upper},workdir={work}{USERXATTR}"));
    let _ = fs::remove_dir_all(&d);
}

/// A label says `:overlay`; a step of an overlay volume has its words in
/// `ns_setup_step`; `ELOOP` and `EBUSY` of the mount are the step of the
/// overlap, a refusal of class `Usage`.
#[test]
fn a_refused_overlay_volume_names_the_volume_and_the_step() {
    let v = VolumeBind { host: "/h".to_string(), guest: "/v".to_string(), ..VolumeBind::default() };
    assert_eq!(crate::volume_label(&v), "/h:/v");
    let v = VolumeBind { overlay: Some(OverlayLayers::default()), ..v };
    assert_eq!(crate::volume_label(&v), "/h:/v:overlay");
    for step in NS_LAYER_LOWER_STEP..=NS_LAYER_OVERLAP_STEP {
        assert_eq!(crate::ns_setup_step(step), step_name(step), "step {step}");
        assert!(step_name(step).is_some(), "step {step} has no words");
    }
    assert_eq!(mount_step(sys::ELOOP), NS_LAYER_OVERLAP_STEP);
    assert_eq!(mount_step(sys::EBUSY), NS_LAYER_OVERLAP_STEP);
    assert_eq!(mount_step(sys::EINVAL), NS_LAYER_MOUNT_STEP);
    let p = plan(vec![v]);
    let text = |step, e| crate::ns_volume::failure(&p, step, e, 0).map(|e| e.to_string()).unwrap_or_default();
    let e = text(NS_LAYER_OVERLAP_STEP, sys::ELOOP);
    assert!(e.contains("the volume /h:/v:overlay is refused") && e.contains("must not hold each other"), "{e}");
    assert!(e.contains("the payload did not run"), "{e}");
    let e = text(NS_LAYER_MOUNT_STEP, sys::EINVAL);
    assert!(e.contains("could not be mounted") && e.contains("user.* attributes"), "{e}");
    let e = text(NS_LAYER_LOWER_STEP, sys::ENOTDIR);
    assert!(e.contains("opening the lower layer of an overlay volume as a directory failed with ENOTDIR"), "{e}");
    let e = text(crate::NS_VOLUME_LINK_STEP, sys::ELOOP);
    assert!(e.contains("crosses a symbolic link"), "the steps of a bind stay: {e}");
    assert!(failure("x", crate::NS_VOLUME_BIND_STEP, sys::EPERM).is_none());
}
