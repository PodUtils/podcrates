//! The completion of a root whose directories the image closed (T-604): a
//! write opens its directory for the step, and the mode of the image comes
//! back after it. Root passes each check of the permissions: as root, the
//! worker runs as uid 1000.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::testing::{Scratch, as_uid_1000, no_in_root};
use crate::{Settings, complete, reach};

/// The variable that makes the workers below run.
const WORKER: &str = "POD_ROOTFS_TEST_CLOSED_ETC";

fn mode(p: &Path) -> u32 {
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
}

fn set(p: &Path, m: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

/// A musl root with `etc` and `etc/apt` at 0555. `etc/apt` has no
/// `apt.conf.d`, so the drop-in of apt makes a directory in it.
fn closed_root(at: &Path) {
    for d in ["etc/apt", "lib"] {
        std::fs::create_dir_all(at.join(d)).unwrap();
    }
    for (f, body) in [
        ("etc/passwd", "root:x:0:0:root:/root:/bin/sh\n"),
        ("etc/group", "root:x:0:\n"),
        ("etc/hosts", "127.0.0.1 localhost\n"),
        ("etc/resolv.conf", "nameserver 8.8.8.8\n"),
        ("etc/apt/sources.list", "deb http://deb.debian.org/debian bookworm main\n"),
        ("lib/ld-musl-x86_64.so.1", ""),
    ] {
        std::fs::write(at.join(f), body).unwrap();
    }
    set(&at.join("etc/apt"), 0o555);
    set(&at.join("etc"), 0o555);
}

/// Complete `rootfs` (with `upper` on two layers) and check the rows and the
/// modes of `written`, the directory that takes the writes.
fn complete_closed(s: &Scratch, rootfs: &Path, upper: Option<&Path>) {
    let resolv = s.at("resolv.conf");
    std::fs::write(&resolv, "nameserver 10.1.2.3\n").unwrap();
    let settings = Settings { host_resolv_conf: resolv, upper: upper.map(Path::to_path_buf), ..Settings::default() };
    let r = complete(rootfs, &settings, &mut reach::Fixed(true)).unwrap();
    let failed: Vec<_> = r.fixups.iter().filter(|f| f.action == crate::Action::Failed).collect();
    assert!(failed.is_empty(), "a fixup failed in a closed directory: {failed:#?}");
    let written = upper.unwrap_or(rootfs);
    assert_eq!(std::fs::read_to_string(written.join("etc/resolv.conf")).unwrap(), "nameserver 10.1.2.3\n");
    assert!(written.join("etc/mtab").exists(), "no etc/mtab");
    // A new directory in a closed directory: the drop-in of apt.
    assert!(written.join("etc/apt/apt.conf.d").is_dir(), "no etc/apt/apt.conf.d in the closed etc/apt");
    for d in ["etc", "etc/apt"] {
        let m = mode(&written.join(d));
        assert_eq!(m, 0o555, "the mode of the image did not come back on {d}: {m:04o}");
    }
    let row = r.fixups.iter().find(|f| f.id == "identity" && f.path == "etc/passwd").expect("an identity row");
    assert!(
        row.degraded && row.detail.contains("mode 0555"),
        "the identity row does not name the closed directory: {row:?}"
    );
}

/// Open each closed directory again, so that the scratch directory goes.
fn reopen(at: &Path) {
    for d in ["etc", "etc/apt"] {
        if at.join(d).is_dir() {
            set(&at.join(d), 0o755);
        }
    }
}

fn as_user() {
    let s = Scratch::new("closed-one");
    let root = s.at("rootfs");
    closed_root(&root);
    complete_closed(&s, &root, None);
    reopen(&root);

    // Two layers: the closed `etc` of the lower layer is copied up with its
    // mode, and each write goes to the upper layer.
    let s = Scratch::new("closed-two");
    let (lower, upper) = (s.at("lower"), s.at("upper"));
    closed_root(&lower);
    std::fs::create_dir(&upper).unwrap();
    complete_closed(&s, &lower, Some(&upper));
    assert_eq!(std::fs::read_to_string(lower.join("etc/resolv.conf")).unwrap(), "nameserver 8.8.8.8\n");
    reopen(&lower);
    reopen(&upper);
}

/// The completion of a root whose `etc` is 0555 writes each file, keeps
/// 0555, and its identity row says that `etc` is closed; on one root and on
/// two layers.
#[test]
fn a_closed_etc_is_completed_and_keeps_its_mode() {
    if no_in_root() {
        return;
    }
    if pod_extract::apply::Ids::current().uid != 0 {
        as_user();
        return;
    }
    as_uid_1000("closed::worker_completes_a_closed_etc", WORKER);
}

/// The half that runs as uid 1000. It does nothing with no [`WORKER`].
#[test]
fn worker_completes_a_closed_etc() {
    if std::env::var_os(WORKER).is_none() {
        return;
    }
    assert_ne!(pod_extract::apply::Ids::current().uid, 0, "the worker must not run as root: it then proves nothing");
    as_user();
}
