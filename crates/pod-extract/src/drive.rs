//! ⭐ **The acceptance, driven in-process against crafted layers.**
//!
//! `AGENTS.md`: verify, do not accept. Every module beside this one tests
//! its own decision in isolation, and a set of correct decisions wired together
//! wrongly still extracts `/etc/passwd`. These build real tar layers, run the
//! real extraction over them, and look at the tree that came out.
//!
//! ⛔ **The crafted archives are deliberate.** `TODO/extract.md` T-0303's
//! premise says it in as many words: neither pinned image happens to carry a
//! root-level whiteout, so passing on them would say nothing. The same is true
//! of the path-escape case, which no honest image contains at all.

use std::io::Cursor;

use crate::apply::{apply_layer, Ids};
use crate::error::Error;
use crate::remove;
use crate::safety::Dir;
use crate::sidecar::Sidecar;

/// A scratch directory that removes itself. ⚠ Named by pid AND by a counter:
/// two tests in one process share a pid, and a shared scratch directory is two
/// tests measuring each other.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "podvm-extract-{}-{}-{}",
            tag,
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Scratch(p)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

enum E<'a> {
    File(&'a str, &'a [u8], u32),
    Owned(&'a str, &'a [u8], u64, u64),
    Dir(&'a str, u32),
    Symlink(&'a str, &'a str),
    Hardlink(&'a str, &'a str),
    /// ⭐ **A name written straight into the header, bypassing the writer's own
    /// validation.**
    ///
    /// ⚠ FOUND BY WRITING THIS TEST, and it is worth recording: `tar::Builder`
    /// REFUSES to write a member whose path contains `..`
    /// ("paths in archives must not have `..`"), while `tar::Archive`'s reader
    /// hands that same member straight to the caller. The writer's check is not
    /// the reader's, so a hostile layer cannot be built through the safe API and
    /// **can** be read through it. That asymmetry is the whole reason T-0304 is
    /// an entry: a Rust tar crate does not make this safe by itself.
    Raw(&'a str, &'a [u8]),
}

/// Build one uncompressed layer.
fn layer(entries: &[E]) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for e in entries {
        let mut h = tar::Header::new_gnu();
        match e {
            E::File(p, data, mode) => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_mode(*mode);
                h.set_size(data.len() as u64);
                b.append_data(&mut h, p, Cursor::new(*data)).unwrap();
            }
            E::Owned(p, data, uid, gid) => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_mode(0o640);
                h.set_uid(*uid);
                h.set_gid(*gid);
                h.set_size(data.len() as u64);
                b.append_data(&mut h, p, Cursor::new(*data)).unwrap();
            }
            E::Dir(p, mode) => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_mode(*mode);
                h.set_size(0);
                b.append_data(&mut h, p, std::io::empty()).unwrap();
            }
            E::Symlink(p, t) => {
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_mode(0o777);
                h.set_size(0);
                b.append_link(&mut h, p, t).unwrap();
            }
            E::Hardlink(p, t) => {
                h.set_entry_type(tar::EntryType::Link);
                h.set_mode(0o644);
                h.set_size(0);
                b.append_link(&mut h, p, t).unwrap();
            }
            E::Raw(p, data) => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_mode(0o644);
                h.set_size(data.len() as u64);
                {
                    let g = h.as_gnu_mut().expect("a GNU header");
                    let bytes = p.as_bytes();
                    assert!(bytes.len() < g.name.len(), "the crafted name must fit");
                    g.name[..bytes.len()].copy_from_slice(bytes);
                }
                // ⛔ After the name, or the checksum covers the old one and a
                // conforming reader rejects the member before podvm sees it.
                h.set_cksum();
                b.append(&h, Cursor::new(*data)).unwrap();
            }
        }
    }
    b.into_inner().unwrap()
}

