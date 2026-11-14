//! **The acceptance, in this process, against crafted layers.**
//!
//! Each module beside this one tests its own decision alone, and correct
//! decisions with wrong connections between them still extract
//! `/etc/passwd`. These tests make real tar layers, run the real extraction
//! over them, and look at the tree that came out.
//!
//! **The crafted archives are on purpose.** Real images do not have a
//! whiteout at the root of a layer, so a pass on them tells nothing. The same
//! is true of the path escapes, which no honest image has.

use std::io::Cursor;

use crate::apply::{Ids, apply_layer};
use crate::error::io;
use crate::modes::DirModes;
use crate::remove;
use crate::safety::Dir;
use crate::sidecar::Sidecar;

/// A scratch directory that removes itself. Named by the pid AND a counter:
/// two tests in one process have the same pid, and a shared scratch
/// directory is two tests that measure each other.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "pod-extract-{}-{}-{}",
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
        // The crate's own removal opens a directory that the image closed
        // (0555); `remove_dir_all` as a user cannot empty it.
        let _ = crate::remove_tree(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

enum E<'a> {
    File(&'a str, &'a [u8], u32),
    Owned(&'a str, &'a [u8], u64, u64),
    Dir(&'a str, u32),
    Symlink(&'a str, &'a str),
    Hardlink(&'a str, &'a str),
    /// **A name written into the header, past the check of the writer.**
    ///
    /// `tar::Builder` REFUSES to write a member whose path has `..` ("paths in
    /// archives must not have `..`"), and the reader of `tar::Archive` gives
    /// the same member to the caller. The check of the writer is not the check
    /// of the reader: the safe API cannot make a hostile layer and **can**
    /// read one. A Rust tar crate does not make the extraction safe alone.
    Raw(&'a str, &'a [u8]),
    /// A directory whose name is written into the header past the check
    /// of the writer: `tar::Builder` refuses an empty path.
    RawDir(&'a str, u32),
    /// A fifo, which the crate does not make.
    Fifo(&'a str),
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
                // After the name: else the checksum covers the old name, and a
                // conforming reader refuses the member before the crate sees it.
                h.set_cksum();
                b.append(&h, Cursor::new(*data)).unwrap();
            }
            E::RawDir(p, mode) => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_mode(*mode);
                h.set_size(0);
                {
                    let g = h.as_gnu_mut().expect("a GNU header");
                    let bytes = p.as_bytes();
                    assert!(bytes.len() < g.name.len(), "the crafted name must fit");
                    g.name[..bytes.len()].copy_from_slice(bytes);
                }
                h.set_cksum();
                b.append(&h, std::io::empty()).unwrap();
            }
            E::Fifo(p) => {
                h.set_entry_type(tar::EntryType::Fifo);
                h.set_mode(0o644);
                h.set_size(0);
                b.append_data(&mut h, p, std::io::empty()).unwrap();
            }
        }
    }
    b.into_inner().unwrap()
}

/// Run the steps of each layer, whiteouts first, and the pass after the last
/// layer, as [`crate::extract`] does.
fn run(dir: &std::path::Path, layers: &[Vec<u8>]) -> crate::Result<u64> {
    steps(dir, layers, true)
}

/// [`run`] with no pass after the last layer: the tree between two layers.
fn run_open(dir: &std::path::Path, layers: &[Vec<u8>]) -> crate::Result<u64> {
    steps(dir, layers, false)
}

fn steps(dir: &std::path::Path, layers: &[Vec<u8>], last_pass: bool) -> crate::Result<u64> {
    let root = Dir::open(&dir.to_string_lossy())?;
    let side = dir.join(".meta.jsonl");
    let mut sc = Sidecar::create(&side).map_err(io)?;
    let mut dirs = DirModes::new();
    let mut removed = 0;
    for (n, l) in layers.iter().enumerate() {
        let ops = remove::collect(&format!("sha256:layer{n}"), &mut Cursor::new(l.clone()))?;
        removed += remove::apply(&root, &ops)?;
        dirs.forget(&ops);
        let st = apply_layer(&root, &format!("sha256:layer{n}"), &mut Cursor::new(l.clone()), Ids::current(), &mut sc)?;
        dirs.record(&st.dir_modes);
    }
    if last_pass {
        dirs.set(&root)?;
    }
    sc.finish().map_err(io)?;
    Ok(removed)
}

