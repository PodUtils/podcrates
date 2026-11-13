//! A rule of the confinement on a path that the child mounts after the
//! compile (T-227): the namespace rung mounts its tmpfs on the `/tmp` of
//! the image in the child. Landlock does not read a rule on a directory
//! under a mount point, so the child opens the path of each rule after
//! its mounts and its root change.

use pod_confine::{Access, Spec};
use pod_enter::userland::Maps;
use pod_enter::{RootDir, spawn, spawn_joined, spawn_namespace, spawn_userland};
use pod_hostprobe::sys as k;
use pod_testkit::host::skip_line;

use super::payload::{READER, WRITER, run};
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

/// The namespace rung mounts a fresh proc on the `/proc` of the image,
/// with no set-user-ID, no device and no exec; the mount stays in the
/// namespace of the run. The chroot sequence has no `/proc`.
pub fn a_fresh_proc_is_on_the_proc_of_the_namespace_rung() {
    if let Err(why) = pod_testkit::host::root() {
        skip_line(&format!("the rungs with a chroot need chroot(2): {why}"));
        return;
    }
    let d = dir("proc");
    image::build(&d);
    std::fs::create_dir_all(d.join("proc")).unwrap();
    let argv: Vec<String> = [image::GUEST_EXE, READER, "/proc/self/status"].map(String::from).to_vec();
    let root = root_over_the_pipes(&text(&d));
    let mut p = plan(argv, None);
    let mut banner = Vec::new();
    let r = run(&mut p, 0, |p| spawn_namespace(&root, p, &mut banner)).unwrap();
    if !banner.is_empty() {
        skip_line(&format!("the namespace rung fell back to chroot: {}", String::from_utf8_lossy(&banner).trim_end()));
        return;
    }
    assert_eq!(r.line.trim_end(), "/proc/self/status=0", "namespace: {r:?}");
    let options = r.options("/proc").unwrap_or_default();
    for flag in ["nosuid", "nodev", "noexec"] {
        assert!(options.split(',').any(|o| o == flag), "the proc of the rung is not {flag}: {r:?}");
    }
    assert!(!d.join("proc/self").exists(), "the proc mount reached the host");
    let r = run(&mut p, 0, |p| spawn(&root, p, &mut std::io::sink())).unwrap();
    assert_eq!(r.line.trim_end(), "/proc/self/status=-2", "chroot: {r:?}");
    let _ = std::fs::remove_dir_all(d);
}

/// An entry that joins the mount namespace of a running payload sees its
/// tmpfs: a file that the payload of the run writes in `/tmp` opens for
/// the payload of the join. With the pidfd of a process that ended, the
/// entry names the refused step and runs in a namespace of its own.
pub fn a_joined_entry_sees_the_tmpfs_of_the_running_payload() {
    if let Err(why) = pod_testkit::host::root() {
        skip_line(&format!("the rungs with a chroot need chroot(2): {why}"));
        return;
    }
    let d = dir("join");
    image::build(&d);
    let root = root_over_the_pipes(&text(&d));
    let pipe = || {
        let mut p = [0i32; 2];
        k::pipe2(&mut p, k::O_CLOEXEC).unwrap();
        (i64::from(p[0]), i64::from(p[1]))
    };
    let (wait_r, wait_w) = pipe();
    let (line_r, line_w) = pipe();
    let mut first = plan([image::GUEST_EXE, WRITER, "/tmp/shared"].map(String::from).to_vec(), None);
    first.fds.pass = vec![(3, wait_r), (4, line_w)];
    let mut banner = Vec::new();
    let held = spawn_namespace(&root, &first, &mut banner).unwrap();
    let _ = (k::close(wait_r), k::close(line_w));
    let mut line = Vec::new();
    let mut buf = [0u8; 64];
    while let Ok(n @ 1..) = k::read(line_r, &mut buf) {
        line.extend_from_slice(&buf[..n as usize]);
    }
    let pidfd = k::pidfd_open(held.pid).unwrap();
    let mut p = plan([image::GUEST_EXE, READER, "/tmp/shared"].map(String::from).to_vec(), None);
    let mut joined = Vec::new();
    let r = run(&mut p, 0, |p| spawn_joined(&root, p, pidfd, &mut joined));
    let _ = (k::close(wait_w), k::close(line_r));
    assert_eq!(held.wait().unwrap(), 0);
    if !banner.is_empty() {
        skip_line(&format!("the namespace rung fell back to chroot: {}", String::from_utf8_lossy(&banner).trim_end()));
        return;
    }
    assert_eq!(String::from_utf8_lossy(&line).trim_end(), "/tmp/shared=0");
    let r = r.unwrap();
    assert!(joined.is_empty(), "the join fell back: {}", String::from_utf8_lossy(&joined));
    assert_eq!(r.line.trim_end(), "/tmp/shared=0", "joined: {r:?}");
    let mut fell = Vec::new();
    let r = run(&mut p, 0, |p| spawn_joined(&root, p, pidfd, &mut fell)).unwrap();
    let fell = String::from_utf8_lossy(&fell);
    assert!(fell.contains("setns(pidfd, CLONE_NEWNS) (ESRCH (3))"), "{fell}");
    assert_eq!(r.line.trim_end(), "/tmp/shared=-2", "a namespace of its own: {r:?}");
    let _ = k::close(pidfd);
    let _ = std::fs::remove_dir_all(d);
}

/// The namespace rung binds the host's `/dev/pts` on the `/dev/pts` of the
/// image, so `ttyname` of a pty finds its path; the bind stays in the
/// namespace of the run. The chroot sequence binds nothing.
pub fn the_pts_of_the_host_are_bound_on_the_namespace_rung() {
    if let Err(why) = pod_testkit::host::root() {
        skip_line(&format!("the rungs with a chroot need chroot(2): {why}"));
        return;
    }
    if !std::path::Path::new("/dev/pts/ptmx").exists() {
        skip_line("the host has no devpts on /dev/pts");
        return;
    }
    let d = dir("pts");
    image::build(&d);
    std::fs::create_dir_all(d.join("dev/pts")).unwrap();
    let argv: Vec<String> = [image::GUEST_EXE, READER, "/dev/pts/ptmx"].map(String::from).to_vec();
    let root = root_over_the_pipes(&text(&d));
    let mut p = plan(argv, None);
    let mut banner = Vec::new();
    let r = run(&mut p, 0, |p| spawn_namespace(&root, p, &mut banner)).unwrap();
    if !banner.is_empty() {
        skip_line(&format!("the namespace rung fell back to chroot: {}", String::from_utf8_lossy(&banner).trim_end()));
        return;
    }
    assert!(r.options("/dev/pts").is_some(), "no mount on /dev/pts: {r:?}");
    assert_eq!(r.line.trim_end(), "/dev/pts/ptmx=0", "namespace: {r:?}");
    assert!(!d.join("dev/pts/ptmx").exists(), "the bind reached the host");
    let r = run(&mut p, 0, |p| spawn(&root, p, &mut std::io::sink())).unwrap();
    assert_eq!(r.line.trim_end(), "/dev/pts/ptmx=-2", "chroot: {r:?}");
    let _ = std::fs::remove_dir_all(d);
}
