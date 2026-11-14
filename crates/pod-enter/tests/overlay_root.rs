//! The acceptance test of podcrates@T-486 (PodUtils/podvm#294, #322): an
//! overlay root in the namespace rung, the image read-only under a tmpfs
//! or directory upper layer.
//!
//! The payload writes a new file, changes a file of the image and removes a
//! file of the image, all under `/`. The test then checks:
//!
//! 1. the payload saw its own writes (its exit code is 0 only if each step
//!    of its script held);
//! 2. the lower layer is byte for byte the same: a digest of each path,
//!    type, mode, size, link target and content before and after;
//! 3. the new and the changed file are in the upper layer, and the removed
//!    file is still in the lower layer.
//!
//! ⚠ One adapter: [`run_in_overlay`] calls the API that T-486 adds. Each
//! other line is the acceptance and stays as it is.
//!
//! The fixture is a static `busybox` of the host (`PODVM_T486_BUSYBOX`, or
//! `/bin/busybox` where it is static). Where the kernel refuses an overlay
//! mount in a user namespace (before Linux 5.11, or a sysctl that denies
//! user namespaces), the test says so and passes as skipped: the rung then
//! falls back, and the fall back is another test.
#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// ⚠ THE ADAPTER. Run `argv` with `lower` as the read-only image and
/// `upper` as the writable layer of an overlay root, in the namespace rung,
/// and give the exit code of the payload, or `Err` with the refusal of the
/// rung (a kernel that denies the mount). Wire it to the call of T-486.
fn run_in_overlay(lower: &Path, upper: &Path, argv: &[&str]) -> Result<i32, String> {
    use pod_enter::{Fds, Names, Plan, Root, RootDir, Upper, run_namespace_root};
    // The work directory of overlayfs: empty, beside the upper layer, on
    // its file system.
    let work = upper.with_file_name("work");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let text = |p: &Path| p.to_str().map(str::to_string).ok_or_else(|| format!("{p:?} is not UTF-8"));
    let (upper, work) = (text(upper)?, text(&work)?);
    let lower = RootDir::open(&text(lower)?).map_err(|e| e.to_string())?;
    let env = vec![format!("PATH={}", Plan::DEFAULT_PATH)];
    let plan = Plan {
        argv: argv.iter().map(|a| a.to_string()).collect(),
        path_dirs: Plan::path_from(&env),
        env,
        working_dir: "/".to_string(),
        fds: Fds::default(),
        banner: String::new(),
        names: Names::DEFAULT,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
        confine: None,
    };
    let root = Root::Overlay { lower: &lower, upper: Upper::Dir { upper: &upper, work: &work } };
    run_namespace_root(root, &plan, &mut std::io::sink()).map_err(|e| e.to_string())
}

/// A scratch directory of this test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("t486-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A static busybox, or the reason that there is none.
fn busybox() -> Result<PathBuf, String> {
    let p = std::env::var_os("PODVM_T486_BUSYBOX").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/bin/busybox"));
    let mut head = [0u8; 4096];
    let n = fs::File::open(&p).and_then(|mut f| f.read(&mut head)).map_err(|e| format!("{}: {e}", p.display()))?;
    // A dynamic ELF has a PT_INTERP that names its loader.
    if head[..n].windows(b"ld-linux".len()).any(|w| w == b"ld-linux")
        || head[..n].windows(b"ld-musl".len()).any(|w| w == b"ld-musl")
    {
        return Err(format!("{} is not static", p.display()));
    }
    Ok(p)
}

/// The image of the test: `/bin/busybox` with `sh`, `cat`, `rm` and
/// `mkdir`, `/etc/keep`, `/etc/change` and `/etc/remove`.
fn image(lower: &Path, bb: &Path) {
    fs::create_dir_all(lower.join("bin")).unwrap();
    fs::create_dir_all(lower.join("etc")).unwrap();
    fs::create_dir_all(lower.join("tmp")).unwrap();
    fs::copy(bb, lower.join("bin/busybox")).unwrap();
    fs::set_permissions(lower.join("bin/busybox"), fs::Permissions::from_mode(0o755)).unwrap();
    for applet in ["sh", "cat", "rm", "mkdir"] {
        std::os::unix::fs::symlink("busybox", lower.join("bin").join(applet)).unwrap();
    }
    fs::write(lower.join("etc/keep"), b"keep\n").unwrap();
    fs::write(lower.join("etc/change"), b"before\n").unwrap();
    fs::write(lower.join("etc/remove"), b"remove\n").unwrap();
}

/// One line for each path under `root`: type, mode, size, link target and
/// the bytes of a regular file. Two equal maps are two equal trees.
fn digest(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            let m = fs::symlink_metadata(&p).unwrap();
            let rel = p.strip_prefix(root).unwrap().display().to_string();
            let kind = if m.file_type().is_symlink() {
                format!("link {}", fs::read_link(&p).unwrap().display())
            } else if m.is_dir() {
                stack.push(p.clone());
                "dir".to_string()
            } else {
                format!("file {:?}", fs::read(&p).unwrap())
            };
            out.insert(rel, format!("{:o} {} {kind}", m.mode(), m.size()));
        }
    }
    out
}

#[test]
fn an_overlay_root_keeps_the_image_and_takes_each_write_in_the_upper_layer() {
    let bb = match busybox() {
        Ok(p) => p,
        Err(why) => {
            eprintln!("SKIP: no static busybox: {why}");
            return;
        }
    };
    let d = scratch("root");
    let (lower, upper) = (d.join("lower"), d.join("upper"));
    fs::create_dir_all(&upper).unwrap();
    image(&lower, &bb);
    let before = digest(&lower);

    // Each step must hold, or the script exits with the number of the step.
    let script = "\
        echo new > /etc/new || exit 1; \
        [ \"$(cat /etc/new)\" = new ] || exit 2; \
        echo after > /etc/change || exit 3; \
        [ \"$(cat /etc/change)\" = after ] || exit 4; \
        rm /etc/remove || exit 5; \
        [ ! -e /etc/remove ] || exit 6; \
        [ \"$(cat /etc/keep)\" = keep ] || exit 7; \
        mkdir /var && echo x > /var/x || exit 8; \
        exit 0";
    let code = match run_in_overlay(&lower, &upper, &["/bin/sh", "-c", script]) {
        Ok(c) => c,
        Err(why) if why.contains("wire run_in_overlay") => panic!("{why}"),
        Err(why) => {
            eprintln!("SKIP: the rung refused the overlay root: {why}");
            return;
        }
    };
    assert_eq!(code, 0, "the step {code} of the script failed in the overlay root");

    // 2. The image is the same, byte for byte.
    assert_eq!(digest(&lower), before, "the lower layer changed");

    // 3. The writes are in the upper layer; the removed file stays below.
    assert_eq!(fs::read(upper.join("etc/new")).unwrap(), b"new\n");
    assert_eq!(fs::read(upper.join("etc/change")).unwrap(), b"after\n");
    assert_eq!(fs::read(upper.join("var/x")).unwrap(), b"x\n");
    assert_eq!(fs::read(lower.join("etc/remove")).unwrap(), b"remove\n");
    assert!(!upper.join("etc/keep").exists(), "a file that was only read went up");

    let _ = fs::remove_dir_all(&d);
}