fn meta(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join(".meta.jsonl")).unwrap_or_default()
}

/// The mode on disk of each path, and the applied mode of its sidecar row
/// (`None` when the row has none).
fn kept(dir: &std::path::Path, paths: &[&str]) -> Vec<(String, u32, Option<String>)> {
    use std::os::unix::fs::PermissionsExt;
    let m = meta(dir);
    paths
        .iter()
        .map(|p| {
            let disk = std::fs::symlink_metadata(dir.join(p)).unwrap().permissions().mode() & 0o7777;
            let row = m
                .lines()
                .find(|l| l.contains(&format!(r#""path":"{p}""#)))
                .unwrap_or_else(|| panic!("no sidecar row for {p} in:\n{m}"));
            let applied = row.split(r#""applied":{"#).nth(1).and_then(|a| a.split('}').next()).unwrap_or("");
            let mode = applied.split(r#""mode":""#).nth(1).map(|v| v[..4].to_string());
            ((*p).to_string(), disk, mode)
        })
        .collect()
}

/// The cause of a refused extraction, or a panic that says what came.
fn refusal_of<T: std::fmt::Debug>(r: crate::Result<T>) -> crate::Refused {
    match r {
        Err(f) => crate::refused(&f).cloned().unwrap_or_else(|| panic!("not a refusal: {}", f.chain())),
        Ok(v) => panic!("not refused: {v:?}"),
    }
}

// ---------------------------------------------------------------- containment

/// **THE ATTACK.** A crafted layer makes `evil -> /etc` as its first entry and
/// writes `evil/passwd` as its second. Both entries are lexically in the
/// destination, and the second lands outside it.
///
/// onelf checks a symlink entry when it makes the symlink, and never checks a
/// later entry whose path goes through it.
#[test]
fn an_entry_traversing_a_symlink_the_same_layer_made_is_refused() {
    let s = Scratch::new("traverse");
    let l = layer(&[E::Symlink("evil", "/etc"), E::File("evil/passwd", b"pwned\n", 0o644)]);
    let r = refusal_of(run(s.path(), &[l]));
    assert_eq!(r.entry, "evil/passwd");
    let why = r.refusal.why();
    assert!(why.contains("leaves the destination"), "{why}");
    // The digest of the layer is named, not only the entry.
    assert!(r.layer.contains("sha256:"), "{}", r.layer);
    // AND NOTHING LANDED OUTSIDE. The refusal has a value only when the write
    // did not happen, so this asserts the tree and not the message.
    assert!(!s.path().join("etc").join("passwd").exists(), "a file was written through the symlink");
}

/// The same shape with a relative target, which needs no absolute path at all.
#[test]
fn a_relative_escaping_symlink_is_refused_too() {
    let s = Scratch::new("relesc");
    let l = layer(&[E::Symlink("out", "../../../../tmp"), E::File("out/x", b"x", 0o644)]);
    refusal_of(run(s.path(), &[l]));
}

/// Refused lexically, before a system call.
///
/// The archive is crafted in the header, as `tar::Builder` does not write
/// this member ([`E::Raw`]): the check of the writer is not the check of the
/// reader, and only the reader is the problem of this crate.
#[test]
fn a_dotdot_path_is_refused() {
    let s = Scratch::new("dotdot");
    let l = layer(&[E::Raw("../escaped", b"x")]);
    let r = refusal_of(run(s.path(), &[l]));
    assert_eq!(r.entry, "../escaped");
    assert!(r.refusal.why().contains(".."), "{}", r.refusal.why());
    assert!(!s.path().parent().unwrap().join("escaped").exists(), "a file landed beside the destination");
}

/// An absolute member name: the other half of the lexical check.
#[test]
fn an_absolute_path_is_refused() {
    let s = Scratch::new("abs");
    let l = layer(&[E::Raw("/etc/pod-extract-should-never-write-this", b"x")]);
    let why = refusal_of(run(s.path(), &[l])).refusal.why();
    assert!(why.contains("absolute"), "{why}");
    assert!(!std::path::Path::new("/etc/pod-extract-should-never-write-this").exists());
}

/// **Refuse, do not repair.** The extraction fails whole; the hostile entry is
/// not skipped while the rest lands.
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
    // The entry after the refusal was never reached: "fails the extraction",
    // not "skips the entry".
    assert!(!s.path().join("after").exists());
}

// ---------------------------------------------------------- absolute symlinks

/// The link of a real void layer: `var/cache/xbps -> /var/cache/xbps`. A
/// refusal of it, as onelf does, refuses almost each real image.
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
    // Stored WITH NO CHANGE. The rebase decides if the link is allowed, not
    // what is written: a stored rebased form puts the root of this extraction
    // into the image.
    assert_eq!(got.to_string_lossy(), "/var/cache/xbps");
    assert_eq!(std::fs::read_link(s.path().join("etc-mtab")).unwrap().to_string_lossy(), "/proc/self/mounts");
}

// ---------------------------------------------------------------- whiteouts

/// **THE MEASURED DEFECT.** udocker's selector,
/// `tar t --wildcards -f LAYER '*/.wh.*'`, on a crafted archive with one
/// whiteout at the root of the layer and one deeper, **misses the one at the
/// root**. Both apply here.
#[test]
fn a_whiteout_at_the_layer_root_removes_its_target() {
    let s = Scratch::new("whroot");
    let one = layer(&[E::File("atroot", b"1", 0o644), E::Dir("d/", 0o755), E::File("d/nested", b"2", 0o644)]);
    let two = layer(&[E::File(".wh.atroot", b"", 0o644), E::File("d/.wh.nested", b"", 0o644)]);
    let removed = run(s.path(), &[one, two]).unwrap();
    assert_eq!(removed, 2, "both whiteouts must have removed something");
    assert!(!s.path().join("atroot").exists(), "the ROOT-level whiteout is the one a glob misses");
    assert!(!s.path().join("d/nested").exists());
    // The marker itself is never made.
    assert!(!s.path().join(".wh.atroot").exists());
}

/// The name of the whiteout `.wh...` is `..`: at the root of a layer it names
/// the directory above the destination. The crate refuses the layer, and
/// removes nothing outside the destination.
#[test]
fn a_whiteout_of_dotdot_removes_nothing_outside_the_destination() {
    let s = Scratch::new("whdotdot");
    let dest = s.path().join("root");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(s.path().join("keep"), b"k").unwrap();
    let r = run(&dest, &[layer(&[E::Raw(".wh...", b"")])]);
    assert!(s.path().join("keep").exists(), "a whiteout removed a file outside the destination");
    let r = refusal_of(r);
    assert_eq!(r.entry, ".wh...");
}

/// A whiteout under `..` gets the lexical check of each other entry. With
/// the walk (a kernel with no `openat2`) it would reach the directory above
/// the destination.
#[test]
fn a_whiteout_under_dotdot_is_refused() {
    let s = Scratch::new("whup");
    let dest = s.path().join("root");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(s.path().join("keep"), b"k").unwrap();
    let r = refusal_of(run(&dest, &[layer(&[E::Raw("../.wh.keep", b"")])]));
    assert_eq!(r.entry, "../.wh.keep");
    assert!(s.path().join("keep").exists(), "a whiteout removed a file outside the destination");
}

/// `.wh..wh..opq` removes the directory's CONTENTS and keeps the directory.
#[test]
fn an_opaque_marker_empties_its_directory_and_keeps_it() {
    let s = Scratch::new("opq");
    let one = layer(&[E::Dir("var/", 0o755), E::File("var/a", b"1", 0o644), E::File("var/b", b"2", 0o644)]);
    let two = layer(&[E::File("var/.wh..wh..opq", b"", 0o644), E::File("var/c", b"3", 0o644)]);
    run(s.path(), &[one, two]).unwrap();
    assert!(s.path().join("var").is_dir(), "the directory itself must survive");
    assert!(!s.path().join("var/a").exists());
    assert!(!s.path().join("var/b").exists());
    // The order: the opaque marker applies BEFORE the entries of its layer,
    // so `var/c`, written by the same layer, stays.
    assert_eq!(std::fs::read(s.path().join("var/c")).unwrap(), b"3");
}

// ------------------------------------------------------------ order and links

/// **THE ORDER: the case that tells the two orders apart.** The whiteouts of
/// layer N apply before the entries of N. A layer that removes a path and
/// makes it again works only in that order; the other order removes what the
/// same layer wrote.
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
    let l = layer(&[E::File("orig", b"shared", 0o644), E::Hardlink("copy", "orig")]);
    run(s.path(), &[l]).unwrap();
    assert_eq!(std::fs::read(s.path().join("copy")).unwrap(), b"shared");
    let a = std::fs::metadata(s.path().join("orig")).unwrap();
    let b = std::fs::metadata(s.path().join("copy")).unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_eq!(a.ino(), b.ino(), "a hard link must share the inode");
}

/// A hard link whose target is outside the destination is refused.
#[test]
fn a_hard_link_out_of_the_destination_is_refused() {
    let s = Scratch::new("hardescape");
    let l = layer(&[E::Hardlink("stolen", "/etc/passwd")]);
    refusal_of(run(s.path(), &[l]));
}

/// A later layer that puts a symlink in place of a file is ordinary OCI, and
/// must not write THROUGH the old file or the new link.
#[test]
fn a_later_layer_may_replace_a_file_with_a_symlink() {
    let s = Scratch::new("replace");
    let one = layer(&[E::File("x", b"file", 0o644)]);
    let two = layer(&[E::Symlink("x", "/dev/null")]);
    run(s.path(), &[one, two]).unwrap();
    let m = std::fs::symlink_metadata(s.path().join("x")).unwrap();
    assert!(m.file_type().is_symlink());
}

// ---------------------------------------------------------- the permission pass

/// **With no pass, the failure looks like a corrupt layer.** A directory of
/// mode 0555 in layer 1 cannot take the entries of layer 2 (udocker's pass
/// after each layer).
#[test]
fn a_read_only_directory_in_one_layer_can_be_written_into_by_the_next() {
    let s = Scratch::new("reperm");
    let one = layer(&[E::Dir("ro/", 0o555)]);
    let two = layer(&[E::File("ro/added", b"ok", 0o644)]);
    run(s.path(), &[one, two]).expect("the second layer must be able to write into a 0555 directory");
    assert_eq!(std::fs::read(s.path().join("ro/added")).unwrap(), b"ok");
}

mod dirmodes;
mod records;
mod setid;
mod umask;
mod user;
mod whiteouts;

/// A layer whose first entry is the root as a directory with an empty name
/// (`public.ecr.aws/lambda/provided:al2023`, PodUtils/podvm#261) applies:
/// the root entry is skipped, as `./` is, and the next entry is written. A
/// file with an empty name is still refused.
#[test]
fn a_root_directory_with_an_empty_name_is_the_root() {
    let s = Scratch::new("emptyroot");
    let l = layer(&[E::RawDir("", 0o700), E::File("THIRD-PARTY-LICENSES.txt", b"text", 0o644)]);
    run(s.path(), &[l]).expect("the layer applies");
    assert_eq!(std::fs::read(s.path().join("THIRD-PARTY-LICENSES.txt")).unwrap(), b"text");
    let r = refusal_of(run(s.path(), &[layer(&[E::Raw("", b"x")])]));
    assert_eq!(r.entry, "");
}

/// The `./` member names the destination: an entry, and not a thing that the
/// crate could not make (PodUtils/podvm#352). A fifo still is one.
#[test]
fn the_root_member_is_not_a_skipped_entry() {
    let s = Scratch::new("rootskip");
    let root = Dir::open(&s.path().to_string_lossy()).unwrap();
    let mut sc = Sidecar::create(&s.path().join(".meta.jsonl")).unwrap();
    let mut apply =
        |l: Vec<u8>| apply_layer(&root, "sha256:root", &mut Cursor::new(l), Ids::current(), &mut sc).unwrap();
    let st = apply(layer(&[E::RawDir("./", 0o755), E::File("f", b"f", 0o644)]));
    assert_eq!((st.entries, st.skipped), (2, 0), "the root member counted as skipped: {:?}", st.skipped_kinds);
    assert!(st.skipped_kinds.is_empty(), "the root member counted as skipped: {:?}", st.skipped_kinds);
    let st = apply(layer(&[E::RawDir("./", 0o755), E::Fifo("p")]));
    assert_eq!((st.entries, st.skipped), (2, 1));
    assert_eq!(st.skipped_kinds, ["Fifo"], "the fifo is not named, or the root member is");
}
