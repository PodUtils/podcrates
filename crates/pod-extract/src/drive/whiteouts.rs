//! The whiteouts that leave the destination. The cases of podvm's
//! `drive/whiteouts.rs` (PodUtils/podvm T-1698) and of its check in
//! `remove.rs`, on the API of this crate.

use super::*;

/// The destination of these tests is one level below the scratch directory.
/// A whiteout that escapes can then reach only the sentinel beside it, and
/// never the shared temporary directory.
fn nested(s: &Scratch) -> std::path::PathBuf {
    let root = s.path().join("rootfs");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(s.path().join("sentinel"), b"keep").unwrap();
    root
}

fn sentinel_stays(s: &Scratch, what: &str) {
    assert!(s.path().join("sentinel").exists(), "{what} left the destination");
}

/// `.wh...` at the root of a layer names `..`. Its removal empties the
/// directory above the destination. The tar writer makes this member.
#[test]
fn a_whiteout_of_dotdot_at_the_root_refuses_the_layer() {
    let s = Scratch::new("whdotdotroot");
    let root = nested(&s);
    let r = refusal_of(run(&root, &[layer(&[E::File(".wh...", b"", 0o644)])]));
    assert_eq!(r.entry, ".wh...");
    let why = r.refusal.why();
    assert!(why.contains(".."), "{why}");
    sentinel_stays(&s, ".wh...");
}

/// A whiteout whose directory climbs out gets the lexical refusal, so a
/// kernel with no `openat2` never sees it.
#[test]
fn a_whiteout_that_climbs_out_refuses_the_layer() {
    let s = Scratch::new("whclimb");
    let root = nested(&s);
    for p in ["../.wh.sentinel", "a/../../.wh.sentinel", "../.wh..wh..opq"] {
        let r = refusal_of(run(&root, &[layer(&[E::Raw(p, b"")])]));
        assert_eq!(r.entry, p);
        sentinel_stays(&s, p);
    }
}

/// `.wh..` names the directory that holds it. No layer can delete that
/// directory by name; the layer is refused, as an entry named `.` is.
#[test]
fn a_whiteout_of_dot_refuses_the_layer() {
    let s = Scratch::new("whdot");
    let root = nested(&s);
    let l = layer(&[E::Dir("d/", 0o755), E::File("d/.wh..", b"", 0o644)]);
    let r = refusal_of(run(&root, &[l]));
    assert_eq!(r.entry, "d/.wh..");
    sentinel_stays(&s, "d/.wh..");
}

/// An absolute whiteout gets the refusal of an absolute entry (podvm's
/// `checked_dir`). The refusal comes before each removal of the layer, also
/// of a legal whiteout before it.
#[test]
fn an_absolute_whiteout_refuses_the_layer_before_each_removal() {
    let s = Scratch::new("whabs");
    let root = nested(&s);
    run(&root, &[layer(&[E::File("f", b"1", 0o644)])]).unwrap();
    let two = layer(&[E::File(".wh.f", b"", 0o644), E::Raw("/.wh.sentinel", b"")]);
    let r = refusal_of(run(&root, &[two]));
    assert_eq!(r.entry, "/.wh.sentinel");
    let why = r.refusal.why();
    assert!(why.contains("absolute"), "{why}");
    assert!(root.join("f").exists(), "a whiteout applied before the refusal of its layer");
    sentinel_stays(&s, "/.wh.sentinel");
}
