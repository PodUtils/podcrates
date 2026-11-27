//! The view of two layers before the fork (T-750): what the upper layer
//! adds, hides and removes, and a volume over both.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::*;
use crate::abi::{ResolveKind, resolve_dir_in, resolve_in};
use crate::{Fds, Names, VolumeBind};

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

fn text(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// A whiteout at `p`, or a skip line where this host refuses one to this
/// process (Linux 5.8 gives it with no privilege on most file systems).
fn whiteout_at(p: &Path) -> bool {
    let c = CBuf::new(text(p)).unwrap();
    match sys::mknodat(sys::AT_FDCWD as i64, &c, sys::S_IFCHR | 0o600, 0) {
        Ok(_) => true,
        Err(e) => {
            pod_testkit::host::skip_line(&format!("this host refuses a whiteout: {}", e.name()));
            false
        }
    }
}

/// Mark the directory `p` opaque with `user.overlay.opaque`, or a skip
/// line where the file system refuses user attributes.
fn opaque_at(p: &Path) -> bool {
    let c = CBuf::new(text(p)).unwrap();
    let fd = sys::open(&c, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC, 0).unwrap();
    let r = sys::fsetxattr(fd, &CBuf::new("user.overlay.opaque").unwrap(), b"y", 0);
    let _ = sys::close(fd);
    if let Err(e) = r {
        pod_testkit::host::skip_line(&format!("this file system refuses user attributes: {}", e.name()));
    }
    r.is_ok()
}

/// The two layers of a test: `lower` holds `both/l`, `gone`, `gonedir/`,
/// `op/old`, `fd/x` and `vol/l`; `upper` holds `only/`, `both/u`, a
/// whiteout at `gone` and at `gonedir`, the opaque `op/` with `op/new`,
/// the file `fd`, `vol/u` and the link `ln` to `/both`.
struct Layers {
    dir: PathBuf,
    root: RootDir,
    upper: String,
    opaque: bool,
}

impl Drop for Layers {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn layers(tag: &str) -> Option<Layers> {
    let dir = std::env::temp_dir().join(format!("pod-enter-ns-upper-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (lower, upper) = (dir.join("lower"), dir.join("upper"));
    for d in ["both", "gonedir", "op", "fd", "vol"].map(|d| lower.join(d)) {
        std::fs::create_dir_all(d).unwrap();
    }
    for f in ["both/l", "gone", "op/old", "fd/x", "vol/l"] {
        std::fs::write(lower.join(f), f).unwrap();
    }
    for d in ["only", "both", "op", "vol"].map(|d| upper.join(d)) {
        std::fs::create_dir_all(d).unwrap();
    }
    for f in ["both/u", "op/new", "fd", "vol/u"] {
        std::fs::write(upper.join(f), f).unwrap();
    }
    std::os::unix::fs::symlink("/both", upper.join("ln")).unwrap();
    let made = whiteout_at(&upper.join("gone")) && whiteout_at(&upper.join("gonedir"));
    let opaque = opaque_at(&upper.join("op"));
    let root = RootDir::open(text(&lower)).unwrap();
    let l = Layers { upper: text(&upper).to_string(), dir, root, opaque };
    made.then_some(l)
}

/// The view of a mount in a user namespace, with `userxattr`: the mark of
/// `opaque_at` is the one that it reads.
fn view(l: &Layers, volumes: Vec<VolumeBind>) -> View {
    View::new(&l.root, &plan(volumes), Some(UpperDir { path: &l.upper, userxattr: true }))
}

/// The upper layer adds and hides entries, a whiteout removes one, an
/// opaque directory hides the lower one, and a file of the upper layer
/// hides a directory of the lower one. A link of the upper layer resolves
/// in the merged view.
#[test]
fn the_view_shows_the_upper_layer_over_the_lower_one() {
    let Some(l) = layers("view") else { return };
    let v = view(&l, Vec::new());
    let (lower, upper) = (l.dir.join("lower"), l.dir.join("upper"));
    assert_eq!(resolve_dir_in(&v, "/only"), Ok(upper.join("only")), "a directory of the upper layer alone");
    assert_eq!(resolve_in(&v, "/both/l"), Ok(lower.join("both/l")), "a merged directory: the lower file");
    assert_eq!(resolve_in(&v, "/both/u"), Ok(upper.join("both/u")), "a merged directory: the upper file");
    assert_eq!(resolve_in(&v, "/gone"), Err(ResolveKind::Absent), "a file that a whiteout removes");
    assert_eq!(resolve_dir_in(&v, "/gonedir"), Err(ResolveKind::Absent), "a directory that a whiteout removes");
    assert_eq!(resolve_in(&v, "/fd/x"), Err(ResolveKind::Absent), "a file of the upper layer over a directory");
    assert_eq!(resolve_in(&v, "/ln/l"), Ok(lower.join("both/l")), "a link of the upper layer");
    assert_eq!(resolve_in(&v, "/op/new"), Ok(upper.join("op/new")));
    if l.opaque {
        assert_eq!(resolve_in(&v, "/op/old"), Err(ResolveKind::Absent), "an entry under an opaque directory");
    }
    // A mount of a host root reads `trusted.` alone: the mark of `user.`
    // makes no opaque directory there.
    let host_root = View::new(&l.root, &plan(Vec::new()), Some(UpperDir { path: &l.upper, userxattr: false }));
    assert_eq!(resolve_in(&host_root, "/op/old"), Ok(lower.join("op/old")), "user. on a mount of a host root");
    // With no upper layer, the image alone.
    let bare = View::new(&l.root, &plan(Vec::new()), None);
    assert_eq!(resolve_dir_in(&bare, "/only"), Err(ResolveKind::Absent));
    assert_eq!(resolve_in(&bare, "/gone"), Ok(lower.join("gone")));
}

/// A volume hides both layers at its guest path.
#[test]
fn a_volume_hides_both_layers() {
    let Some(l) = layers("volume") else { return };
    let host = l.dir.join("host");
    std::fs::create_dir_all(&host).unwrap();
    std::fs::write(host.join("h"), "h").unwrap();
    let bind =
        VolumeBind { host: text(&host).to_string(), guest: "/vol".to_string(), read_only: false, host_base: None };
    let v = view(&l, vec![bind]);
    assert_eq!(resolve_in(&v, "/vol/h"), Ok(host.join("h")));
    assert_eq!(resolve_in(&v, "/vol/u"), Err(ResolveKind::Absent), "the file of the upper layer under the volume");
    assert_eq!(resolve_in(&v, "/vol/l"), Err(ResolveKind::Absent), "the file of the lower layer under the volume");
    assert_eq!(resolve_in(&v, "/both/u"), Ok(l.dir.join("upper/both/u")));
}

/// The check of the working directory reads the view: a directory of the
/// upper layer alone passes, and one that a whiteout removes is refused
/// with the upper layer in its words.
#[test]
fn the_working_directory_is_checked_in_the_view() {
    let Some(l) = layers("workdir") else { return };
    let mut p = plan(Vec::new());
    p.working_dir = "/only".to_string();
    assert!(crate::ns_volume::check_workdir(&l.root, &p, Some(UpperDir { path: &l.upper, userxattr: true })).is_ok());
    assert!(crate::ns_volume::check_workdir(&l.root, &p, None).is_err(), "the image alone has no /only");
    p.working_dir = "/gonedir".to_string();
    let e = crate::ns_volume::check_workdir(&l.root, &p, Some(UpperDir { path: &l.upper, userxattr: true }))
        .unwrap_err()
        .to_string();
    assert!(e.contains("\"/gonedir\"") && e.contains(&format!("under the upper layer {:?}", l.upper)), "{e}");
}

/// The opener of the confinement takes the view: a rule on a directory of
/// the upper layer alone opens it, and a path that a whiteout removes is
/// `ENOENT`.
#[test]
fn a_rule_opens_in_the_view() {
    let Some(l) = layers("rule") else { return };
    let v = view(&l, Vec::new());
    let file = |guest: &str| {
        let fd = crate::confine::open_through(&v, &l.root, guest).map_err(|e| e.name())?;
        let st = sys::fstatat(fd, &sys::cempty(), sys::AT_EMPTY_PATH).unwrap();
        let _ = sys::close(fd);
        Ok::<_, String>(st.st_ino)
    };
    let ino = |p: PathBuf| Ok::<_, String>(std::fs::metadata(p).unwrap().ino());
    assert_eq!(file("/only"), ino(l.dir.join("upper/only")));
    assert_eq!(file("/both/l"), ino(l.dir.join("lower/both/l")));
    assert_eq!(file("/gone"), Err("ENOENT".to_string()));
    assert_eq!(file("/gonedir"), Err("ENOENT".to_string()));
}
