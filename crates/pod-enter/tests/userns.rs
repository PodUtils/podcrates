//! The namespace rung for a caller with no capabilities
//! (PodUtils/podvm#406): the entry makes a user namespace that maps the
//! caller to 0, then its mount and UTS namespaces, and does not fall back
//! to chroot. The payload sees uid 0, the name of the plan, a `/tmp` that
//! is not the `/tmp` of the image, and a `/proc` (a bind of the host's,
//! T-672). An exec that joins the run sees the `/tmp` of the run (T-671).
//! The device nodes of the host replace the regular files of the image
//! (T-688). The volumes of a plan are bound as root and as the caller with
//! no capabilities (T-738, `userns/volumes.rs`), and a rule of the
//! confinement on a volume holds on the volume (T-739). On an overlay root
//! with an upper directory, the checks before the fork read the upper
//! layer over the image (T-750, `userns/upper.rs`). With the ids inside
//! of the plan, the payload is a user of its image with no capability
//! (T-786, `userns/ids.rs`); a caller that is uid 0 of a user namespace
//! gets them through a nested one (T-787, `userns/nested.rs`). With a PID
//! namespace, the payload sees the processes of its run alone (T-806,
//! `userns/pid.rs`). The unit gets a devpts instance of its own and a
//! `/dev/ptmx` that opens (PodUtils/podvm#469, `userns/pts.rs`). A tmpfs
//! of the plan holds its byte limit, and a join finds it (PodUtils/podvm#667,
//! `userns/tmpfs.rs`).
//!
//! As root, the test makes the image and starts its own binary again as
//! uid and gid 65534 with no groups (`pod_testkit::nobody`). The test of
//! the run starts again also as another user, in namespaces of its own
//! over a fresh proc with no `nosuid`, `nodev` or `noexec`: the check of
//! the remount of the bind then does not depend on the host's proc. As
//! root, the test of the devices starts again in a private mount namespace
//! whose `/dev` has no `nosuid` and no `noexec`, so that its check of the
//! remount of each bind does not depend on the host's `/dev`; as another
//! user over such a `/dev`, it says that the check is not seen here
//! (`scripts/ci/userns-tests.sh` runs it again with root). The image is
//! a static `busybox` of the host (`PODVM_T486_BUSYBOX`, or `/bin/busybox`
//! where it is static). Where the host has none, where the kernel gives
//! such a caller no user namespace that takes a mount, or where uid 65534
//! cannot reach the binary, the test says `skipping` and passes:
//! `scripts/ci/userns-tests.sh` fails on that line.
#![cfg(target_os = "linux")]

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use pod_enter::{Fds, Names, Plan, RootDir, run_namespace, spawn_joined, spawn_namespace};
use pod_hostprobe::sys;
use pod_testkit::host::skip_line;
use pod_testkit::nobody;

#[path = "userns/ids.rs"]
mod ids;
#[path = "userns/nested.rs"]
mod nested;
#[path = "userns/pid.rs"]
mod pid;
#[path = "userns/pts.rs"]
mod pts;
#[path = "userns/tmpfs.rs"]
mod tmpfs;
#[path = "userns/upper.rs"]
mod upper;
#[path = "userns/volumes.rs"]
mod volumes;

const NAME: &str = "unit2";
/// The image that the root parent made for the child of uid 65534.
const IMAGE: &str = "POD_ENTER_USERNS_IMAGE";
/// Set where the root parent gave the child a `/dev` with no `nosuid` and
/// no `noexec`.
const BARE_DEV: &str = "POD_ENTER_USERNS_BARE_DEV";

/// One test at a time: a test that forks while another test writes its
/// copy of busybox gives the child that write descriptor until the exec,
/// and the exec of that copy then fails with `ETXTBSY`.
static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn busybox() -> Result<PathBuf, String> {
    let p = std::env::var_os("PODVM_T486_BUSYBOX").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/bin/busybox"));
    let mut head = [0u8; 4096];
    let n = fs::File::open(&p).and_then(|mut f| f.read(&mut head)).map_err(|e| format!("{}: {e}", p.display()))?;
    if [&b"ld-linux"[..], b"ld-musl"].iter().any(|ld| head[..n].windows(ld.len()).any(|w| w == *ld)) {
        return Err(format!("{} is not static", p.display()));
    }
    Ok(p)
}

