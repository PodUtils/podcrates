//! The cases of T-1638, T-1639 and T-1641: a hard limit of host root, the
//! `/dev` of a read-only root, a caller with an ambient capability, and a
//! caller whose `/proc` is of another PID namespace.

use std::os::unix::fs::FileTypeExt;

use pod_enter::{Limit, Resource, RootDir, spawn_namespace};
use pod_hostprobe::sys::{self as k, rlimit};

use super::run::{EPERM, EROFS, expect, host_root, plan, start, tree};

const MIB: u64 = 1 << 20;

/// `CAP_SYS_RESOURCE`.
const CAP_SYS_RESOURCE: u32 = 24;

/// A limit of `memlock` over the hard value of the caller. Host root with
/// `CAP_SYS_RESOURCE` raises it before the lock, and the payload holds it.
/// Each other caller has no `CAP_SYS_RESOURCE` of the initial user
/// namespace (a container can take it from root too): the child refuses
/// the limit by name, as before.
pub fn limit_over_hard(pid: bool) -> Result<(), String> {
    let (soft, hard) = k::prlimit(rlimit::MEMLOCK).map_err(|e| format!("prlimit: {e:?}"))?;
    let resource = pod_testkit::nobody::effective_caps().is_some_and(|c| c & (1 << CAP_SYS_RESOURCE) != 0);
    let host = host_root() && resource;
    if !host && hard == rlimit::INFINITY {
        return Ok(());
    }
    // Host root lowers its own hard value for the case, so that a raise is
    // over it on each host; it raises it back after.
    if host {
        k::set_rlimit(rlimit::MEMLOCK, soft.min(MIB), MIB).map_err(|e| format!("setrlimit: {e:?}"))?;
    }
    let want = if host { 2 * MIB } else { hard.saturating_add(MIB) };
    let t = tree(if pid { "limit-pid" } else { "limit" });
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let op = format!("hard:{}", rlimit::MEMLOCK);
    let mut p = plan(&t, std::slice::from_ref(&op), pid, false);
    p.limits = vec![Limit { resource: Resource::Memlock, soft: want, hard: want }];
    let mut err = Vec::new();
    let r = start(&mut p, |p| spawn_namespace(&root, p, &mut err));
    if host {
        k::set_rlimit(rlimit::MEMLOCK, soft, hard).map_err(|e| format!("setrlimit back: {e:?}"))?;
    }
    match (host, r) {
        (true, Ok(r)) => {
            let out = expect(&r, &[(op.as_str(), want as i64)]);
            r.end()?;
            out
        }
        (true, Err(e)) => Err(format!("host root could not raise the hard limit: {e}")),
        (false, Err(e)) if e.contains("RLIMIT_MEMLOCK") => Ok(()),
        (false, Err(e)) => Err(format!("another refusal: {e}")),
        (false, Ok(r)) => {
            let _ = r.end();
            Err("a caller with no CAP_SYS_RESOURCE raised a hard limit".to_string())
        }
    }
}

/// The `/dev` of a read-only root is a tmpfs of the rung: a write there
/// passes and does not reach the image, `/dev/shm` takes a write, and
/// `/dev/null` is the host's device, also where the image holds a link at
/// `/dev/null` (a payload of an earlier run could leave one).
pub fn dev_of_its_own(pid: bool) -> Result<(), String> {
    let t = tree(if pid { "devfs-pid" } else { "devfs" });
    std::os::unix::fs::symlink("/rw/gone", t.image.join("dev/null")).map_err(|e| e.to_string())?;
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let host_null = std::fs::metadata("/dev/null").is_ok_and(|m| m.file_type().is_char_device());
    let mut want = vec![("write:/dev/made", 0), ("write:/dev/shm/made", 0), ("write:/made", EROFS)];
    if host_null {
        want.push(("chr:/dev/null", 1));
    }
    let ops: Vec<String> = want.iter().map(|(op, _)| op.to_string()).collect();
    let mut err = Vec::new();
    let r = start(&mut plan(&t, &ops, pid, true), |p| spawn_namespace(&root, p, &mut err))?;
    let out = expect(&r, &want);
    let code = r.end()?;
    out?;
    let link = std::fs::symlink_metadata(t.image.join("dev/null")).is_ok_and(|m| m.file_type().is_symlink());
    if code != 0 || t.image.join("dev/made").exists() || !link {
        return Err(format!("exit {code}, or the image changed under /dev: {}", String::from_utf8_lossy(&err)));
    }
    Ok(())
}

