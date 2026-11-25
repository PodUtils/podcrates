//! The host name on the namespace rung (PodUtils/podvm#403): the payload
//! of a plan with a name sees that name in `uname -n` and in
//! `/proc/sys/kernel/hostname`, and the host keeps its own.
//!
//! The fixture is a static `busybox` of the host (`PODVM_T486_BUSYBOX`, or
//! `/bin/busybox` where it is static), as for the test of the overlay root.
//! Where the host has none, or the kernel refuses the namespaces, the test
//! says so and passes as skipped.
#![cfg(target_os = "linux")]

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use pod_enter::{Fds, Names, Plan, RootDir, run_namespace};

const NAME: &str = "unit1";

fn busybox() -> Result<PathBuf, String> {
    let p = std::env::var_os("PODVM_T486_BUSYBOX").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/bin/busybox"));
    let mut head = [0u8; 4096];
    let n = fs::File::open(&p).and_then(|mut f| f.read(&mut head)).map_err(|e| format!("{}: {e}", p.display()))?;
    if head[..n].windows(b"ld-linux".len()).any(|w| w == b"ld-linux")
        || head[..n].windows(b"ld-musl".len()).any(|w| w == b"ld-musl")
    {
        return Err(format!("{} is not static", p.display()));
    }
    Ok(p)
}

/// An image with `/bin/busybox`, its `sh`, `uname` and `cat`, and `/tmp`.
fn image(bb: &Path, tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pod-enter-hostname-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for d in ["bin", "tmp", "proc"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    fs::copy(bb, root.join("bin/busybox")).unwrap();
    fs::set_permissions(root.join("bin/busybox"), fs::Permissions::from_mode(0o755)).unwrap();
    for applet in ["sh", "uname", "cat", "test"] {
        std::os::unix::fs::symlink("busybox", root.join("bin").join(applet)).unwrap();
    }
    root
}

fn plan(script: &str, hostname: Option<&str>) -> Plan {
    let env = vec![format!("PATH={}", Plan::DEFAULT_PATH)];
    Plan {
        argv: vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()],
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
        hostname: hostname.map(str::to_string),
        volumes: Vec::new(),
        userns_ids: None,
        pid_namespace: false,
        tmpfs: Vec::new(),
        relay_title: None,
    }
}

#[test]
fn the_payload_sees_the_name_of_the_plan_and_the_host_keeps_its_own() {
    let bb = match busybox() {
        Ok(bb) => bb,
        Err(why) => {
            pod_testkit::host::skip_line(&format!("no static busybox: {why}"));
            return;
        }
    };
    let root = image(&bb, "named");
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let before = fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    let script = format!(
        "test \"$(uname -n)\" = {NAME} || exit 3; test \"$(cat /proc/sys/kernel/hostname)\" = {NAME} || exit 4"
    );
    let mut err = Vec::new();
    match run_namespace(&dir, &plan(&script, Some(NAME)), &mut err) {
        Ok(code) => assert_eq!(code, 0, "the payload saw another name: {}", String::from_utf8_lossy(&err)),
        // A host with no user or UTS namespace refuses the rung by name;
        // with a host name there is no fall back to chroot.
        Err(e) => {
            let e = e.to_string();
            assert!(e.contains("needs the namespace rung"), "{e}");
            pod_testkit::host::skip_line(&format!("the namespace rung is refused here: {e}"));
        }
    }
    assert_eq!(fs::read_to_string("/proc/sys/kernel/hostname").unwrap(), before, "the host's name changed");
    let _ = fs::remove_dir_all(&root);
}

/// With no name, the payload sees the host's name, as before.
#[test]
fn a_plan_with_no_name_keeps_the_host_name() {
    let Ok(bb) = busybox() else {
        pod_testkit::host::skip_line("no static busybox");
        return;
    };
    let root = image(&bb, "plain");
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    let script = format!("test \"$(uname -n)\" = {}", host.trim());
    match run_namespace(&dir, &plan(&script, None), &mut std::io::sink()) {
        Ok(code) => assert_eq!(code, 0),
        // A user with no namespace and no chroot (a runner with no root):
        // the root script runs this test as root, where a skip fails.
        Err(e) => pod_testkit::host::skip_line(&format!("no rung with a chroot here: {e}")),
    }
    let _ = fs::remove_dir_all(&root);
}
