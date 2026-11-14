//! The helpers of the tests: a scratch directory and the guard of a test
//! that needs `openat2(2)`.

use std::path::{Path, PathBuf};

/// A directory of one test, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// A new empty directory, with `etc` in it.
    pub(crate) fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("pod-rootfs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("etc")).unwrap();
        Scratch(p)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// A path in the directory.
    pub(crate) fn at(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    pub(crate) fn root(&self) -> crate::write::Root {
        crate::write::Root::open(&self.0).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// True when the test must stop: no `openat2(2)` under an emulator (QEMU
/// user mode does not translate it), so each path of the root is refused.
/// The test then writes a line `skipping, REASON`. A real kernel with no
/// `openat2` runs the test, and the test fails.
pub(crate) fn no_in_root() -> bool {
    if pod_extract::safety::have_openat2() {
        return false;
    }
    let set = |v: &str| std::env::var_os(v).is_some();
    if pod_hostprobe::interp::detect(&set).is_emulated() {
        pod_testkit::host::skip_line("an emulator runs this test, and it does not translate openat2(2)");
        return true;
    }
    false
}

/// Make a fifo at `path`, with no tool.
pub(crate) fn mkfifo(path: &Path) {
    use pod_hostprobe::sys;
    let c = sys::CBuf::new(path.to_str().unwrap()).unwrap();
    sys::mknodat(sys::AT_FDCWD as i64, &c, sys::S_IFIFO | 0o644, 0).expect("an unprivileged process makes a fifo");
}

/// Run `f` in a thread, and give its value, or `None` when it did not end
/// in 20 seconds: a call that waits for ever fails the test and does not
/// stop it.
pub(crate) fn bounded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(20)).ok()
}

/// Run the test `name` of this binary again as uid 1000 and gid 1000, with
/// `var` set, from a link or a copy of the binary in a directory of mode
/// 0755. Root passes each check of the permissions, so a proof of a mode
/// runs as a user. A host that cannot start the child gives a skip line.
pub(crate) fn as_uid_1000(name: &str, var: &str) {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    let s = Scratch::new(&format!("uid1000-{}", name.replace(':', "-")));
    std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let exe = s.at("test-binary");
    let me = std::env::current_exe().unwrap();
    if std::fs::hard_link(&me, &exe).is_err() {
        std::fs::copy(&me, &exe).unwrap();
    }
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = std::process::Command::new(&exe)
        .args(["--exact", name, "--nocapture"])
        .env(var, "1")
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
