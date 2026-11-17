//! A pty in a unit of a caller with no capabilities (PodUtils/podvm#469):
//! the namespace rung mounts a new devpts instance on the `/dev/pts` of the
//! image and binds its `ptmx` on `/dev/ptmx`. The image has a `/dev/pts`
//! directory and no `/dev/ptmx`, as `pod-rootfs` leaves it in a user
//! namespace.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use pod_enter::{DEVPTS_FLAGS, Names, RootDir, devpts_refused, run_namespace};
use pod_hostprobe::sys;
use pod_testkit::nobody;

/// The `/dev` of [`a_pty`], of the caller of the test: the child makes the
/// mount point of `/dev/ptmx` in it. Also the `stat` applet.
pub fn of_the_image(root: &Path) {
    std::os::unix::fs::symlink("busybox", root.join("bin/stat")).unwrap();
    let dev = root.join("dev");
    fs::create_dir_all(dev.join("pts")).unwrap();
    if nobody::is_root() {
        std::os::unix::fs::chown(&dev, Some(nobody::NOBODY), Some(nobody::NOBODY)).unwrap();
    }
}

/// `/dev/ptmx` is a character device that opens, `/dev/pts` is not the
/// host's devpts, its `ptmx` has mode 0666, the first pty of the unit is
/// `/dev/pts/0` and the only one, and the bind of `/dev/ptmx` is `nosuid`
/// and `noexec`. The payload gets no line of a refusal.
pub fn a_pty(root: &Path) {
    if !super::no_capabilities() {
        return;
    }
    let host = fs::metadata("/dev/pts").map(|m| m.dev()).unwrap_or(0);
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let script = format!(
        "test -c /dev/ptmx || exit 41; test \"$(stat -c %d /dev/pts)\" != {host} || exit 42; \
         test \"$(stat -c %a /dev/pts/ptmx)\" = 666 || exit 43; \
         out=$({{ ls /dev/pts; }} 3<>/dev/ptmx) || exit 44; test \"$out\" = \"$(printf '0\\nptmx')\" || exit 45; \
         grep -Eq ' /dev/ptmx [^ ]*nosuid[^ ]*noexec' /proc/self/mountinfo || exit 46"
    );
    // The child writes the line of a refusal to the standard error of the
    // payload: a file here.
    let lines = std::env::temp_dir().join(format!("pod-enter-userns-pts-err-{}", std::process::id()));
    let file = fs::File::create(&lines).unwrap();
    let mut plan = super::plan(&script, None);
    plan.fds.pass.push((2, i64::from(std::os::fd::AsRawFd::as_raw_fd(&file))));
    let mut err = Vec::new();
    let code = run_namespace(&dir, &plan, &mut err);
    let (err, payload) = (String::from_utf8_lossy(&err), fs::read_to_string(&lines).unwrap_or_default());
    let _ = fs::remove_file(&lines);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the payload as uid {}: {err}{payload}", nobody::NOBODY);
    assert!(!err.contains("entered chroot instead"), "{err}");
    let product = Names::DEFAULT.product;
    assert!(!payload.contains(&format!("{product}: the namespace rung")), "no line of a refusal: {payload}");
    assert_eq!(fs::metadata(root.join("dev/ptmx")).map(|m| m.len()).ok(), Some(0), "the mount point of the bind");
}

/// Where the kernel refuses the new devpts instance, the payload gets the
/// line of the refusal, `/dev/pts` is the host's devpts, and `/dev/ptmx` is
/// its `ptmx`, which opens where the host's `ptmxmode` lets each user open
/// it. A seccomp filter on this thread, which the child takes, refuses with
/// `EPERM` each `mount(2)` with the flags of the instance and no other
/// mount: the kernels of CI mount the instance.
pub fn a_refused_instance(root: &Path) {
    if !super::no_capabilities() {
        return;
    }
    refuse_the_instance();
    let pts = fs::metadata("/dev/pts").map(|m| m.dev()).unwrap_or(0);
    let opens = fs::metadata("/dev/pts/ptmx").is_ok_and(|m| m.mode() & 0o006 == 0o006);
    let opens = u8::from(opens);
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let script = format!(
        "test -c /dev/ptmx || exit 41; test \"$(stat -c %d /dev/pts)\" = {pts} || exit 42; \
         test \"$(stat -c %d /dev/ptmx)\" = {pts} || exit 43; \
         if {{ :; }} 3<>/dev/ptmx 2>/dev/null; then test {opens} = 1 || exit 44; else test {opens} = 0 || exit 45; fi"
    );
    let lines = std::env::temp_dir().join(format!("pod-enter-userns-pts-refused-{}", std::process::id()));
    let file = fs::File::create(&lines).unwrap();
    let mut plan = super::plan(&script, None);
    plan.fds.pass.push((2, i64::from(std::os::fd::AsRawFd::as_raw_fd(&file))));
    let mut err = Vec::new();
    let code = run_namespace(&dir, &plan, &mut err);
    let (err, payload) = (String::from_utf8_lossy(&err), fs::read_to_string(&lines).unwrap_or_default());
    let _ = fs::remove_file(&lines);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the payload as uid {}: {err}{payload}", nobody::NOBODY);
    assert!(!err.contains("entered chroot instead"), "{err}");
    let line = format!("{}{}\n", devpts_refused(&Names::DEFAULT), sys::EPERM.0);
    assert!(payload.contains(&line), "the line of the refusal: {payload}");
}

/// A filter on this thread alone: `mount(2)` with the flags of the devpts
/// instance gets `EPERM`. The thread ends with its test.
fn refuse_the_instance() {
    let deny = [pod_confine::Deny::call("mount", sys::SYS_MOUNT, 1).when_equal(3, u32::MAX, DEVPTS_FLAGS as u32)];
    let filter = pod_confine::deny::program(&deny).unwrap();
    sys::set_no_new_privs().unwrap();
    sys::seccomp_set_filter(&filter, 0).unwrap();
}
