//! The overlay root of the namespace rung (T-486): the image is the
//! read-only lower layer, and each write of the payload goes to a tmpfs
//! of the run. These need root, as the other tests of the rung.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use pod_enter::{Error, Root, RootDir, Upper, spawn_joined_root, spawn_namespace_root};
use pod_hostprobe::sys::{self as k, CBuf};
use pod_testkit::host::skip_line;

use super::mounts::root_over_the_pipes;
use super::payload::{CHANGER, READER, WRITER, run};
use super::{dir, image, plan, text};

/// The upper layer of each test: a tmpfs of 64 MiB.
const UPPER: Upper<'static> = Upper::Tmpfs { size: 64 << 20 };

/// A refusal of the host (no mount namespace, overlayfs refused): a skip.
/// Each other refusal, such as a failed landing check, fails the test.
fn host_refuses(e: &str) -> bool {
    ["at unshare(CLONE_NEWNS)", "at mounting overlay on the root"].iter().any(|s| e.contains(s))
}

/// True where the namespace rung can run here; else a skip line.
fn rung() -> bool {
    match pod_testkit::host::root() {
        Ok(()) => true,
        Err(why) => {
            skip_line(&format!("the namespace rung needs a mount namespace: {why}"));
            false
        }
    }
}

/// One line for each path under `root`: its mode, its owner, its size,
/// its link target and the bytes of a file. Two equal maps are two equal
/// trees.
fn digest(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            let m = std::fs::symlink_metadata(&p).unwrap();
            let what = if m.file_type().is_symlink() {
                format!("link {:?}", std::fs::read_link(&p).unwrap())
            } else if m.is_dir() {
                stack.push(p.clone());
                "dir".to_string()
            } else {
                format!("file {:?}", std::fs::read(&p).unwrap())
            };
            let rel = text(p.strip_prefix(root).unwrap());
            out.insert(rel, format!("{:o} {}:{} {} {what}", m.mode(), m.uid(), m.gid(), m.size()));
        }
    }
    out
}

/// A payload that writes a new file, changes a file of the image, removes
/// a file of the image and makes a directory, all under `/`, sees each
/// change; the lower directory keeps its digest; a second run sees the
/// image as it was. The path of the image holds `,` and `:`, the
/// separators of the options of overlayfs.
pub fn an_overlay_root_leaves_the_lower_directory_unchanged() {
    if !rung() {
        return;
    }
    let d = dir("overlay,a:b");
    image::build(&d);
    std::fs::write(d.join("change"), b"before").unwrap();
    std::fs::write(d.join("remove"), b"remove").unwrap();
    let before = digest(&d);
    let lower = root_over_the_pipes(&text(&d));
    let root = Root::Overlay { lower: &lower, upper: UPPER };
    let changes = ["+/new", "+/change", "-/remove", "d/dir", "+/dir/a", "+/tmp/t"];
    let argv: Vec<String> = [image::GUEST_EXE, CHANGER].into_iter().chain(changes).map(String::from).collect();
    let mut p = plan(argv, None);
    let r = match run(&mut p, 0, |p| spawn_namespace_root(root, p, &mut std::io::sink())) {
        Err(e) if host_refuses(&e) => {
            skip_line(&format!("this host refuses the overlay root: {e}"));
            return;
        }
        r => r.unwrap(),
    };
    let all: Vec<String> = changes.iter().map(|c| format!("{c}=0")).collect();
    assert_eq!(r.line.trim_end(), all.join(" "), "{r:?}");
    assert_eq!(r.code, 0);
    assert_eq!(digest(&d), before, "the payload changed the lower directory");
    let argv = [image::GUEST_EXE, READER, "/new", "/remove", "/change", "/dir"].map(String::from).to_vec();
    let mut p = plan(argv, None);
    let r = run(&mut p, 0, |p| spawn_namespace_root(root, p, &mut std::io::sink())).unwrap();
    assert_eq!(r.line.trim_end(), "/new=-2 /remove=0 /change=0 /dir=-2", "the writes outlived the run: {r:?}");
    let _ = std::fs::remove_dir_all(d);
}

/// Where overlayfs refuses the mount, the entry refuses with
/// [`Error::Overlay`] and the payload does not run: no fallback to chroot
/// on the lower directory. A child of this process (one thread) stacks
/// overlays in a mount namespace of its own until the kernel refuses one
/// (its limit of stacked file systems), and gives the last one as the
/// lower directory.
pub fn a_refused_overlay_mount_is_a_typed_error() {
    if !rung() {
        return;
    }
    let d = dir("refused");
    image::build(&d.join("l0"));
    // SAFETY: this process has one thread, so the child may allocate.
    let pid = match unsafe { k::clone_fork(k::SIGCHLD) }.unwrap() {
        0 => k::exit_group(refused_in_child(&d)),
        pid => pid,
    };
    let mut status = 0;
    k::wait4(pid, &mut status).unwrap();
    match status >> 8 {
        3 => skip_line("the child has no mount namespace of its own"),
        4 => skip_line("the kernel stacked four overlays and refused none"),
        code => assert_eq!(code, 0, "the child: 1 another refusal, 2 the payload ran on the lower directory"),
    }
    let _ = std::fs::remove_dir_all(d);
}

