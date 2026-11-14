//! The set-user-ID and set-group-ID bits of a file (PodUtils/podvm#342).
//!
//! A `write(2)` by a process with no `CAP_FSETID` clears `S_ISUID` and
//! `S_ISGID`. Root keeps them, so the proof runs again as a user: the test
//! binary starts itself as uid 1000.

use super::*;

/// The variable that makes the worker below run. It names a directory of
/// mode 2777 and group 0 that the parent made.
const SHARED: &str = "POD_EXTRACT_TEST_SETID_SHARED";

/// The three modes of the report: `su` (4755), `wall` (2755), and both.
fn set_id_layer() -> Vec<u8> {
    layer(&[
        E::Dir("bin/", 0o755),
        E::File("bin/su", b"su", 0o4755),
        E::File("bin/wall", b"wall", 0o2755),
        E::File("bin/both", b"both", 0o6755),
    ])
}

/// Each file keeps the mode of the image, and its row claims no other.
fn assert_exact(dir: &std::path::Path, who: &str) {
    let got = kept(dir, &["bin/su", "bin/wall", "bin/both"]);
    let want = [("bin/su", 0o4755), ("bin/wall", 0o2755), ("bin/both", 0o6755)];
    for ((p, disk, applied), (_, mode)) in got.iter().zip(want) {
        assert_eq!(*disk, mode, "{p} lost its set-id bits as {who}: {disk:04o}, not {mode:04o}");
        assert_eq!(*applied, None, "{p} as {who}: the sidecar claims a mode that is not on disk");
    }
}

/// 4755, 2755 and 6755 stay on disk, as the user of the test and, when the
/// test runs as root, as uid 1000 too.
#[test]
fn set_id_bits_survive_the_write() {
    let s = Scratch::new("setid");
    run(s.path(), &[set_id_layer()]).unwrap();
    assert_exact(s.path(), "the user of the test");
    if Ids::current().uid != 0 {
        pod_testkit::host::skip_line("the test does not run as root, so it cannot start a child as uid 1000");
        return;
    }
    as_uid_1000();
}

/// The test binary again, as uid 1000 and gid 1000, with a directory of
/// group 0 and `S_ISGID`.
fn as_uid_1000() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let s = Scratch::new("setid-child");
    std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    // A directory of group 0 with S_ISGID: a new file in it has group 0,
    // which uid 1000 is not in.
    let shared = s.path().join("shared");
    std::fs::create_dir(&shared).unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o2777)).unwrap();
    assert_eq!(std::fs::metadata(&shared).unwrap().gid(), 0);
    user::as_uid_1000(&s, "drive::setid::worker_extracts_as_a_user", SHARED, shared.as_os_str());
}

/// The half that runs as uid 1000. It does nothing with no [`SHARED`].
#[test]
fn worker_extracts_as_a_user() {
    let Some(shared) = std::env::var_os(SHARED) else {
        return;
    };
    assert_ne!(Ids::current().uid, 0, "the worker must not run as root: it then proves nothing");
    let s = Scratch::new("setid-user");
    run(s.path(), &[set_id_layer()]).unwrap();
    assert_exact(s.path(), "uid 1000");

    // The kernel drops S_ISGID of a file whose group is not a group of the
    // process, with no error. The sidecar says what stayed.
    let shared = std::path::Path::new(&shared);
    run(shared, &[set_id_layer()]).unwrap();
    let got = kept(shared, &["bin/su", "bin/wall", "bin/both"]);
    let want = [("bin/su", 0o4755, None), ("bin/wall", 0o755, Some("0755")), ("bin/both", 0o4755, Some("4755"))];
    for ((p, disk, applied), (_, mode, row)) in got.iter().zip(want) {
        assert_eq!(*disk, mode, "{p} in a directory of group 0: {disk:04o}, not {mode:04o}");
        assert_eq!(applied.as_deref(), row, "{p}: the sidecar does not record the mode that stayed");
    }
    // A directory of group 0 too: the pass after the last layer cannot set
    // S_ISGID on it, and one more row records the mode that stayed (T-605).
    let sub = shared.join("dirs");
    std::fs::create_dir(&sub).unwrap();
    run(&sub, &[layer(&[E::Dir("sg/", 0o2755), E::Dir("plain/", 0o755)])]).unwrap();
    let got = kept(&sub, &["sg", "plain"]);
    let want = [("sg", 0o755, Some("0755")), ("plain", 0o755, None)];
    for ((p, disk, applied), (_, mode, row)) in got.iter().zip(want) {
        assert_eq!(*disk, mode, "the directory {p} in a directory of group 0: {disk:04o}, not {mode:04o}");
        assert_eq!(applied.as_deref(), row, "the directory {p}: the sidecar does not record the mode that stayed");
    }
}