/// Run the whole per-layer sequence, whiteouts first, exactly as
/// [`crate::extract`] does.
fn run(dir: &std::path::Path, layers: &[Vec<u8>]) -> crate::Result<u64> {
    let root = Dir::open(&dir.to_string_lossy())?;
    let side = dir.join(".meta.jsonl");
    let mut sc = Sidecar::create(&side)?;
    let mut removed = 0;
    for (n, l) in layers.iter().enumerate() {
        let ops = remove::collect(&mut Cursor::new(l.clone()))?;
        removed += remove::apply(&root, &ops)?;
        apply_layer(
            &root,
            &format!("sha256:layer{n}"),
            &mut Cursor::new(l.clone()),
            Ids::current(),
            &mut sc,
        )?;
    }
    sc.finish()?;
    Ok(removed)
}

fn meta(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join(".meta.jsonl")).unwrap_or_default()
}

// ---------------------------------------------------------------- T-0304

/// ⛔ **THE ATTACK M2 ACCEPTANCE 3 NAMES.** A crafted layer creates
/// `evil -> /etc` as its first entry and writes `evil/passwd` as its second.
/// Both entries are lexically inside the destination and the second lands
/// outside it.
///
/// ⚠ This is the half the corpus does NOT have.
/// `references/qaidvoid__onelf/tree/crates/onelf-format/src/manifest.rs:19-48`
/// validates a symlink entry when the symlink is created and never validates a
/// later entry whose path traverses it.
#[test]
fn an_entry_traversing_a_symlink_the_same_layer_made_is_refused() {
    let s = Scratch::new("traverse");
    let l = layer(&[
        E::Symlink("evil", "/etc"),
        E::File("evil/passwd", b"pwned\n", 0o644),
    ]);
    let r = run(s.path(), &[l]);
    match r {
        Err(Error::Refused { entry, why, layer }) => {
            assert_eq!(entry, "evil/passwd");
            assert!(why.contains("leaves the destination"), "{why}");
            // ⛔ T-0304: the layer digest is named, not just the entry.
            assert!(layer.contains("sha256:"), "{layer}");
        }
        other => panic!("the traversal was not refused: {other:?}"),
    }
    // ⛔ AND NOTHING LANDED OUTSIDE. The refusal is only worth having if the
    // write did not happen, so this asserts the tree rather than the message.
    assert!(
        !s.path().join("etc").join("passwd").exists(),
        "a file was written through the symlink"
    );
}

/// The same shape with a relative target, which needs no absolute path at all.
#[test]
fn a_relative_escaping_symlink_is_refused_too() {
    let s = Scratch::new("relesc");
    let l = layer(&[
        E::Symlink("out", "../../../../tmp"),
        E::File("out/x", b"x", 0o644),
    ]);
    assert!(
        matches!(run(s.path(), &[l]), Err(Error::Refused { .. })),
        "a relative escape was not refused"
    );
}

/// ⛔ Refused lexically, before any syscall.
///
/// ⚠ The archive is crafted at the header level because `tar::Builder` refuses
/// to write this member at all. See [`E::Raw`]: the writer's validation is not
/// the reader's, and only the reader's side is podvm's problem.
#[test]
fn a_dotdot_path_is_refused() {
    let s = Scratch::new("dotdot");
    let l = layer(&[E::Raw("../escaped", b"x")]);
    match run(s.path(), &[l]) {
        Err(Error::Refused { why, entry, .. }) => {
            assert_eq!(entry, "../escaped");
            assert!(why.contains(".."), "{why}");
        }
        other => panic!("`..` was not refused: {other:?}"),
    }
    assert!(
        !s.path().parent().unwrap().join("escaped").exists(),
        "a file landed beside the destination"
    );
}

/// ⛔ An absolute member name, which is the other half of the lexical check.
#[test]
fn an_absolute_path_is_refused() {
    let s = Scratch::new("abs");
    let l = layer(&[E::Raw("/etc/podvm-should-never-write-this", b"x")]);
    match run(s.path(), &[l]) {
        Err(Error::Refused { why, .. }) => assert!(why.contains("absolute"), "{why}"),
        other => panic!("an absolute path was not refused: {other:?}"),
    }
    assert!(!std::path::Path::new("/etc/podvm-should-never-write-this").exists());
}

