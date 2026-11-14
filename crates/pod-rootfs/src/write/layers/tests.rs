use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use super::*;
use crate::testing::Scratch;
use crate::write::{Kind, Reach, Wrote};

/// A scratch directory with an empty `lower` and `upper`.
fn two(name: &str) -> (Scratch, PathBuf, PathBuf) {
    let d = Scratch::new(name);
    let (lower, upper) = (d.at("lower"), d.at("upper"));
    std::fs::create_dir_all(&lower).unwrap();
    std::fs::create_dir_all(&upper).unwrap();
    (d, lower, upper)
}

fn put(root: &Path, rel: &str, body: &str, mode: u32) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn link(root: &Path, rel: &str, target: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    symlink(target, p).unwrap();
}

fn mode_of(p: &Path) -> u32 {
    std::fs::symlink_metadata(p).unwrap().mode() & 0o7777
}

/// One line for each path under `root`: the type, the mode, the owner, the
/// size, the device number, the link target, the bytes of a file, and with
/// `times` the times of the change and of the inode. Two equal maps are
/// two equal trees.
pub(super) fn digest(root: &Path, times: bool) -> BTreeMap<String, String> {
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
            } else if m.is_file() {
                format!("file {:?}", String::from_utf8_lossy(&std::fs::read(&p).unwrap()))
            } else {
                format!("node {:o} {}", m.mode() & 0o170000, m.rdev())
            };
            let size = if m.is_dir() { 0 } else { m.size() };
            let mut line = format!("{:o} {}:{} {size} {what}", m.mode() & 0o7777, m.uid(), m.gid());
            if times {
                line.push_str(&format!(" {}.{} {}.{}", m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec()));
            }
            out.insert(p.strip_prefix(root).unwrap().display().to_string(), line);
        }
    }
    out
}

/// Can this process make a whiteout? Linux 5.8 permits it with no
/// privilege on most file systems.
fn whiteout_at(p: &Path) -> bool {
    let c = sys::CBuf::new(p.to_str().unwrap()).unwrap();
    match sys::mknodat(sys::AT_FDCWD as i64, &c, sys::S_IFCHR | 0o600, 0) {
        Ok(_) => true,
        Err(e) => {
            pod_testkit::host::skip_line(&format!("this host refuses a whiteout: {}", e.name()));
            false
        }
    }
}

/// Mark the directory `p` opaque with `user.overlay.opaque`.
fn opaque_at(p: &Path) -> bool {
    let c = sys::CBuf::new(p.to_str().unwrap()).unwrap();
    let fd = sys::open(&c, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC, 0).unwrap();
    let r = sys::fsetxattr(fd, &sys::CBuf::new("user.overlay.opaque").unwrap(), b"y", 0);
    let _ = sys::close(fd);
    if let Err(e) = r {
        pod_testkit::host::skip_line(&format!("this file system refuses user attributes: {}", e.name()));
    }
    r.is_ok()
}

/// An entry of the upper layer hides the same name of the lower layer: a
/// file over a file, a link over a file, a directory over a file.
#[test]
fn an_entry_of_upper_hides_the_same_name_of_lower() {
    let (_d, lower, upper) = two("l-hide");
    put(&lower, "etc/hosts", "lower\n", 0o644);
    put(&upper, "etc/hosts", "upper\n", 0o600);
    put(&lower, "etc/a", "lower a\n", 0o644);
    link(&upper, "etc/a", "hosts");
    put(&lower, "etc/b", "lower b\n", 0o644);
    put(&upper, "etc/b/inner", "upper inner\n", 0o644);
    let r = Root::layers(&lower, &upper).unwrap();
    assert_eq!(r.read_text("etc/hosts").unwrap().as_deref(), Some("upper\n"), "the upper layer hides the lower one");
    assert_eq!(r.kind("etc/hosts").unwrap(), Kind::Regular { len: 6, mode: 0o600 });
    assert_eq!(r.kind("etc/a").unwrap(), Kind::Symlink("hosts".to_string()));
    assert_eq!(r.read_following("etc/a").unwrap().as_deref(), Some(&b"upper\n"[..]));
    assert_eq!(r.kind("etc/b").unwrap(), Kind::Dir);
    assert_eq!(r.list("etc/b").unwrap(), ["inner"]);
    // A file only the lower layer holds shows through.
    put(&lower, "etc/only", "below\n", 0o644);
    assert_eq!(r.read_text("etc/only").unwrap().as_deref(), Some("below\n"));
}

