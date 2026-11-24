//! The modes of the tree against the umask of the process, and the
//! permission pass of the directories (T-597).
//!
//! The umask belongs to the whole process, and libtest runs the tests on
//! threads of one process: the test binary starts itself again, and the
//! child sets the umask.

use super::*;

/// The variable that makes the worker below run.
const WORKER: &str = "POD_EXTRACT_TEST_UMASK_WORKER";

/// Each path of the layer below, and the mode of the image.
const MODES: [(&str, u32); 5] = [("d", 0o755), ("d/x", 0o755), ("d/y", 0o644), ("g", 0o775), ("g/z", 0o664)];

/// 0755, 0644, 0775 and 0664 stay under a umask of 027, after the last
/// layer and between two layers.
#[test]
fn the_modes_of_the_image_stay_under_a_umask_of_027() {
    in_worker("worker_extracts_under_a_umask_of_027", "027");
}

/// A directory that a file below it made has mode 0755 under a umask of
/// 077 (README rule 6), the mode that `flatten` writes for it.
#[test]
fn a_directory_made_for_a_file_below_it_has_mode_0755_under_a_umask_of_077() {
    in_worker("worker_makes_a_directory_under_a_umask_of_077", "077");
}

/// The half that runs in the child. It does nothing with no [`WORKER`].
#[test]
fn worker_makes_a_directory_under_a_umask_of_077() {
    if std::env::var_os(WORKER).is_none() {
        return;
    }
    pod_hostprobe::sys::umask(0o077);
    let s = Scratch::new("umask-made");
    run(s.path(), &[layer(&[E::File("m/n/f", b"f", 0o644)])]).unwrap();
    for (p, disk, _) in kept(s.path(), &["m", "m/n", "m/n/f"]) {
        let want = if p == "m/n/f" { 0o644 } else { 0o755 };
        assert_eq!(disk, want, "the umask changed the mode of {p}: {disk:04o}, not {want:04o}");
    }
}

/// Run the test `name` of this module in a new process of the test binary,
/// where it sets the umask `umask` (for the text of a failure).
fn in_worker(name: &str, umask: &str) {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &format!("drive::umask::{name}"), "--nocapture"])
        .env(WORKER, "1")
        .output();
    let out = match out {
        Ok(o) => o,
        // A test binary under an emulator with no binfmt registration.
        Err(e) if e.raw_os_error() == Some(8) => {
            pod_testkit::host::skip_line(&format!("this host cannot exec the test binary as a child ({e})"));
            return;
        }
        Err(e) => panic!("the start of the worker failed: {e}"),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "the worker under a umask of {umask} failed:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}

/// The half that runs in the child. It does nothing with no [`WORKER`].
#[test]
fn worker_extracts_under_a_umask_of_027() {
    if std::env::var_os(WORKER).is_none() {
        return;
    }
    pod_hostprobe::sys::umask(0o027);
    let l = layer(&[
        E::Dir("d/", 0o755),
        E::File("d/x", b"x", 0o755),
        E::File("d/y", b"y", 0o644),
        E::Dir("g/", 0o775),
        E::File("g/z", b"z", 0o664),
    ]);

    // The tree between two layers: the pass after the last layer has not
    // run, so the permission pass of the layer alone sets the group and
    // other bits of each directory.
    let open = Scratch::new("umask-open");
    run_open(open.path(), std::slice::from_ref(&l)).unwrap();
    for (p, mode) in [("d", 0o755), ("g", 0o775)] {
        let disk = kept(open.path(), &[p])[0].1;
        let want = mode | 0o700;
        assert_eq!(disk, want, "the umask changed the mode of {p}: {disk:04o}, not {want:04o}, between two layers");
    }

    let s = Scratch::new("umask");
    run(s.path(), &[l]).unwrap();
    let paths: Vec<&str> = MODES.iter().map(|(p, _)| *p).collect();
    for ((p, disk, applied), (_, mode)) in kept(s.path(), &paths).iter().zip(MODES) {
        assert_eq!(*disk, mode, "the umask changed the mode of {p}: {disk:04o}, not {mode:04o}");
        assert_eq!(*applied, None, "{p}: the sidecar claims a mode that is not on disk");
    }
}

/// A directory that an earlier layer made gets the mode of the later layer
/// that names it again.
#[test]
fn a_later_layer_sets_the_mode_of_a_directory_again() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("redir");
    run(s.path(), &[layer(&[E::Dir("d/", 0o700)]), layer(&[E::Dir("d/", 0o755)])]).unwrap();
    let m = std::fs::metadata(s.path().join("d")).unwrap().permissions().mode() & 0o7777;
    assert_eq!(m, 0o755, "the mode of the later layer was not set: {m:04o}");
}

/// A symlink that a later entry of the layer puts at the name of a
/// directory is not followed by the permission pass: its target is a file
/// of the host.
#[test]
fn the_permission_pass_does_not_follow_a_symlink_at_the_name_of_a_directory() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("passlink");
    let victim = s.path().join("victim");
    std::fs::write(&victim, b"host").unwrap();
    std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o600)).unwrap();
    let dest = s.path().join("root");
    std::fs::create_dir(&dest).unwrap();
    let target = victim.to_string_lossy().into_owned();
    // The absolute target is inside the root after the rebase, so the layer
    // applies; the host resolves the same text to the victim.
    run(&dest, &[layer(&[E::Dir("d/", 0o555), E::Symlink("d", &target)])]).unwrap();
    assert!(std::fs::symlink_metadata(dest.join("d")).unwrap().file_type().is_symlink());
    let m = std::fs::metadata(&victim).unwrap().permissions().mode() & 0o7777;
    assert_eq!(m, 0o600, "the permission pass followed the symlink to a file of the host: {m:04o}");
}
