//! The volumes of a plan on the namespace rung (T-738), as root and as a
//! caller with no capabilities (uid 65534 where the test runs as root,
//! else the user of the test). Each run starts the test binary again over
//! a tmpfs with `nosuid`, `nodev`, `noexec` and `noatime`
//! (`nobody::again_over_a_flagged_tmpfs`), which holds the host side of
//! each volume: a read-only volume must keep each of those flags, and a
//! user namespace locks them.

use super::*;
use pod_confine::{Access, Spec};
use pod_enter::{NS_VOLUME_LINK_STEP, VolumeBind, ns_setup_step, volume_label};

/// The tmpfs of the host sides, set by the parent.
const VOLS: &str = "POD_ENTER_USERNS_VOLUMES";

/// The flags of the tmpfs that a read-only volume keeps.
const FLAGS: [&str; 4] = ["nosuid", "nodev", "noexec", "noatime"];

/// As root: the test again as root, then as uid 65534, each over its own
/// image and tmpfs. As another user: the test again as this user. The
/// test again runs `body`.
pub fn as_root_and_nobody(test: &str, body: fn(&Path, &Path)) {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    if let (Some(root), Some(vols)) = (std::env::var_os(IMAGE), std::env::var_os(VOLS)) {
        return body(Path::new(&root), Path::new(&vols));
    }
    let bb = match busybox() {
        Ok(bb) => bb,
        Err(why) => return skip_line(&format!("no static busybox: {why}")),
    };
    let runs: &[bool] = if nobody::is_root() { &[true, false] } else { &[false] };
    for &as_root in runs {
        let tag = if as_root { "vol-root" } else { "vol-user" };
        let root = image(&bb, tag);
        image_of_the_volumes(&root, nobody::is_root() && !as_root);
        let vols = std::env::temp_dir().join(format!("pod-enter-userns-{tag}-host-{}", std::process::id()));
        fs::create_dir_all(&vols).unwrap();
        fs::set_permissions(&vols, fs::Permissions::from_mode(0o755)).unwrap();
        let env = [(IMAGE, root.to_str().unwrap()), (VOLS, vols.to_str().unwrap())];
        let args = [test, "--exact", "--nocapture", "--test-threads=1"];
        let code = nobody::again_over_a_flagged_tmpfs(&vols, as_root, &args, &env);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir(&vols);
        match code {
            Ok(code) => assert_eq!(code, 0, "the test of the volumes failed ({tag})"),
            Err(why) => return skip_line(&why),
        }
    }
}