fn refused_in_child(d: &Path) -> i32 {
    let c = |s: &str| CBuf::new(s).unwrap();
    let (empty, slash, overlay, tmpfs) = (k::cempty(), c("/"), c("overlay"), c("tmpfs"));
    if k::unshare(k::CLONE_NEWNS).is_err() || k::mount(&empty, &slash, &empty, k::MS_REC | k::MS_PRIVATE).is_err() {
        return 3;
    }
    let mut lower = text(&d.join("l0"));
    for level in 1..=5 {
        if level == 5 {
            return 4;
        }
        let (t, m) = (text(&d.join(format!("t{level}"))), text(&d.join(format!("m{level}"))));
        for p in [&t, &m] {
            std::fs::create_dir_all(p).unwrap();
        }
        if k::mount(&tmpfs, &c(&t), &tmpfs, 0).is_err() {
            return 3;
        }
        for sub in ["u", "w"] {
            std::fs::create_dir_all(format!("{t}/{sub}")).unwrap();
        }
        let options = c(&format!("lowerdir={lower},upperdir={t}/u,workdir={t}/w"));
        if k::mount_data(&overlay, &c(&m), &overlay, 0, &options).is_err() {
            break;
        }
        lower = m;
    }
    let root = RootDir::open(&lower).unwrap();
    let p = plan([image::GUEST_EXE, WRITER, "/ran"].map(String::from).to_vec(), None);
    match spawn_namespace_root(Root::Overlay { lower: &root, upper: UPPER }, &p, &mut std::io::sink()) {
        Err(e @ Error::Overlay { .. }) if e.to_string().contains("mounting overlay on the root: EINVAL") => 0,
        Err(e) => {
            eprintln!("another refusal: {e}");
            1
        }
        Ok(child) => {
            let _ = child.wait();
            2
        }
    }
}

/// An entry that joins the mount namespace of a payload on an overlay
/// root sees the writes of that payload under `/`, and the lower directory
/// does not. With the pidfd of a process that ended, the entry names the
/// refused step and enters an overlay root of its own.
pub fn a_joined_entry_sees_the_overlay_of_the_running_payload() {
    if !rung() {
        return;
    }
    let d = dir("overlay-join");
    image::build(&d);
    let lower = root_over_the_pipes(&text(&d));
    let root = Root::Overlay { lower: &lower, upper: UPPER };
    let pipe = || {
        let mut p = [0i32; 2];
        k::pipe2(&mut p, k::O_CLOEXEC).unwrap();
        (i64::from(p[0]), i64::from(p[1]))
    };
    let (wait_r, wait_w) = pipe();
    let (line_r, line_w) = pipe();
    let mut first = plan([image::GUEST_EXE, WRITER, "/shared"].map(String::from).to_vec(), None);
    first.fds.pass = vec![(3, wait_r), (4, line_w)];
    let held = match spawn_namespace_root(root, &first, &mut std::io::sink()) {
        Err(e) if host_refuses(&e.to_string()) => {
            skip_line(&format!("this host refuses the overlay root: {e}"));
            return;
        }
        r => r.unwrap(),
    };
    let _ = (k::close(wait_r), k::close(line_w));
    let mut line = Vec::new();
    let mut buf = [0u8; 64];
    while let Ok(n @ 1..) = k::read(line_r, &mut buf) {
        line.extend_from_slice(&buf[..n as usize]);
    }
    let pidfd = k::pidfd_open(held.pid).unwrap();
    let mut p = plan([image::GUEST_EXE, READER, "/shared"].map(String::from).to_vec(), None);
    let mut joined = Vec::new();
    let r = run(&mut p, 0, |p| spawn_joined_root(root, p, pidfd, &mut joined));
    let _ = (k::close(wait_w), k::close(line_r));
    assert_eq!(held.wait().unwrap(), 0);
    assert_eq!(String::from_utf8_lossy(&line).trim_end(), "/shared=0");
    let r = r.unwrap();
    assert!(joined.is_empty(), "the join fell back: {}", String::from_utf8_lossy(&joined));
    assert_eq!(r.line.trim_end(), "/shared=0", "joined: {r:?}");
    assert!(!d.join("shared").exists(), "the write of the run reached the lower directory");
    let mut fell = Vec::new();
    let r = run(&mut p, 0, |p| spawn_joined_root(root, p, pidfd, &mut fell)).unwrap();
    let fell = String::from_utf8_lossy(&fell);
    assert!(fell.contains("setns(pidfd, CLONE_NEWNS) (ESRCH (3))"), "{fell}");
    assert_eq!(r.line.trim_end(), "/shared=-2", "an overlay root of its own: {r:?}");
    let _ = k::close(pidfd);
    let _ = std::fs::remove_dir_all(d);
}
