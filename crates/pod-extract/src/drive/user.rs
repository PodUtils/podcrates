//! A worker of a test, run again as uid 1000 and gid 1000. Root passes each
//! check of the permissions, so a proof of a mode runs as a user.

use super::*;

/// Run the test `name` of this binary as uid 1000 and gid 1000, with `var`
/// set to `value`, from a link or a copy of the binary in `s`. The caller
/// sets the mode of `s` to 0755. A host that cannot start the child gives a
/// skip line; a worker that fails fails the test.
pub(super) fn as_uid_1000(s: &Scratch, name: &str, var: &str, value: &std::ffi::OsStr) {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    let exe = s.path().join("test-binary");
    let me = std::env::current_exe().unwrap();
    if std::fs::hard_link(&me, &exe).is_err() {
        std::fs::copy(&me, &exe).unwrap();
    }
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = std::process::Command::new(&exe)
        .args(["--exact", name, "--nocapture"])
        .env(var, value)
        .current_dir(s.path())
        .uid(1000)
        .gid(1000)
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) if matches!(e.raw_os_error(), Some(1 | 13 | 8)) => {
            pod_testkit::host::skip_line(&format!("this host cannot run the test binary as uid 1000 ({e})"));
            return;
        }
        Err(e) => panic!("the start of the worker failed: {e}"),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "the worker as uid 1000 failed:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}
