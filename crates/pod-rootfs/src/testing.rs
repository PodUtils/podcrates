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
