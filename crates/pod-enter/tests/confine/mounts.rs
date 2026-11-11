//! A rule of the confinement on a path that the child mounts after the
//! compile (T-227): the namespace rung mounts its tmpfs on the `/tmp` of
//! the image in the child. Landlock does not read a rule on a directory
//! under a mount point, so the child opens the path of each rule after
//! its mounts and its root change.

use pod_confine::{Access, Spec};
use pod_enter::userland::Maps;
use pod_enter::{RootDir, spawn, spawn_namespace, spawn_userland};
use pod_hostprobe::sys as k;
use pod_testkit::host::skip_line;

use super::payload::{WRITER, run};
use super::{dir, image, landlock, plan, text};

/// The writes of each run: `/tmp` passes, and `/var` and `/` of the image
/// are outside the rules.
const WRITES: &str = "/tmp/a=0 /var/a=-13 /a=-13";

/// True where the rungs with a chroot can run here; else a skip line.
fn chroot_rungs() -> bool {
    if !landlock() {
        return false;
    }
    match pod_testkit::host::root() {
        Ok(()) => true,
        Err(why) => {
            skip_line(&format!("the rungs with a chroot need chroot(2): {why}"));
            false
        }
    }
}

/// The root descriptor above 3 and 4, the numbers of the pipes of `run`:
/// a passed descriptor onto the number of the root is refused.
fn root_over_the_pipes(path: &str) -> RootDir {
    let null = k::CBuf::new("/dev/null").unwrap();
    let mut held = Vec::new();
    while held.last().is_none_or(|&fd| fd < 4) {
        held.push(k::open(&null, k::O_RDONLY | k::O_CLOEXEC, 0).unwrap());
    }
    let root = RootDir::open(path).unwrap();
    for fd in held {
        let _ = k::close(fd);
    }
    root
}

/// The image of this binary with `/var`, and a spec that gives `access`
/// on `rule` and read and execute on the directories of the programs.
fn image_and_spec(tag: &str, rule: &str, access: Access) -> (std::path::PathBuf, Spec) {
    let d = dir(tag);
    let dirs = image::build(&d);
    std::fs::create_dir_all(d.join("var")).unwrap();
    let mut spec = Spec::new().allow(rule, access);
    for dir in dirs {
        spec = spec.allow(dir, Access::READ | Access::EXECUTE);
    }
    (d, spec)
}

/// A rule on `/tmp` covers the tmpfs of the namespace rung: a write into
/// `/tmp` passes and lands on the tmpfs, not in the image. A write outside
/// the rules fails with `EACCES`. The chroot sequence, with the `/tmp` of
/// the image, gives the same results.
pub fn a_rule_covers_the_tmpfs_of_the_namespace_rung() {
    if !chroot_rungs() {
        return;
    }
    let (d, spec) = image_and_spec("tmpfs", "/tmp", Access::READ | Access::WRITE);
    let argv: Vec<String> = [image::GUEST_EXE, WRITER, "/tmp/a", "/var/a", "/a"].map(String::from).to_vec();
    let root = root_over_the_pipes(&text(&d));
    let mut p = plan(argv, Some(spec));
    let mut banner = Vec::new();
    let r = run(&mut p, 0, |p| spawn_namespace(&root, p, &mut banner)).unwrap();
    if !banner.is_empty() {
        skip_line(&format!("the namespace rung fell back to chroot: {}", String::from_utf8_lossy(&banner).trim_end()));
        return;
    }
    assert_eq!(r.line.trim_end(), WRITES, "namespace: {r:?}");
    assert_eq!(r.code, 0, "namespace");
    assert!(!d.join("tmp/a").exists(), "the write of the namespace rung landed in the image, not on its tmpfs");
    let r = run(&mut p, 0, |p| spawn(&root, p, &mut std::io::sink())).unwrap();
    assert_eq!(r.line.trim_end(), WRITES, "chroot: {r:?}");
    assert!(d.join("tmp/a").exists(), "the chroot sequence writes the /tmp of the image");
    let _ = std::fs::remove_dir_all(d);
}

/// A path of the image under the tmpfs of the namespace rung exists for
/// the compile and not for the payload: the child refuses its rule by
/// name, and the payload does not run. The chroot sequence sees it.
pub fn a_rule_on_a_path_that_the_tmpfs_hides_is_refused() {
    if !chroot_rungs() {
        return;
    }
    let (d, spec) = image_and_spec("hidden", "/tmp/hidden", Access::READ);
    std::fs::create_dir_all(d.join("tmp/hidden")).unwrap();
    let argv: Vec<String> = [image::GUEST_EXE, WRITER, "/tmp/hidden/a"].map(String::from).to_vec();
    let root = root_over_the_pipes(&text(&d));
    let mut p = plan(argv, Some(spec));
    let mut banner = Vec::new();
    match run(&mut p, 0, |p| spawn_namespace(&root, p, &mut banner)) {
        Err(e) => assert!(e.contains("open(O_PATH) for the path \"/tmp/hidden\": ENOENT (2)"), "{e}"),
        Ok(_) if !banner.is_empty() => {
            let fell = String::from_utf8_lossy(&banner);
            skip_line(&format!("the namespace rung fell back to chroot: {}", fell.trim_end()));
            return;
        }
        Ok(r) => panic!("the payload ran with a rule on a path that its tmpfs hides: {r:?}"),
    }
    let r = run(&mut p, 0, |p| spawn(&root, p, &mut std::io::sink())).unwrap();
    assert_eq!(r.line.trim_end(), "/tmp/hidden/a=-13", "chroot, a read rule: {r:?}");
    let _ = std::fs::remove_dir_all(d);
}

/// The entry with no chroot opens the host paths of the rules in the
/// child too: a write rule on a directory holds, and a write beside it
/// fails with `EACCES`.
pub fn a_write_rule_holds_on_the_entry_with_no_chroot() {
    if !landlock() {
        return;
    }
    let d = dir("write");
    for sub in ["tmp", "var"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    let exe = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let mut spec = Spec::new().allow(text(&d.join("tmp")), Access::READ | Access::WRITE);
    for dir in image::dirs_of(&image::mapped_files()) {
        spec = spec.allow(dir, Access::READ | Access::EXECUTE);
    }
    let (tmp, var) = (text(&d.join("tmp/a")), text(&d.join("var/a")));
    let argv = vec![text(&exe), WRITER.to_string(), tmp.clone(), var.clone()];
    let mut p = plan(argv.clone(), Some(spec));
    let m = Maps::of_root(&text(&d));
    let root = root_over_the_pipes(m.rootfs());
    let r = run(&mut p, 0, |p| spawn_userland(&root, p, &m, argv, "userland", None, &mut std::io::sink())).unwrap();
    assert_eq!(r.line.trim_end(), format!("{tmp}=0 {var}=-13"), "{r:?}");
    let _ = std::fs::remove_dir_all(d);
}
