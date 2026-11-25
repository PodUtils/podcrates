//! The tmpfs mounts of a plan with a byte limit on the namespace rung
//! (PodUtils/podvm#667), as root and as a caller with no capabilities (the
//! harness of the volumes, `userns/volumes.rs`). A write under the limit
//! passes, a write over it gets `ENOSPC`, a limit that the kernel does not
//! enforce is a refusal by name, and a join of the run finds the mount with
//! its limit and mounts nothing.

use std::num::NonZeroU64;

use super::*;
use pod_enter::{NS_TMPFS_LIMIT_STEP, NS_TMPFS_LINK_STEP, TmpfsMount, ns_setup_step, tmpfs_label};

/// The limit of the tests: 1 MiB, whole pages of 4 KiB and of 64 KiB.
const LIMIT: u64 = 1 << 20;

fn work(bytes: u64) -> TmpfsMount {
    TmpfsMount { guest: "/work".to_string(), bytes: NonZeroU64::new(bytes).unwrap() }
}

/// The checks of the payload on the mount `/work` of [`LIMIT`]: one line in
/// `/proc/self/mountinfo`, with `nosuid`, `nodev` and the size.
const MOUNTED: &str = "test \"$(grep -c ' /work ' /proc/self/mountinfo)\" = 1 || exit 20; \
     grep ' /work ' /proc/self/mountinfo | grep -Eq ' rw,nosuid,nodev[^ ]* .*- tmpfs [^ ]* [^ ]*size=1024k' || exit 21";

fn who() -> &'static str {
    if nobody::is_root() { "root" } else { "a caller with no capabilities" }
}

/// The run: the working directory in the tmpfs (the image does not hold
/// it), a write under the limit, a write over it with `ENOSPC`; then a
/// limit that wraps to no limit in the kernel, and a guest path through a
/// link of the image, each refused by name before the exec.
pub fn limited(root: &Path, _vols: &Path) {
    if !nobody::is_root() && !no_capabilities() {
        return;
    }
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    // The error of the full write goes to the /tmp of the rung: the tmpfs
    // has no room for it.
    let script = format!(
        "{MOUNTED}; test \"$(pwd)\" = /work/sub || exit 3; \
         busybox yes 2>/tmp/yes | head -c 65536 > small || exit 4; test \"$(wc -c < small)\" = 65536 || exit 5; \
         (busybox yes | head -c 2000000) 2>/tmp/yes | busybox dd of=/work/big bs=65536 2>/tmp/err && exit 6; \
         grep -q 'No space left on device' /tmp/err || exit 7"
    );
    let mut p = plan(&script, None);
    p.tmpfs = vec![work(LIMIT)];
    p.working_dir = "/work/sub".to_string();
    let mut err = Vec::new();
    let code = run_namespace(&dir, &p, &mut err);
    let err = String::from_utf8_lossy(&err);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the payload with a tmpfs of {LIMIT} bytes as {}: {err}", who());
    assert!(!err.contains("entered chroot instead"), "{err}");
    assert!(root.join("work").is_dir(), "the child made the mount point in the image");
    assert_eq!(fs::read_dir(root.join("work")).unwrap().count(), 0, "a write of the payload landed in the image");

    // A size near u64::MAX wraps to a tmpfs with no limit in the kernel:
    // the check after the mount refuses it, and the payload does not run.
    let mut p = plan("exit 0", None);
    p.tmpfs = vec![work(u64::MAX)];
    let mut err = Vec::new();
    let wrapped = run_namespace(&dir, &p, &mut err).map_err(|e| e.to_string());
    let label = tmpfs_label(&p.tmpfs[0]);
    assert!(
        wrapped.as_ref().is_err_and(|e| e.contains(&label) && e.contains("did not enforce its byte limit")),
        "a limit that the kernel does not enforce as {}: {wrapped:?}",
        who()
    );
    assert!(ns_setup_step(NS_TMPFS_LIMIT_STEP).is_some());
    assert!(!String::from_utf8_lossy(&err).contains("entered chroot instead"));

    // A guest path through a link of the image: refused with the step, and
    // nothing made through the link.
    let mut p = plan("exit 0", None);
    p.tmpfs = vec![TmpfsMount { guest: "/lnk/x".to_string(), bytes: NonZeroU64::new(LIMIT).unwrap() }];
    let mut err = Vec::new();
    let linked = run_namespace(&dir, &p, &mut err).map_err(|e| e.to_string());
    let words = ns_setup_step(NS_TMPFS_LINK_STEP).unwrap();
    assert!(linked.as_ref().is_err_and(|e| e.contains(words) && e.contains("/lnk/x:size=")), "{linked:?}");
    assert!(!root.join("tmp/x").exists(), "the walk made a path through the link");
}

/// A run with the tmpfs that writes a file and sleeps, then a join of it
/// with the same plan: the join sees the file and the one mount with its
/// limit, and does not fall back to a mount namespace of its own.
pub fn joined(root: &Path, _vols: &Path) {
    if !nobody::is_root() && !no_capabilities() {
        return;
    }
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let mut p = plan("touch /work/of-the-run && exec sleep 60", None);
    p.tmpfs = vec![work(LIMIT)];
    let mut err = Vec::new();
    let run = spawn_namespace(&dir, &p, &mut err).unwrap();
    // The exec of `sleep` comes after the write of the file.
    let comm = format!("/proc/{}/comm", run.pid);
    for _ in 0..200 {
        if fs::read_to_string(&comm).is_ok_and(|c| c.trim_end() == "sleep") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let pidfd = sys::pidfd_open(run.pid).unwrap();
    let mut p = plan(&format!("test -e /work/of-the-run || exit 3; {MOUNTED}"), None);
    p.tmpfs = vec![work(LIMIT)];
    let mut joined = Vec::new();
    let code = spawn_joined(&dir, &p, pidfd, &mut joined).and_then(|c| c.wait());
    let _ = sys::close(pidfd);
    let _ = sys::kill(run.pid, 9);
    let _ = run.wait();
    let (err, joined) = (String::from_utf8_lossy(&err), String::from_utf8_lossy(&joined));
    assert!(!err.contains("entered chroot instead"), "the run fell back: {err}");
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the joined exec as {}: {joined}", who());
    assert!(!joined.contains("does not share the mounts"), "{joined}");
}
