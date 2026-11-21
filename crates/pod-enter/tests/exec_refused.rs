//! A program that the kernel finds and refuses to exec gives 126, not
//! 127, on each rung (PodUtils/podvm#639). The environment holds one value
//! of 1 MiB, over the bound of the kernel for one string
//! (`MAX_ARG_STRLEN`, 128 KiB), so `execve` fails with `E2BIG`.
//!
//! The namespace rung (or its fall back to chroot) needs root or a user
//! namespace; as another user with neither, its tests say so and pass as
//! skipped. `scripts/ci/root-enter-confine-tests.sh` runs them as root.
#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use pod_enter::status::CANNOT_INVOKE;
use pod_enter::userland::Maps;
use pod_enter::{Error, Fds, Names, Plan, RootDir, spawn_namespace, spawn_userland};

/// One value over the bound of the kernel for one string.
fn big_env() -> String {
    format!("BIG={}", "x".repeat(1 << 20))
}

fn plan(argv: &[&str], env: Vec<String>, path_dirs: &[&str]) -> Plan {
    Plan {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        env,
        working_dir: "/".to_string(),
        fds: Fds::default(),
        banner: String::new(),
        path_dirs: path_dirs.iter().map(|s| s.to_string()).collect(),
        names: Names::DEFAULT,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
        confine: None,
        hostname: None,
        volumes: Vec::new(),
        userns_ids: None,
        pid_namespace: false,
    }
}

/// An image with `/bin/prog` (a script, mode 0755), `/bin/plain` (mode
/// 0644, no execute permission), `/tmp` and `/proc`. No `/usr/bin`: a
/// search of the path finds each program in `/bin` and nothing after it.
fn image(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pod-enter-exec-refused-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for d in ["bin", "tmp", "proc"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    for (name, mode) in [("prog", 0o755), ("plain", 0o644)] {
        fs::write(root.join("bin").join(name), b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(root.join("bin").join(name), fs::Permissions::from_mode(mode)).unwrap();
    }
    root
}

/// The entry on the namespace rung must refuse with 126 and name `errno`.
/// A host with no rung that changes the root says so and skips.
fn expect_refused_on_the_namespace_rung(tag: &str, p: &Plan, errno: &str) {
    let root = image(tag);
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let got = spawn_namespace(&dir, p, &mut std::io::sink());
    let _ = fs::remove_dir_all(&root);
    match got {
        Err(e @ Error::CannotInvoke(_)) => {
            assert_eq!(e.payload_code(), Some(CANNOT_INVOKE));
            assert!(e.to_string().contains(errno), "{e}");
        }
        Err(e) if e.to_string().contains("chroot into the rootfs") => {
            pod_testkit::host::skip_line(&format!("no rung with a chroot here: {e}"));
        }
        Err(e) => panic!("{tag}: not 126: {e} (code {:?})", e.payload_code()),
        Ok(c) => panic!("{tag}: the payload started, exit {:?}", c.wait()),
    }
}

/// `E2BIG` from the first candidate of the path is 126, also when a later
/// candidate is absent (`ENOENT`).
#[test]
fn a_too_large_environment_on_the_namespace_rung_is_found_and_not_invocable() {
    let env = vec!["PATH=/bin:/usr/bin".to_string(), big_env()];
    let p = plan(&["prog"], env, &["/bin", "/usr/bin"]);
    expect_refused_on_the_namespace_rung("e2big", &p, "E2BIG (7)");
}

/// A file with no execute permission (`EACCES`) on the path is 126, also
/// when a later candidate is absent (`ENOENT`), as `execvp` and the shell.
#[test]
fn a_program_with_no_execute_permission_before_an_absent_one_is_not_invocable() {
    let p = plan(&["plain"], vec!["PATH=/bin:/usr/bin".to_string()], &["/bin", "/usr/bin"]);
    expect_refused_on_the_namespace_rung("eacces", &p, "EACCES (13)");
}

/// The userland rung execs its argv one time: `E2BIG` is 126.
#[test]
fn a_too_large_environment_on_the_userland_rung_is_found_and_not_invocable() {
    pod_testkit::need_tool!("/bin/sh");
    let d = std::env::temp_dir().join(format!("pod-enter-exec-refused-userland-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let maps = Maps::of_root(d.to_str().unwrap());
    let root = RootDir::open(maps.rootfs()).unwrap();
    let argv = vec!["/bin/sh".to_string(), "-c".to_string(), "exit 0".to_string()];
    let p = plan(&["sh"], vec!["PATH=/bin:/usr/bin".to_string(), big_env()], &["/bin"]);
    let got = spawn_userland(&root, &p, &maps, argv, "userland", None, &mut std::io::sink());
    let _ = fs::remove_dir_all(&d);
    match got {
        Err(e @ Error::CannotInvoke(_)) => {
            assert_eq!(e.payload_code(), Some(CANNOT_INVOKE));
            assert!(e.to_string().contains("E2BIG (7)"), "{e}");
        }
        Err(e) => panic!("not 126: {e} (code {:?})", e.payload_code()),
        Ok(c) => panic!("the payload started, exit {:?}", c.wait()),
    }
}