/// ⛔ **Refuse rather than sanitize** (T-0304's Decision). The extraction fails
/// whole; the hostile entry is not quietly skipped while the rest lands.
#[test]
fn a_refusal_fails_the_extraction_rather_than_skipping_the_entry() {
    let s = Scratch::new("nosanitize");
    let l = layer(&[
        E::File("good", b"g", 0o644),
        E::Symlink("evil", "/etc"),
        E::File("evil/passwd", b"p", 0o644),
        E::File("after", b"a", 0o644),
    ]);
    assert!(run(s.path(), &[l]).is_err());
    // The entry after the refusal was never reached, which is what "fails the
    // extraction" means as opposed to "skips the entry".
    assert!(!s.path().join("after").exists());
}

// ---------------------------------------------------------------- T-0305

/// ⭐ The measured link out of a real voidlinux layer,
/// `experiments/results/whiteout-contract.txt` check C:
/// `var/cache/xbps -> /var/cache/xbps`. Rejecting it, as onelf does at
/// `references/qaidvoid__onelf/tree/crates/onelf-format/src/manifest.rs:27-29`,
/// would refuse almost every real image.
#[test]
fn an_absolute_symlink_is_created_and_points_where_the_image_said() {
    let s = Scratch::new("abslink");
    let l = layer(&[
        E::Dir("var/", 0o755),
        E::Dir("var/cache/", 0o755),
        E::Symlink("var/cache/xbps", "/var/cache/xbps"),
        E::Symlink("etc-mtab", "/proc/self/mounts"),
        E::Symlink("bin", "usr/bin"),
    ]);
    run(s.path(), &[l]).expect("legitimate absolute symlinks must extract");

    let got = std::fs::read_link(s.path().join("var/cache/xbps")).unwrap();
    // ⛔ Stored VERBATIM. Rebasing decides whether the link is allowed, never
    // what is written: storing the rebased form would bake this extraction's
    // idea of the rootfs into the image, and T-0305 rebases rather than
    // dereferences for exactly that reason.
    assert_eq!(got.to_string_lossy(), "/var/cache/xbps");
    assert_eq!(
        std::fs::read_link(s.path().join("etc-mtab"))
            .unwrap()
            .to_string_lossy(),
        "/proc/self/mounts"
    );
}

// ---------------------------------------------------------------- T-0303

/// ⭐ **THE MEASURED DEFECT.** `experiments/70-whiteout-contract.sh` check D
/// runs udocker's own selector, `tar t --wildcards -f LAYER '*/.wh.*'`
/// (`references/indigo-dc__udocker/tree/udocker/container/structure.py:242`),
/// against a crafted archive with one whiteout at the layer root and one
/// nested, and **the root one is missed**. Both are honoured here.
#[test]
fn a_whiteout_at_the_layer_root_removes_its_target() {
    let s = Scratch::new("whroot");
    let one = layer(&[
        E::File("atroot", b"1", 0o644),
        E::Dir("d/", 0o755),
        E::File("d/nested", b"2", 0o644),
    ]);
    let two = layer(&[
        E::File(".wh.atroot", b"", 0o644),
        E::File("d/.wh.nested", b"", 0o644),
    ]);
    let removed = run(s.path(), &[one, two]).unwrap();
    assert_eq!(removed, 2, "both whiteouts must have removed something");
    assert!(
        !s.path().join("atroot").exists(),
        "the ROOT-level whiteout is the one a glob misses"
    );
    assert!(!s.path().join("d/nested").exists());
    // ⛔ The marker itself is never materialised.
    assert!(!s.path().join(".wh.atroot").exists());
}