/// A caller of uid 65534 with `CAP_SYS_ADMIN` in its ambient set: the rung
/// makes no user namespace and no lock (the payload is not uid 0), so the
/// ambient set must not cross the exec (T-1641). The payload then cannot
/// remount a read-only volume.
pub fn ambient(pid: bool) -> Result<(), String> {
    let t = tree(if pid { "ambient-pid" } else { "ambient" });
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let want = [("remount:/ro", EPERM), ("write:/ro/f", EROFS), ("umount:/ro", EPERM)];
    let ops: Vec<String> = want.iter().map(|(op, _)| op.to_string()).collect();
    let mut err = Vec::new();
    let r = start(&mut plan(&t, &ops, pid, false), |p| spawn_namespace(&root, p, &mut err))?;
    let out = expect(&r, &want);
    let code = r.end()?;
    out.and(if code == 0 { Ok(()) } else { Err(format!("exit {code}: {}", String::from_utf8_lossy(&err))) })
}

/// Host root as pid 1 of a PID namespace whose `/proc` is the host's: the
/// parent writes the maps of the lock through `/proc/PID`, which names
/// another process there. The entry refuses (T-1641), and writes nothing.
pub fn foreign_proc(_pid: bool) -> Result<(), String> {
    let t = tree("foreign");
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let mut err = Vec::new();
    match start(&mut plan(&t, &[], false, false), |p| spawn_namespace(&root, p, &mut err)) {
        Err(e) if e.contains("the /proc of the caller is not the proc of its PID namespace") => Ok(()),
        Err(e) => Err(format!("another refusal: {e}")),
        Ok(r) => {
            let _ = r.end();
            Err("the payload ran".to_string())
        }
    }
}

/// A read-only root whose tree has a mount under it (a tmpfs on `/sub`, in
/// a mount namespace of this thread): the bind of the root is not
/// recursive, so the payload sees the directory of the tree there, which
/// is read-only too (T-1641). A caller that cannot mount has no such tree.
pub fn mount_under_the_tree(pid: bool) -> Result<(), String> {
    if !pod_testkit::nobody::is_root() {
        return Ok(());
    }
    let t = tree(if pid { "under-pid" } else { "under" });
    let sub = t.image.join("sub");
    std::fs::create_dir_all(&sub).map_err(|e| e.to_string())?;
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let c = |s: &str| k::CBuf::new(s).expect("no NUL");
    let want = [("write:/sub/made", EROFS)];
    let ops: Vec<String> = want.iter().map(|(op, _)| op.to_string()).collect();
    let run = || -> Result<(), String> {
        k::unshare(k::CLONE_NEWNS).map_err(|e| format!("unshare: {e:?}"))?;
        let (empty, tmpfs) = (k::cempty(), c("tmpfs"));
        k::mount(&empty, &c("/"), &empty, k::MS_REC | k::MS_PRIVATE).map_err(|e| format!("private: {e:?}"))?;
        let at = c(sub.to_str().unwrap());
        k::mount_data(&tmpfs, &at, &tmpfs, 0, &c("mode=0777")).map_err(|e| format!("the tmpfs: {e:?}"))?;
        let mut err = Vec::new();
        let r = start(&mut plan(&t, &ops, pid, true), |p| spawn_namespace(&root, p, &mut err))?;
        let out = expect(&r, &want);
        r.end()?;
        out
    };
    std::thread::scope(|s| s.spawn(run).join().unwrap_or_else(|_| Err("the thread panicked".to_string())))
}
