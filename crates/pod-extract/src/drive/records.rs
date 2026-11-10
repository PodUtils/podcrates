use super::*;

/// Owner bits only. Wider group or other bits change what the image means for
/// a payload that reads modes.
#[test]
fn re_permissioning_widens_the_owner_and_leaves_group_and_other_alone() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("ownerbits");
    let one = layer(&[E::Dir("d/", 0o555)]);
    run(s.path(), &[one]).unwrap();
    let m = std::fs::metadata(s.path().join("d")).unwrap().permissions().mode() & 0o777;
    assert_eq!(m & 0o700, 0o700, "owner rwx must be on: got {m:04o}");
    assert_eq!(m & 0o077, 0o055, "group and other must be untouched: got {m:04o}");
}

// ---------------------------------------------------------------- the sidecar

/// **THE WALL.** alpine has `-rw-r----- 0/42 etc/shadow`. Its extraction must
/// not fail, and the sidecar must keep what the image meant (a consumer runs
/// `jq -e 'select(.path=="etc/shadow") | .uid==0 and .gid==42 and .applied.gid==0'`
/// over the line that this asserts).
#[test]
fn the_shadow_file_extracts_and_its_dropped_gid_is_recorded() {
    let s = Scratch::new("shadow");
    let l = layer(&[E::Dir("etc/", 0o755), E::Owned("etc/shadow", b"root:!::\n", 0, 42)]);
    run(s.path(), &[l]).expect("gid 42 must not stop the extraction");

    // The file is there. GNU tar, containers/storage, Apptainer, pacman and
    // rurima stop here, with EINVAL from lchown.
    assert!(s.path().join("etc/shadow").is_file());

    let m = meta(s.path());
    let line = m
        .lines()
        .find(|l| l.contains(r#""path":"etc/shadow""#))
        .unwrap_or_else(|| panic!("no sidecar row for etc/shadow in:\n{m}"));
    assert!(line.contains(r#""gid":42"#), "{line}");
    assert!(line.contains(r#""uid":0"#), "{line}");
    assert!(line.contains(r#""reason""#), "{line}");
    assert!(line.contains("gid 42 unmapped"), "{line}");
}

/// **The sidecar changes no permission check of the kernel**, and this
/// asserts the true half: the file on disk has the extracting id, NOT gid 42.
/// A test that read only the sidecar would pass on a crate that claimed an
/// ownership that it never applied.
#[test]
fn the_sidecar_does_not_claim_an_ownership_the_kernel_did_not_apply() {
    use std::os::unix::fs::MetadataExt;
    let s = Scratch::new("honest");
    let l = layer(&[E::Dir("etc/", 0o755), E::Owned("etc/shadow", b"x", 0, 42)]);
    run(s.path(), &[l]).unwrap();
    let md = std::fs::metadata(s.path().join("etc/shadow")).unwrap();
    assert_ne!(md.gid(), 42, "the gid was somehow applied; the sidecar exists because it cannot be");
    assert_eq!(md.gid(), Ids::current().gid as u32);
}

/// Each entry that lands gets a row, so an export has the whole tree.
#[test]
fn every_extracted_entry_has_a_sidecar_row() {
    let s = Scratch::new("rows");
    let l = layer(&[
        E::Dir("a/", 0o755),
        E::File("a/one", b"1", 0o644),
        E::File("a/two", b"2", 0o600),
        E::Symlink("a/three", "one"),
    ]);
    run(s.path(), &[l]).unwrap();
    let m = meta(s.path());
    assert_eq!(m.lines().count(), 4, "one row per entry:\n{m}");
    for p in ["a", "a/one", "a/two", "a/three"] {
        assert!(m.contains(&format!(r#""path":"{p}""#)), "no row for {p} in:\n{m}");
    }
}

// ------------------------------------------------- the whole extraction

/// The `sha256:<hex>` of `bytes`.
fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let h = sha2::Sha256::digest(bytes);
    let mut s = String::from("sha256:");
    for b in h {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// One layer blob in the scratch directory, with its place, so that
/// [`crate::extract`] runs and not only [`apply_layer`].
fn one_layer(s: &Scratch, entries: &[E]) -> Vec<crate::Layer> {
    use std::io::Write as _;
    let tar = layer(entries);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    let blob = gz.finish().unwrap();
    vec![stored(s, &blob, "application/vnd.oci.image.layer.v1.tar+gzip")]
}

/// A blob in the scratch directory, named by its digest.
fn stored(s: &Scratch, blob: &[u8], media_type: &str) -> crate::Layer {
    let digest = sha256(blob);
    let path = s.path().join("blobs").join(digest.replace(':', "_"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, blob).unwrap();
    crate::Layer { digest, media_type: media_type.into(), size: blob.len() as u64, blob: path }
}

/// The place of the image `digest` in the scratch directory.
fn place(s: &Scratch, digest: &str) -> crate::Place {
    crate::Place::under(&s.path().join("rootfs").join(digest.replace(':', "_")))
}

/// **A DEFECT THAT NO UNIT TEST FOUND.**
///
/// Each decision of this crate was correct and the connection between them
/// was not. A refused extraction leaves a directory and a sidecar, as the
/// refusal comes partway through their write. A check of only those two told
/// the NEXT extraction of the same image "already done", with the path and
/// exit 0: a half tree with the symlink of the attacker in it. The refusal was
/// correct, and the call after it undid it.
#[test]
fn a_refused_extraction_leaves_nothing_that_reads_as_extracted() {
    let s = Scratch::new("refusedstate");
    let layers = one_layer(&s, &[E::Symlink("evil", "/etc"), E::File("evil/passwd", b"pwned", 0o644)]);
    let digest = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    let p = place(&s, digest);
    let settings = crate::Settings::default();

    let first = crate::extract(&settings, digest, &layers, &p);
    assert!(first.as_ref().err().and_then(crate::refused).is_some(), "{first:?}");

    // The three things that were all wrong before.
    assert!(!p.is_extracted(), "a refused extraction reports as extracted");
    assert!(!p.rootfs.join("evil").exists(), "the attacker's symlink survived the refusal");
    // A second try must refuse again, and not serve the wreck.
    let second = crate::extract(&settings, digest, &layers, &p);
    assert!(
        second.as_ref().err().and_then(crate::refused).is_some(),
        "the second extraction did not refuse: {second:?}"
    );
}

/// The other half: a successful extraction DOES read as extracted, so the
/// marker is not always absent.
#[test]
fn a_successful_extraction_reports_as_extracted() {
    let s = Scratch::new("okstate");
    let layers = one_layer(&s, &[E::File("f", b"hello", 0o644)]);
    let digest = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
    let p = place(&s, digest);

    assert!(!p.is_extracted(), "before");
    let done = crate::extract(&crate::Settings::default(), digest, &layers, &p).unwrap();
    assert!(p.is_extracted(), "after");
    assert_eq!(std::fs::read(done.rootfs.join("f")).unwrap(), b"hello");
    // The gzip trailer gives the true uncompressed length: a measurement, not
    // a multiplier.
    assert!(!done.uncompressed_estimated);
    assert_eq!(done.uncompressed_bytes, layer(&[E::File("f", b"hello", 0o644)]).len() as u64);
}

/// A directory and a sidecar with NO marker is what a `SIGKILL` between two
/// entries leaves. It must not read as extracted either: the marker is
/// necessary, not only the removal.
#[test]
fn a_tree_left_by_a_kill_does_not_read_as_extracted() {
    let s = Scratch::new("killed");
    let p = place(&s, "sha256:3333333333333333333333333333333333333333333333333333333333333333");
    std::fs::create_dir_all(&p.rootfs).unwrap();
    std::fs::write(&p.sidecar, b"{}\n").unwrap();
    assert!(!p.is_extracted(), "a rootfs and a sidecar with no completion marker read as extracted");
}

/// Each layer gives one event to the sink of the caller, with the short
/// digest and the counts, and the crate prints nothing.
#[test]
fn each_layer_gives_one_event_to_the_caller() {
    use std::sync::{Arc, Mutex};
    let s = Scratch::new("events");
    let layers = one_layer(&s, &[E::File("f", b"x", 0o644), E::File(".wh.gone", b"", 0o644)]);
    let digest = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
    let seen = Arc::new(Mutex::new(Vec::new()));
    let keep = seen.clone();
    let settings = crate::Settings {
        report: pod_core::Report {
            events: Arc::new(move |e: pod_core::Event| keep.lock().unwrap().push(e)),
            ..pod_core::Report::default()
        },
        ..crate::Settings::default()
    };
    crate::extract(&settings, digest, &layers, &place(&s, digest)).unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].kind, crate::LAYER_EVENT);
    let short = &layers[0].digest["sha256:".len().."sha256:".len() + 12];
    assert_eq!(seen[0].text, format!("{short}: Extracted (2 entries, 1 whiteouts)"));
}

// ---------------------------------------------------------------- the modes

/// The mode that the image asked for is the mode of the file at the end, also
/// when the crate had to widen it for the write of the content.
#[test]
fn a_mode_the_extractor_cannot_write_through_is_still_the_final_mode() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("mode");
    let l = layer(&[E::File("readonly", b"content", 0o444)]);
    run(s.path(), &[l]).unwrap();
    let p = s.path().join("readonly");
    assert_eq!(std::fs::read(&p).unwrap(), b"content");
    assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o444);
}

// ---------------------------------------------------------------- the digest

/// A blob whose bytes changed after the pull is refused with a digest
/// mismatch that names the blob, and the changed bytes are never written: the
/// manifest names the first digest, and the stored blob no longer agrees.
#[test]
fn a_blob_edited_after_pull_is_refused_and_never_written() {
    let s = Scratch::new("tamper");
    let bytes = layer(&[E::File("greeting", b"hello honest world", 0o644)]);
    let l = stored(&s, &bytes, "application/vnd.oci.image.layer.v1.tar");
    let want = l.digest.clone();
    // One content byte changed in the stored blob: `hello` becomes `heLlo`.
    let mut changed = std::fs::read(&l.blob).unwrap();
    let at = changed.windows(b"hello".len()).position(|w| w == b"hello").expect("the stored tar carries the greeting");
    changed[at + 2] = b'L';
    std::fs::write(&l.blob, &changed).unwrap();
    let digest = format!("sha256:{}", "7".repeat(64));
    let p = place(&s, &digest);
    let Err(e) = crate::extract(&crate::Settings::default(), &digest, &[l], &p) else {
        panic!("a tampered blob extracted cleanly");
    };
    let text = format!("{e}");
    assert!(text.contains("mismatch"), "{text}");
    assert!(text.contains(&want), "{text}");
    assert_eq!(crate::mismatch(&e).map(|m| m.want.as_str()), Some(want.as_str()));
    assert!(std::fs::read(p.rootfs.join("greeting")).is_err(), "the tampered bytes reached the rootfs");
}
