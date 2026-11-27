//! The cases of T-1637: the image does not choose the rung, and a payload
//! of uid 0 does not run on the chroot rung after a refused setup.

use pod_enter::{RootDir, spawn_namespace};
use pod_hostprobe::sys as k;

use super::run::{ENOENT, Running, bare, expect, start, tree};

/// The refusal of the fallback of a payload of uid 0.
const UID_0: &str = "a payload of uid 0 needs the pivot and the lock of the namespace rung";

/// Is the payload of the chroot rung uid 0: this process is uid 0 (host
/// root, or uid 0 of its user namespace)?
fn root_payload() -> bool {
    pod_testkit::nobody::is_root()
}

/// An image with no `/tmp`: the child makes it, and the payload runs on the
/// namespace rung (its way out of a `chroot(2)` reaches no host path), with
/// no fallback line.
pub fn no_tmp(pid: bool) -> Result<(), String> {
    let t = tree(if pid { "notmp-pid" } else { "notmp" });
    std::fs::remove_dir(t.image.join("tmp")).map_err(|e| e.to_string())?;
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let escape = format!("escape:{}", t.marker);
    let mut err = Vec::new();
    let r = start(&mut bare(std::slice::from_ref(&escape), pid, false), |p| spawn_namespace(&root, p, &mut err))?;
    let out = expect(&r, &[(escape.as_str(), ENOENT)]);
    let code = r.end()?;
    let text = String::from_utf8_lossy(&err);
    out?;
    if code != 0 || text.contains("entered chroot instead") || !t.image.join("tmp").is_dir() {
        return Err(format!("exit {code}, or no /tmp made in the image: {text}"));
    }
    Ok(())
}

/// An image whose `/tmp` is a link to a host directory: a refusal before
/// the fork, never a fallback.
pub fn tmp_link(_pid: bool) -> Result<(), String> {
    let t = tree("tmplink");
    std::fs::remove_dir(t.image.join("tmp")).map_err(|e| e.to_string())?;
    std::os::unix::fs::symlink(&t.dir, t.image.join("tmp")).map_err(|e| e.to_string())?;
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let mut err = Vec::new();
    match start(&mut bare(&[], false, false), |p| spawn_namespace(&root, p, &mut err)) {
        Err(e) if e.contains("is a symbolic link or a file") && e.contains("does not fall back to chroot") => Ok(()),
        Err(e) => Err(format!("another refusal: {e}")),
        Ok(r) => {
            let _ = r.end();
            Err(format!("the payload ran: {}", String::from_utf8_lossy(&err)))
        }
    }
}

/// A seccomp filter on one thread refuses `unshare(2)` of a mount
/// namespace with `ENOSPC`, as `max_mnt_namespaces` does when another run
/// used it up; the child of the entry takes the filter. The rung falls back
/// to chroot: a payload of uid 0 refuses it with the cause; a payload of
/// another uid gets the chroot rung, which refuses a caller with no
/// capabilities.
pub fn refused_fallback(_pid: bool) -> Result<(), String> {
    let t = tree("fallback");
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let entered = std::thread::scope(|s| {
        s.spawn(|| {
            let newns = k::CLONE_NEWNS as u32;
            let deny =
                [pod_confine::Deny::call("unshare", k::SYS_UNSHARE, k::ENOSPC.0 as u16).when_equal(0, newns, newns)];
            let filter = pod_confine::deny::program(&deny).map_err(|e| format!("the filter: {e:?}"))?;
            k::set_no_new_privs().map_err(|e| format!("no_new_privs: {e:?}"))?;
            k::seccomp_set_filter(&filter, 0).map_err(|e| format!("the filter: {e:?}"))?;
            let mut err = Vec::new();
            Ok::<_, String>((start(&mut bare(&[], false, false), |p| spawn_namespace(&root, p, &mut err)), err))
        })
        .join()
        .unwrap_or_else(|_| Err("the thread panicked".to_string()))
    })?;
    let (entered, err) = entered;
    match entered {
        Ok(r) => {
            let _ = Running::end(r);
            Err(format!("the payload ran on the chroot rung: {}", String::from_utf8_lossy(&err)))
        }
        Err(e) if (e.contains(UID_0) && e.contains("ENOSPC")) != root_payload() => {
            Err(format!("the refusal does not fit a payload of uid {}: {e}", if root_payload() { 0 } else { 65534 }))
        }
        Err(_) => Ok(()),
    }
}