/// The image: `/data` and `/ro` as mount points, an `/etc` with no
/// `volume.conf`, no `/new`, and `/lnk`, a link to `/tmp`. For uid 65534,
/// the root and `/etc` belong to it: the child makes paths there.
fn image_of_the_volumes(root: &Path, for_nobody: bool) {
    for d in ["data", "ro", "etc"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    std::os::unix::fs::symlink("/tmp", root.join("lnk")).unwrap();
    if for_nobody {
        for d in [root.to_path_buf(), root.join("etc")] {
            std::os::unix::fs::chown(&d, Some(nobody::NOBODY), Some(nobody::NOBODY)).unwrap();
        }
    }
}

fn bind(host: &Path, guest: &str, read_only: bool) -> VolumeBind {
    VolumeBind {
        host: host.to_str().unwrap().to_string(),
        guest: guest.to_string(),
        read_only,
        host_base: None,
        overlay: None,
    }
}

/// The test in the process over the tmpfs: a volume that the payload reads
/// and writes, a read-only one, one on a path that the image does not hold,
/// a file; then a guest path through a link of the image, and a host path
/// that does not exist.
pub fn body(root: &Path, vols: &Path) {
    let who = if nobody::is_root() { "root" } else { "a caller with no capabilities" };
    if !nobody::is_root() && !no_capabilities() {
        return;
    }
    let tmpfs = nobody::mount_options(vols.to_str().unwrap());
    assert!(FLAGS.iter().all(|f| tmpfs.split(',').any(|o| o == *f)), "the tmpfs of the host sides: {tmpfs}");
    let (rw, ro, made, file) = (vols.join("rw"), vols.join("ro"), vols.join("made"), vols.join("file"));
    for d in [&rw, &ro, &made] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(rw.join("in"), "in").unwrap();
    fs::write(ro.join("keep"), "keep").unwrap();
    fs::write(&file, "conf").unwrap();
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let script = "test \"$(cat /data/in)\" = in || exit 3; echo out > /data/out || exit 4; \
         touch /ro/new 2>/data/ro-err && exit 5; grep -q 'Read-only file system' /data/ro-err || exit 6; \
         test \"$(cat /ro/keep)\" = keep || exit 7; grep ' /ro ' /proc/self/mountinfo > /data/ro-line || exit 8; \
         test \"$(cat /etc/volume.conf)\" = conf || exit 9; echo m > /new/deep/dir/m || exit 10";
    let mut p = plan(script, None);
    // The read-only volume opens under its base with no link, and binds by
    // its descriptor (`host_base`, PodUtils/podrunner#277).
    let beneath = VolumeBind { host_base: Some(vols.to_str().unwrap().to_string()), ..bind(&ro, "/ro", true) };
    p.volumes = vec![
        bind(&rw, "/data", false),
        beneath,
        bind(&made, "/new/deep/dir", false),
        bind(&file, "/etc/volume.conf", false),
    ];
    let mut err = Vec::new();
    let code = run_namespace(&dir, &p, &mut err);
    let err = String::from_utf8_lossy(&err);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the payload with the volumes as {who}: {err}");
    assert!(!err.contains("entered chroot instead"), "{err}");
    assert_eq!(fs::read_to_string(rw.join("out")).unwrap(), "out\n", "the write in the volume");
    assert_eq!(fs::read_to_string(made.join("m")).unwrap(), "m\n", "the write in the made path");
    assert!(!ro.join("new").exists(), "the write in the read-only volume");
    assert_eq!(fs::read(root.join("etc/volume.conf")).unwrap(), b"", "the mount point of the file in the image");
    assert_eq!(fs::read_dir(root.join("new/deep/dir")).unwrap().count(), 0, "the made path of the image is empty");
    let line = fs::read_to_string(rw.join("ro-line")).unwrap();
    let options = line.lines().find(|l| l.split(' ').nth(4) == Some("/ro")).and_then(|l| l.split(' ').nth(5));
    let options: Vec<&str> = options.unwrap_or_default().split(',').collect();
    assert!(options.contains(&"ro"), "the read-only volume is not ro: {line}");
    for f in FLAGS {
        assert!(options.contains(&f), "the read-only volume lost the flag {f} of its mount: {line}");
    }

    // A guest path through a link of the image: refused before the exec,
    // with the step. A run that binds it (exit 3) or skips it (exit 4)
    // fails here.
    let mut p = plan("test -d /lnk/x && exit 3; exit 4", None);
    p.volumes = vec![bind(&rw, "/lnk/x", false)];
    let mut err = Vec::new();
    let linked = run_namespace(&dir, &p, &mut err).map_err(|e| e.to_string());
    let words = ns_setup_step(NS_VOLUME_LINK_STEP).unwrap();
    let label = volume_label(&p.volumes[0]);
    assert!(
        linked.as_ref().is_err_and(|e| e.contains(words) && e.contains(&label)),
        "the run with a volume through a link: {linked:?}"
    );
    assert!(!root.join("tmp/x").exists(), "the walk made a path through the link");

    // A host path that does not exist: refused before the banner, so
    // before the fork.
    let mut p = plan("exit 0", None);
    p.volumes = vec![bind(&vols.join("missing"), "/data", false)];
    let mut err = Vec::new();
    let missing = run_namespace(&dir, &p, &mut err).map_err(|e| e.to_string());
    let err = String::from_utf8_lossy(&err);
    assert!(missing.as_ref().is_err_and(|e| e.contains("does not exist")), "a missing host path: {missing:?}");
    assert!(!err.contains("the banner"), "the refusal came after the banner: {err}");
}

/// The confinement with volumes (T-739). Before the fork, the path of each
/// rule resolves through the volumes; the child opens it after the binds,
/// so the rule holds the volume and not the mount point of the image.
/// `/cv` is a volume that the image does not hold, with a read rule, and
/// `/cv/w` is a directory of its host side alone, with a write rule.
/// `/data` is a volume on a directory of the image, with no rule. `/up`
/// is above the volume `/up/deep`, and the image holds neither: the child
/// makes it, and its read rule covers the volume.
pub fn confined(root: &Path, vols: &Path) {
    let who = if nobody::is_root() { "root" } else { "a caller with no capabilities" };
    if !nobody::is_root() && !no_capabilities() {
        return;
    }
    if let Err(e) = pod_confine::check() {
        return skip_line(&format!("no Landlock for the confinement: {e}"));
    }
    let (rw, cv, up) = (vols.join("rw"), vols.join("cv"), vols.join("up"));
    for d in [&rw, &cv.join("w"), &up] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(rw.join("in"), "in").unwrap();
    fs::write(cv.join("in"), "cv").unwrap();
    fs::write(up.join("in"), "up").unwrap();
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let volumes = vec![bind(&rw, "/data", false), bind(&cv, "/cv", false), bind(&up, "/up/deep", false)];
    let spec = || {
        Spec::new()
            .allow("/bin", Access::READ | Access::EXECUTE)
            .allow("/cv", Access::READ)
            .allow("/cv/w", Access::READ | Access::WRITE)
            .allow("/up", Access::READ)
    };
    // Exit codes of their own: a plant names the step that it breaks.
    let script = "test \"$(cat /cv/in)\" = cv || exit 21; cat /data/in && exit 22; \
         echo w > /cv/w/out || exit 23; echo n > /cv/no && exit 24; test \"$(cat /up/deep/in)\" = up || exit 25; \
         exit 0";
    let mut p = plan(script, None);
    p.volumes = volumes.clone();
    p.confine = Some(spec());
    let mut err = Vec::new();
    let code = run_namespace(&dir, &p, &mut err);
    let err = String::from_utf8_lossy(&err);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the confined payload with the volumes as {who}: {err}");
    assert_eq!(fs::read_to_string(cv.join("w/out")).unwrap(), "w\n", "the write under the write rule");
    assert!(!cv.join("no").exists(), "a write outside the rules landed in the volume");

    // A rule on a path that neither the image nor a volume holds: refused
    // by name before the banner, so before the fork.
    let mut p = plan("exit 0", None);
    p.volumes = volumes;
    p.confine = Some(spec().allow("/nowhere", Access::READ));
    let mut err = Vec::new();
    let refused = run_namespace(&dir, &p, &mut err).map_err(|e| e.to_string());
    let err = String::from_utf8_lossy(&err);
    assert!(
        refused.as_ref().is_err_and(|e| e.contains("\"/nowhere\"") && e.contains("does not exist")),
        "a rule on a path in no volume and not in the image: {refused:?}"
    );
    assert!(!err.contains("the banner"), "the refusal of the rule came after the banner: {err}");
}