/// A listing is the union of both layers, sorted, with each name one time;
/// a whiteout hides its name, and an opaque directory hides the lower one.
#[test]
fn a_listing_of_two_layers_is_the_union() {
    let (_d, lower, upper) = two("l-union");
    for n in ["a.list", "c.list", "gone.list"] {
        put(&lower, &format!("etc/apt/sources.list.d/{n}"), "x\n", 0o644);
    }
    for n in ["b.list", "c.list"] {
        put(&upper, &format!("etc/apt/sources.list.d/{n}"), "y\n", 0o644);
    }
    let r = Root::layers(&lower, &upper).unwrap();
    let union = ["a.list", "b.list", "c.list", "gone.list"];
    assert_eq!(r.list("etc/apt/sources.list.d").unwrap(), union, "a listing is the union of both layers");
    if !whiteout_at(&upper.join("etc/apt/sources.list.d/gone.list")) {
        return;
    }
    assert_eq!(r.list("etc/apt/sources.list.d").unwrap(), ["a.list", "b.list", "c.list"]);
    assert_eq!(r.kind("etc/apt/sources.list.d/gone.list").unwrap(), Kind::Missing);
    assert!(!r.unlink("etc/apt/sources.list.d/gone.list").unwrap(), "a whiteout is nothing to remove");
    // A file over the whiteout hides the lower file too.
    assert_eq!(r.write("etc/apt/sources.list.d/gone.list", b"new\n", 0o644).unwrap(), Wrote::Created);
    assert_eq!(r.read_text("etc/apt/sources.list.d/gone.list").unwrap().as_deref(), Some("new\n"));
    // A new directory over a whiteout of a lower entry needs an opaque mark.
    std::fs::create_dir_all(lower.join("opt/old")).unwrap();
    std::fs::create_dir_all(upper.join("opt")).unwrap();
    assert!(whiteout_at(&upper.join("opt/old")));
    let e = r.mkdirs("opt/old/new").unwrap_err();
    assert_eq!(layer_refusal(&e), Some(&LayerRefusal::Opaque("opt/old".to_string())), "{e}");
    if opaque_at(&upper.join("etc/apt/sources.list.d")) {
        assert_eq!(r.list("etc/apt/sources.list.d").unwrap(), ["b.list", "c.list", "gone.list"]);
        assert_eq!(r.kind("etc/apt/sources.list.d/a.list").unwrap(), Kind::Missing);
    }
}

/// A change of a file that only the lower layer holds copies it up with
/// its bytes, owner and mode, then changes the copy; each parent directory
/// is copied up with its mode. The lower layer keeps its digest.
#[test]
fn a_change_of_a_lower_file_copies_it_up_with_its_mode() {
    let (_d, lower, upper) = two("l-copyup");
    put(&lower, "etc/passwd", "root:x:0:0::/root:/bin/sh\n", 0o440);
    put(&lower, "usr/share/doc/keep", "kept\n", 0o604);
    std::fs::set_permissions(lower.join("etc"), std::fs::Permissions::from_mode(0o751)).unwrap();
    std::fs::set_permissions(lower.join("usr/share"), std::fs::Permissions::from_mode(0o705)).unwrap();
    put(&lower, "etc/ssl/bundle.pem", "image\n", 0o644);
    let before = digest(&lower, true);
    let r = Root::layers(&lower, &upper).unwrap();
    // The write bit of `identity`: the same bytes, a new mode.
    assert_eq!(r.write("etc/passwd", b"root:x:0:0::/root:/bin/sh\n", 0o640).unwrap(), Wrote::Replaced);
    assert_eq!(std::fs::read(upper.join("etc/passwd")).unwrap(), b"root:x:0:0::/root:/bin/sh\n");
    assert_eq!(mode_of(&upper.join("etc/passwd")), 0o640);
    assert_eq!(mode_of(&upper.join("etc")), 0o751, "the parent keeps its mode");
    // A new file under a new directory under a directory of the lower layer
    // alone.
    assert_eq!(r.write("usr/new/deep/file", b"x\n", 0o644).unwrap(), Wrote::Created);
    assert!(upper.join("usr/new/deep/file").is_file());
    // The copy itself keeps the mode of the lower file.
    let l = r.two().unwrap();
    let Ok((place, name)) = l.resolve_parent("usr/share/doc/keep", false).unwrap() else { panic!("no place") };
    l.copy_up(&place, &name).unwrap();
    assert_eq!(std::fs::read(upper.join("usr/share/doc/keep")).unwrap(), b"kept\n");
    assert_eq!(mode_of(&upper.join("usr/share/doc/keep")), 0o604);
    assert_eq!(mode_of(&upper.join("usr/share")), 0o705);
    // A follow write changes the copy, not the lower file.
    assert_eq!(r.write_following("etc/ssl/bundle.pem", b"image\nhost\n", 0o644).unwrap(), Ok(Wrote::Replaced));
    assert_eq!(std::fs::read(upper.join("etc/ssl/bundle.pem")).unwrap(), b"image\nhost\n");
    assert_eq!(digest(&lower, true), before, "a change went to the lower layer");
}