/// `.wh..wh..opq` removes the directory's CONTENTS and keeps the directory.
#[test]
fn an_opaque_marker_empties_its_directory_and_keeps_it() {
    let s = Scratch::new("opq");
    let one = layer(&[
        E::Dir("var/", 0o755),
        E::File("var/a", b"1", 0o644),
        E::File("var/b", b"2", 0o644),
    ]);
    let two = layer(&[
        E::File("var/.wh..wh..opq", b"", 0o644),
        E::File("var/c", b"3", 0o644),
    ]);
    run(s.path(), &[one, two]).unwrap();
    assert!(
        s.path().join("var").is_dir(),
        "the directory itself must survive"
    );
    assert!(!s.path().join("var/a").exists());
    assert!(!s.path().join("var/b").exists());
    // ⭐ T-0307's ordering: the opaque marker is applied BEFORE this layer's own
    // entries, so `var/c`, written by the same layer, survives.
    assert_eq!(std::fs::read(s.path().join("var/c")).unwrap(), b"3");
}

// ---------------------------------------------------------------- T-0307

/// ⭐ **THE ORDERING, and it is the case that distinguishes the two orders.**
/// `references/indigo-dc__udocker/tree/udocker/container/structure.py:279`
/// applies layer N's whiteouts before extracting N. A layer that deletes a path
/// and recreates it works only in that order; the opposite order deletes what
/// the same layer just wrote.
#[test]
fn a_layer_may_delete_a_path_and_recreate_it_in_the_same_layer() {
    let s = Scratch::new("delrecreate");
    let one = layer(&[E::File("f", b"old", 0o644)]);
    let two = layer(&[E::File(".wh.f", b"", 0o644), E::File("f", b"new", 0o644)]);
    run(s.path(), &[one, two]).unwrap();
    assert_eq!(
        std::fs::read(s.path().join("f")).unwrap(),
        b"new",
        "the whiteout ran after the layer's own entry and deleted it"
    );
}

/// A hard link is materialised as a link to the already-extracted target.
#[test]
fn a_hard_link_within_the_destination_is_materialised() {
    let s = Scratch::new("hardlink");
    let l = layer(&[
        E::File("orig", b"shared", 0o644),
        E::Hardlink("copy", "orig"),
    ]);
    run(s.path(), &[l]).unwrap();
    assert_eq!(std::fs::read(s.path().join("copy")).unwrap(), b"shared");
    let a = std::fs::metadata(s.path().join("orig")).unwrap();
    let b = std::fs::metadata(s.path().join("copy")).unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_eq!(a.ino(), b.ino(), "a hard link must share the inode");
}

/// ⛔ And one whose target is outside the destination is refused.
#[test]
fn a_hard_link_out_of_the_destination_is_refused() {
    let s = Scratch::new("hardescape");
    let l = layer(&[E::Hardlink("stolen", "/etc/passwd")]);
    assert!(
        matches!(run(s.path(), &[l]), Err(Error::Refused { .. })),
        "a hard link to an absolute path outside the rootfs was not refused"
    );
}

/// A later layer replacing a file with a symlink is ordinary OCI, and must not
/// write THROUGH the old file or the new link.
#[test]
fn a_later_layer_may_replace_a_file_with_a_symlink() {
    let s = Scratch::new("replace");
    let one = layer(&[E::File("x", b"file", 0o644)]);
    let two = layer(&[E::Symlink("x", "/dev/null")]);
    run(s.path(), &[one, two]).unwrap();
    let m = std::fs::symlink_metadata(s.path().join("x")).unwrap();
    assert!(m.file_type().is_symlink());
}

// ---------------------------------------------------------------- T-0306

/// ⭐ **The failure this prevents looks like a corrupt layer.** A mode-0555
/// directory in layer 1 cannot be written into when layer 2 extracts over it.
/// `references/indigo-dc__udocker/tree/udocker/container/structure.py:296-303`
/// is the pass being copied, and its docstring at
/// `references/indigo-dc__udocker/tree/udocker/container/structure.py:265-267`
/// states the reason.
#[test]
fn a_read_only_directory_in_one_layer_can_be_written_into_by_the_next() {
    let s = Scratch::new("reperm");
    let one = layer(&[E::Dir("ro/", 0o555)]);
    let two = layer(&[E::File("ro/added", b"ok", 0o644)]);
    run(s.path(), &[one, two])
        .expect("the second layer must be able to write into a 0555 directory");
    assert_eq!(std::fs::read(s.path().join("ro/added")).unwrap(), b"ok");
}

mod records;
