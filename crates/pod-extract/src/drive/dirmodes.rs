//! The modes of the directories after the last layer, and a directory entry
//! over a name that an earlier layer made as another type (T-598,
//! PodUtils/podvm#354).

use super::records::{place, stored};
use super::*;

const TAR: &str = "application/vnd.oci.image.layer.v1.tar";

/// The variable that makes the worker below run.
const WORKER: &str = "POD_EXTRACT_TEST_CLOSED_TREE";

fn mode(p: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
}

/// One stored layer for each list of entries.
fn layers(s: &Scratch, each: &[&[E]]) -> Vec<crate::Layer> {
    each.iter().map(|e| stored(s, &layer(e), TAR)).collect()
}

/// Two layers. The first closes `ro` and `ro/sub` and opens `tmp` for each
/// user; the second writes into `ro` and closes `d`, which the first made
/// 0755.
fn closed_image(s: &Scratch) -> Vec<crate::Layer> {
    layers(
        s,
        &[
            &[
                E::Dir("ro/", 0o555),
                E::File("ro/kid", b"in", 0o444),
                E::Dir("ro/sub/", 0o555),
                E::File("ro/sub/f", b"f", 0o644),
                E::Dir("tmp/", 0o1777),
                E::Dir("d/", 0o755),
                E::File("d/f", b"f", 0o644),
            ],
            &[E::Dir("d/", 0o555), E::File("ro/added", b"two", 0o644)],
        ],
    )
}

const DIGEST: &str = "sha256:5985985985985985985985985985985985985985985985985985985985985985";

/// The modes of [`closed_image`] in the tree.
fn assert_closed(root: &std::path::Path) {
    for (p, want) in [("ro", 0o555), ("ro/sub", 0o555), ("tmp", 0o1777), ("d", 0o555), ("ro/kid", 0o444)] {
        let got = mode(&root.join(p));
        assert_eq!(got, want, "{p} does not have the mode of the image after the last layer: {got:04o}");
    }
    assert_eq!(std::fs::read(root.join("ro/added")).unwrap(), b"two");
}

/// 0555 and 1777 stay after the last layer, and a later layer that names a
/// directory again closes it. Through [`crate::extract`]: the pass is the
/// step after the loop of the layers.
#[test]
fn each_directory_has_the_mode_of_the_image_after_the_last_layer() {
    let s = Scratch::new("closed");
    let p = place(&s, DIGEST);
    let done = crate::extract(&crate::Settings::default(), DIGEST, &closed_image(&s), &p).unwrap();
    assert_closed(&p.rootfs);
    assert_eq!(done.dir_modes, 4, "ro, ro/sub, tmp and d");
}

/// A directory entry over a symlink of an earlier layer makes the directory
/// of the image, and the target of the symlink, outside the tree, does not
/// change.
#[test]
fn a_directory_entry_over_a_symlink_of_an_earlier_layer_is_a_directory() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("dirlink");
    let outside = s.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("keep"), b"host").unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o700)).unwrap();
    let target = outside.to_string_lossy().into_owned();
    let ls = layers(&s, &[&[E::Symlink("evil", &target)], &[E::Dir("evil/", 0o555), E::File("evil/x", b"x", 0o644)]]);
    let p = place(&s, DIGEST);
    crate::extract(&crate::Settings::default(), DIGEST, &ls, &p)
        .unwrap_or_else(|e| panic!("the directory entry over the symlink did not apply: {}", e.chain()));
    let evil = p.rootfs.join("evil");
    assert!(std::fs::symlink_metadata(&evil).unwrap().is_dir(), "the symlink of the earlier layer stayed");
    assert_eq!(mode(&evil), 0o555);
    assert_eq!(std::fs::read(evil.join("x")).unwrap(), b"x");
    assert_eq!(mode(&outside), 0o700, "a mode outside the tree changed");
    let names: Vec<_> = std::fs::read_dir(&outside).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["keep"], "an entry landed outside the tree");
}

/// A whiteout removes a directory, and a later entry makes it again with no
/// directory entry: the mode of the removed directory is not set on it.
#[test]
fn a_directory_that_a_whiteout_removed_does_not_get_its_old_mode() {
    let s = Scratch::new("whmode");
    let one = layer(&[E::Dir("d/", 0o555)]);
    let two = layer(&[E::File(".wh.d", b"", 0o644), E::File("d/x", b"x", 0o644)]);
    run(s.path(), &[one, two]).unwrap();
    let m = mode(&s.path().join("d"));
    assert_eq!(m & 0o200, 0o200, "the whiteout of d did not forget its mode of 0555: {m:04o}");
}

/// A second extraction over a tree that the first closed passes, a
/// whiteout in a closed directory applies, and the tree is removed. Root
/// passes each check of the permissions: as root, the worker runs as uid
/// 1000.
#[test]
fn a_second_extraction_over_a_closed_tree_passes() {
    use std::os::unix::fs::PermissionsExt;
    if Ids::current().uid != 0 {
        closed_tree_as_a_user();
        return;
    }
    let s = Scratch::new("closed-child");
    std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    user::as_uid_1000(&s, "drive::dirmodes::worker_extracts_over_a_closed_tree", WORKER, "1".as_ref());
}

/// The half that runs as uid 1000. It does nothing with no [`WORKER`].
#[test]
fn worker_extracts_over_a_closed_tree() {
    if std::env::var_os(WORKER).is_none() {
        return;
    }
    assert_ne!(Ids::current().uid, 0, "the worker must not run as root: it then proves nothing");
    closed_tree_as_a_user();
}

fn closed_tree_as_a_user() {
    let s = Scratch::new("closed-user");
    let p = place(&s, DIGEST);
    let settings = crate::Settings::default();
    let image = closed_image(&s);
    crate::extract(&settings, DIGEST, &image, &p).unwrap();
    assert_closed(&p.rootfs);
    crate::extract(&settings, DIGEST, &image, &p)
        .unwrap_or_else(|e| panic!("the second extraction over a closed tree failed: {}", e.chain()));
    assert_closed(&p.rootfs);

    // The whiteouts of a first layer apply before its entries, so `ro` and
    // `ro/sub` are still closed when they run.
    let wh = layers(&s, &[&[E::File("ro/.wh.kid", b"", 0o644), E::File("ro/sub/.wh..wh..opq", b"", 0o644)]]);
    crate::extract(&settings, DIGEST, &wh, &p)
        .unwrap_or_else(|e| panic!("a whiteout in a closed directory failed: {}", e.chain()));
    assert!(!p.rootfs.join("ro/kid").exists() && !p.rootfs.join("ro/sub/f").exists());

    crate::extract(&settings, DIGEST, &image, &p).unwrap();
    let removed = p.remove().unwrap_or_else(|e| panic!("the removal of a closed tree failed: {}", e.chain()));
    assert!(removed && !p.rootfs.exists(), "the removal of a closed tree left {}", p.rootfs.display());
}