/// A remove of an entry that the lower layer holds is refused with its
/// type, and nothing changes; an entry of the upper layer alone goes.
#[test]
fn a_remove_of_a_lower_entry_is_refused_with_its_type() {
    let (_d, lower, upper) = two("l-remove");
    link(&lower, "dev/null", "/nowhere");
    put(&lower, "etc/both", "lower\n", 0o644);
    put(&upper, "etc/both", "upper\n", 0o644);
    put(&upper, "etc/mine", "upper\n", 0o644);
    let before = digest(&lower, true);
    let r = Root::layers(&lower, &upper).unwrap();
    for rel in ["dev/null", "etc/both"] {
        let e = r.unlink(rel).unwrap_err();
        assert_eq!(layer_refusal(&e), Some(&LayerRefusal::Remove(rel.to_string())), "{e}");
        assert_eq!(e.class, pod_core::Class::Unsupported);
    }
    assert_eq!(std::fs::read(upper.join("etc/both")).unwrap(), b"upper\n", "the upper entry over a lower one stays");
    assert!(r.unlink("etc/mine").unwrap());
    assert!(!upper.join("etc/mine").exists());
    assert!(!r.unlink("etc/absent").unwrap());
    assert_eq!(digest(&lower, true), before);
    assert!(layer_refusal(&error::policy("another failure")).is_none());
}

/// A link of the merged view is followed, held in the root: a relative
/// and an absolute target, a target of the other layer, `..` above the
/// root, and a loop.
#[test]
fn a_link_resolves_in_the_merged_view() {
    let (_d, lower, upper) = two("l-link");
    link(&lower, "lib", "usr/lib");
    put(&lower, "usr/lib/x86_64-linux-gnu/libc.so.6", "libc\n", 0o755);
    put(&upper, "usr/lib/ld-musl-x86_64.so.1", "musl\n", 0o755);
    std::fs::create_dir_all(lower.join("var/lib/ca/pem")).unwrap();
    link(&lower, "etc/ssl/certs", "/var/lib/ca/pem");
    link(&lower, "etc/up", "../../../../../etc");
    link(&lower, "etc/loop-a", "loop-b");
    link(&lower, "etc/loop-b", "loop-a");
    let before = digest(&lower, true);
    let r = Root::layers(&lower, &upper).unwrap();
    assert_eq!(r.list("lib").unwrap(), ["ld-musl-x86_64.so.1", "x86_64-linux-gnu"]);
    assert_eq!(crate::identity::probe_libc(&r).unwrap(), crate::Libc::Glibc);
    assert_eq!(r.reach("etc/ssl/certs").unwrap(), Reach::Yes);
    assert_eq!(r.write_following("etc/ssl/certs/new.pem", b"pem\n", 0o644).unwrap(), Ok(Wrote::Created));
    assert_eq!(std::fs::read(upper.join("var/lib/ca/pem/new.pem")).unwrap(), b"pem\n");
    assert_eq!(r.write("etc/ssl/certs/x.0", b"x\n", 0o644).unwrap(), Wrote::Created);
    assert!(upper.join("var/lib/ca/pem/x.0").is_file());
    assert_eq!(r.dir_following("etc/up").unwrap(), Some(true));
    assert_eq!(r.list("etc/up/ssl").unwrap(), ["certs"]);
    assert_eq!(r.read_following("etc/loop-a").unwrap(), None);
    assert_eq!(r.dir_following("etc/loop-a").unwrap(), Some(false));
    assert_eq!(digest(&lower, true), before);
}

/// Two layers that hold each other are refused: a write to the upper
/// layer would change the lower one.
#[test]
fn layers_that_hold_each_other_are_refused() {
    let (_d, lower, upper) = two("l-nest");
    std::fs::create_dir_all(lower.join("inner")).unwrap();
    for (l, u) in [(&lower, lower.join("inner")), (&lower, lower.clone()), (&upper.join("x"), upper.clone())] {
        std::fs::create_dir_all(l).unwrap();
        let e = Root::layers(l, &u).err().expect("the layers hold each other");
        assert_eq!(e.class, pod_core::Class::Usage, "{e}");
    }
}

mod complete;