/// An image with `/bin/busybox` and its applets, `/tmp` with one file of
/// the image, and an empty `/proc`.
fn image(bb: &Path, tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pod-enter-userns-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for d in ["bin", "tmp", "proc"] {
        fs::create_dir_all(root.join(d)).unwrap();
        fs::set_permissions(root.join(d), fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(root.join("tmp/of-the-image"), "x").unwrap();
    // A working directory of the image under /tmp, which the tmpfs of the
    // rung hides (PodUtils/podvm#569).
    fs::create_dir_all(root.join("tmp/wd/deep")).unwrap();
    fs::copy(bb, root.join("bin/busybox")).unwrap();
    fs::set_permissions(root.join("bin/busybox"), fs::Permissions::from_mode(0o755)).unwrap();
    for applet in ["sh", "id", "uname", "test", "touch", "grep", "sleep", "head", "wc", "cat", "ls", "tr"] {
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
        banner: "the banner\n".to_string(),
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

/// As root: the image, then the test `test` again as uid 65534. As
/// another user: the test as this user.
fn as_nobody(test: &str, tag: &str, body: fn(&Path)) {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(root) = std::env::var_os(IMAGE) {
        return body(Path::new(&root));
    }
    let bb = match busybox() {
        Ok(bb) => bb,
        Err(why) => return skip_line(&format!("no static busybox: {why}")),
    };
    let root = image(&bb, tag);
    if tag == "dev" {
        dev_of_the_image(&root);
    }
    if tag == "pts" {
        pts::of_the_image(&root);
    }
    if tag == "ids" || tag == "nested" {
        ids::of_the_image(&root);
    }
    if tag == "nested" && nobody::is_root() {
        nested::host_root(&root);
    }
    // The run checks the flags of the bind of /proc: it gets a proc with
    // none, so that a host whose proc has them already does not hide a
    // missing remount (T-689). The nested test starts again as uid 0 of a
    // user namespace also as another user.
    let bare = tag == "run" || tag == "nested";
    if !nobody::is_root() && !bare {
        body(&root);
        let _ = fs::remove_dir_all(&root);
        return;
    }
    let env = [(IMAGE, root.to_str().unwrap()), (BARE_DEV, "1")];
    let args = [test, "--exact", "--nocapture", "--test-threads=1"];
    // As root, the devices get a /dev with no nosuid and no noexec, so that
    // a host whose /dev has them does not hide a missing remount (T-736).
    let code = match tag {
        "run" => nobody::again_over_a_bare_proc(&args, &env[..1]),
        "nested" => nobody::again_as_root_of_a_user_namespace(&args, &env[..1]),
        "dev" => nobody::again_over_a_bare_dev(&args, &env),
        _ => nobody::again(&args, &env[..1]),
    };
    let _ = fs::remove_dir_all(&root);
    match code {
        Ok(code) => assert_eq!(code, 0, "the test as a caller with no capabilities failed"),
        Err(why) => skip_line(&why),
    }
}

/// The caller holds no capability and gets a user namespace for the rung;
/// else a skip line.
fn no_capabilities() -> bool {
    assert_eq!(nobody::effective_caps(), Some(0), "the caller must hold no capability");
    if let Err(why) = pod_testkit::host::userns() {
        skip_line(&format!("the kernel gives this caller no user namespace for the rung: {why}"));
        return false;
    }
    true
}

#[test]
fn a_caller_with_no_capabilities_enters_the_namespace_rung() {
    as_nobody("a_caller_with_no_capabilities_enters_the_namespace_rung", "run", as_the_caller);
}

#[test]
fn a_caller_with_no_capabilities_gets_the_devices_of_the_host() {
    as_nobody("a_caller_with_no_capabilities_gets_the_devices_of_the_host", "dev", devices_of_the_host);
}

#[test]
fn a_caller_with_no_capabilities_gets_a_pty() {
    as_nobody("a_caller_with_no_capabilities_gets_a_pty", "pts", pts::a_pty);
}

#[test]
fn a_refused_devpts_instance_falls_back_to_the_host_s_pts() {
    as_nobody("a_refused_devpts_instance_falls_back_to_the_host_s_pts", "pts", pts::a_refused_instance);
}

#[test]
fn the_namespace_rung_binds_the_volumes_of_a_plan() {
    volumes::as_root_and_nobody("the_namespace_rung_binds_the_volumes_of_a_plan", volumes::body);
}

#[test]
fn a_rule_of_the_confinement_holds_on_a_volume() {
    volumes::as_root_and_nobody("a_rule_of_the_confinement_holds_on_a_volume", volumes::confined);
}

#[test]
fn a_tmpfs_of_the_plan_holds_its_byte_limit() {
    volumes::as_root_and_nobody("a_tmpfs_of_the_plan_holds_its_byte_limit", tmpfs::limited);
}

#[test]
fn a_join_finds_the_tmpfs_of_its_run_with_its_limit() {
    volumes::as_root_and_nobody("a_join_finds_the_tmpfs_of_its_run_with_its_limit", tmpfs::joined);
}

#[test]
fn the_checks_read_the_upper_layer_of_an_overlay_root() {
    volumes::as_root_and_nobody("the_checks_read_the_upper_layer_of_an_overlay_root", upper::layered);
}

#[test]
fn the_payload_runs_as_the_ids_inside_of_the_plan() {
    as_nobody("the_payload_runs_as_the_ids_inside_of_the_plan", "ids", ids::inside);
}

#[test]
fn uid_0_of_a_user_namespace_gets_the_ids_of_the_plan() {
    as_nobody("uid_0_of_a_user_namespace_gets_the_ids_of_the_plan", "nested", nested::root_of_a_user_namespace);
}

#[test]
fn the_pid_namespace_hides_the_processes_of_the_host() {
    pid::as_root_then_nobody("the_pid_namespace_hides_the_processes_of_the_host", "pid", pid::hides_the_host);
}

#[test]
fn the_payload_ends_with_the_init_of_its_pid_namespace() {
    pid::as_root_then_nobody("the_payload_ends_with_the_init_of_its_pid_namespace", "pid-end", pid::ends_with_its_init);
}

#[test]
fn a_join_lands_in_the_pid_namespace_of_its_run() {
    pid::as_root_then_nobody("a_join_lands_in_the_pid_namespace_of_its_run", "pid-join", pid::a_join_lands_in_it);
}

#[test]
fn the_init_of_a_pid_namespace_keeps_no_descriptor_of_the_caller() {
    pid::as_root_then_nobody(
        "the_init_of_a_pid_namespace_keeps_no_descriptor_of_the_caller",
        "pid-fds",
        pid::keeps_no_descriptor,
    );
}

#[test]
fn the_init_and_the_relay_show_a_title_and_no_environment() {
    pid::as_root_then_nobody("the_init_and_the_relay_show_a_title_and_no_environment", "pid-title", pid::shows_a_title);
}

#[test]
fn a_caller_with_no_capabilities_joins_its_run() {
    as_nobody("a_caller_with_no_capabilities_joins_its_run", "join", joins_its_run);
}

/// The test as the caller with no capabilities.
fn as_the_caller(root: &Path) {
    if !no_capabilities() {
        return;
    }
    let host = nobody::mount_options("/proc");
    assert!(["nosuid", "nodev", "noexec"].iter().all(|f| !host.split(',').any(|o| o == *f)), "a bare proc: {host}");
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let active = format!("${}", Names::DEFAULT.mode_active);
    let script = format!(
        "test \"$(id -u)\" = 0 || exit 3; test \"$(uname -n)\" = {NAME} || exit 4; \
         test -e /tmp/of-the-image && exit 5; touch /tmp/new || exit 6; \
         test \"{active}\" = namespace || exit 7; test -r /proc/self/status || exit 8; \
         grep -q ' /proc [a-z]*,nosuid,nodev,noexec' /proc/self/mountinfo || exit 9"
    );
    let mut err = Vec::new();
    let child = spawn_namespace(&dir, &plan(&script, Some(NAME)), &mut err);
    // The rung that the entry entered, which the caller records.
    let entered = child.as_ref().ok().and_then(|c| c.entered);
    let code = child.and_then(|c| c.wait_forwarding(&mut err));
    let err = String::from_utf8_lossy(&err);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the payload as uid {}: {err}", nobody::NOBODY);
    assert_eq!(entered, Some(pod_hostprobe::select::Rung::Namespace), "{err}");
    assert_eq!(err.matches("the banner").count(), 1, "the banner goes out one time: {err}");
    assert!(!err.contains("entered chroot instead"), "{err}");
    assert!(!err.contains("runs with no /proc"), "{err}");
    assert!(!root.join("tmp/new").exists(), "the write of the payload landed in the /tmp of the image");
    // A working directory under /tmp: the child makes it in the tmpfs.
    let mut p = plan("test \"$(pwd)\" = /tmp/wd/deep || exit 3; touch made || exit 4", Some(NAME));
    p.working_dir = "/tmp/wd/deep".to_string();
    let mut err = Vec::new();
    let code = run_namespace(&dir, &p, &mut err);
    let err = String::from_utf8_lossy(&err);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "a working directory under /tmp: {err}");
    assert!(!root.join("tmp/wd/deep/made").exists(), "the write landed in the image");
}

/// The `/dev` of [`devices_of_the_host`], of the caller of the test: the
/// child makes the missing mount points in it.
fn dev_of_the_image(root: &Path) {
    let dev = root.join("dev");
    fs::create_dir_all(&dev).unwrap();
    fs::write(dev.join("null"), "").unwrap();
    fs::write(dev.join("random"), vec![0u8; 1 << 20]).unwrap();
    fs::write(dev.join("urandom"), vec![0u8; 1 << 20]).unwrap();
    fs::write(root.join("zero-of-the-image"), "z").unwrap();
    std::os::unix::fs::symlink("../zero-of-the-image", dev.join("zero")).unwrap();
    if nobody::is_root() {
        std::os::unix::fs::chown(&dev, Some(nobody::NOBODY), Some(nobody::NOBODY)).unwrap();
    }
}

/// The `/dev` that `pod-rootfs` leaves in a user namespace: regular files
/// (a `/dev/urandom` of 1 MiB), a missing `/dev/full` and `/dev/tty`, and
/// a link at `/dev/zero`. The payload reads more than 1 MiB of
/// `/dev/urandom`, `/dev/null` is a character device, a write to
/// `/dev/full` fails with `ENOSPC`, each bind is `nosuid` and `noexec` but
/// not `nodev`, and the link stays a link.
fn devices_of_the_host(root: &Path) {
    if !no_capabilities() {
        return;
    }
    // A bind keeps the flags of the host's /dev: with them, the check of
    // the remount (exit 8) passes with no remount.
    let host = nobody::mount_options("/dev");
    let flagged = host.split(',').any(|o| o == "nosuid" || o == "noexec");
    if std::env::var_os(BARE_DEV).is_some() {
        assert!(!flagged, "a /dev with no nosuid and no noexec: {host}");
    } else if flagged {
        // Not captured, as `skip_line`, so that the line shows for a pass.
        use std::io::Write as _;
        let _ = writeln!(
            std::io::stderr(),
            "not seen here: the check of the remount of a device: the host's /dev has {host}"
        );
    }
    let dev = root.join("dev");
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let script = "test \"$(head -c 2000000 /dev/urandom | wc -c)\" = 2000000 || exit 3; \
         test \"$(head -c 2000000 /dev/random | wc -c)\" = 2000000 || exit 4; test -c /dev/null || exit 5; \
         out=$(echo x 2>&1 >/dev/full); case \"$out\" in *'No space left'*) ;; *) exit 6;; esac; \
         test -L /dev/zero || exit 7; \
         grep -Eq ' /dev/urandom [^ ]*nosuid[^ ]*noexec' /proc/self/mountinfo || exit 8; \
         grep -Eq ' /dev/(null|urandom) [^ ]*nodev' /proc/self/mountinfo && exit 9; test -c /dev/tty || exit 10";
    // The child writes the line of a skipped path to the standard error
    // of the payload: a file here.
    let lines = std::env::temp_dir().join(format!("pod-enter-userns-dev-err-{}", std::process::id()));
    let file = fs::File::create(&lines).unwrap();
    let mut plan = plan(script, None);
    plan.fds.pass.push((2, i64::from(std::os::fd::AsRawFd::as_raw_fd(&file))));
    let mut err = Vec::new();
    let code = run_namespace(&dir, &plan, &mut err);
    let (err, payload) = (String::from_utf8_lossy(&err), fs::read_to_string(&lines).unwrap_or_default());
    let _ = fs::remove_file(&lines);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the payload as uid {}: {err}{payload}", nobody::NOBODY);
    assert!(!err.contains("entered chroot instead"), "{err}");
    assert!(payload.contains("keeps the image's /dev/zero: the image holds a symbolic link"), "{payload}");
    assert_eq!(payload.matches("keeps the image's").count(), 1, "one line, for the link: {payload}");
    assert_eq!(fs::read(dev.join("urandom")).unwrap().len(), 1 << 20, "the bind wrote into the image");
}

/// A run that writes `/tmp/shared` and sleeps, then an exec that joins it
/// as the same caller: the exec sees the file of the run and uid 0, and
/// does not fall back to a mount namespace of its own.
fn joins_its_run(root: &Path) {
    if !no_capabilities() {
        return;
    }
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let mut err = Vec::new();
    let run = spawn_namespace(&dir, &plan("touch /tmp/shared && exec sleep 60", None), &mut err).unwrap();
    // The exec of `sleep` comes after the write of the file.
    let comm = format!("/proc/{}/comm", run.pid);
    for _ in 0..200 {
        if fs::read_to_string(&comm).is_ok_and(|c| c.trim_end() == "sleep") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let pidfd = sys::pidfd_open(run.pid).unwrap();
    let active = format!("${}", Names::DEFAULT.mode_active);
    let script = format!(
        "test -e /tmp/shared || exit 3; test \"$(id -u)\" = 0 || exit 4; test \"{active}\" = namespace || exit 5"
    );
    let mut joined = Vec::new();
    let code = spawn_joined(&dir, &plan(&script, None), pidfd, &mut joined).and_then(|c| c.wait());
    let _ = sys::close(pidfd);
    let _ = sys::kill(run.pid, 9);
    let _ = run.wait();
    let (err, joined) = (String::from_utf8_lossy(&err), String::from_utf8_lossy(&joined));
    assert!(!err.contains("entered chroot instead"), "the run fell back: {err}");
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the joined exec as uid {}: {joined}", nobody::NOBODY);
    assert!(!joined.contains("does not share the mounts"), "{joined}");
}
