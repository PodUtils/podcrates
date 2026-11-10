use super::*;

/// ⛔ Owner bits only. Widening group or other bits changes what the image
/// means for a payload that reads modes.
#[test]
fn re_permissioning_widens_the_owner_and_leaves_group_and_other_alone() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("ownerbits");
    let one = layer(&[E::Dir("d/", 0o555)]);
    run(s.path(), &[one]).unwrap();
    let m = std::fs::metadata(s.path().join("d"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(m & 0o700, 0o700, "owner rwx must be on: got {m:04o}");
    assert_eq!(
        m & 0o077,
        0o055,
        "group and other must be untouched: got {m:04o}"
    );
}

// ---------------------------------------------------------------- T-0302

/// ⭐ **THE WALL.** `experiments/results/whiteout-contract.txt` check B reads
/// `-rw-r----- 0/42 ... etc/shadow` out of a pinned alpine image. Extracting it
/// must not fail, and the sidecar must carry what the image meant.
///
/// T-0302's `Prove` runs
/// `jq -e 'select(.path=="etc/shadow") | .uid==0 and .gid==42 and .applied.gid==0'`
/// over the line this asserts.
#[test]
fn the_shadow_file_extracts_and_its_dropped_gid_is_recorded() {
    let s = Scratch::new("shadow");
    let l = layer(&[
        E::Dir("etc/", 0o755),
        E::Owned("etc/shadow", b"root:!::\n", 0, 42),
    ]);
    run(s.path(), &[l]).expect("gid 42 must not stop the extraction");

    // ⛔ The file is there. This is where GNU tar, containers/storage,
    // Apptainer, pacman and rurima stop, with EINVAL from lchown.
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

/// ⛔ **The sidecar changes no kernel permission check**, and this asserts the
/// honest half: the file on disk is owned by the extracting id, NOT by gid 42.
/// A test that only read the sidecar would pass on an implementation that
/// claimed an ownership it never applied.
#[test]
fn the_sidecar_does_not_claim_an_ownership_the_kernel_did_not_apply() {
    use std::os::unix::fs::MetadataExt;
    let s = Scratch::new("honest");
    let l = layer(&[E::Dir("etc/", 0o755), E::Owned("etc/shadow", b"x", 0, 42)]);
    run(s.path(), &[l]).unwrap();
    let md = std::fs::metadata(s.path().join("etc/shadow")).unwrap();
    assert_ne!(
        md.gid(),
        42,
        "the gid was somehow applied; the sidecar exists because it cannot be"
    );
    assert_eq!(md.gid(), Ids::current().gid as u32);
}

/// Every entry that lands gets a row, so a re-export has the whole tree.
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
        assert!(
            m.contains(&format!(r#""path":"{p}""#)),
            "no row for {p} in:\n{m}"
        );
    }
}

// ------------------------------------------------- the whole-extract seam

/// Build a one-layer store and manifest, so [`crate::extract`] can be driven
/// rather than only [`apply_layer`].
fn one_layer_store(s: &Scratch, entries: &[E]) -> (podvm_image::Store, String) {
    use std::io::Write as _;
    let store = podvm_image::Store::open(s.path().join("store")).unwrap();
    let tar = layer(entries);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    let blob = gz.finish().unwrap();

    let ldigest = podvm_image::digest::Digest::of(&blob);
    store.put_bytes(&blob, &ldigest, "layer").unwrap();
    let cfg = br#"{"architecture":"amd64","os":"linux"}"#;
    let cdigest = podvm_image::digest::Digest::of(cfg);
    store.put_bytes(cfg, &cdigest, "config").unwrap();

    let m = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json",
            "config":{{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"{cdigest}","size":{}}},
            "layers":[{{"mediaType":"application/vnd.oci.image.layer.v1.tar+gzip","digest":"{ldigest}","size":{}}}]}}"#,
        cfg.len(),
        blob.len()
    );
    (store, m)
}

/// ⭐ **THE DEFECT `experiments/220-extract-path-safety.sh` FOUND, and no unit
/// test could have.**
///
/// Every decision in this crate was correct and the seam between them was not.
/// A refused extraction leaves a directory and a sidecar behind, because the
/// refusal happens partway through writing them. `is_extracted` asked whether
/// those two existed, so the very NEXT `podvm extract` of the same image
/// answered "already done", printed the path and exited **0**,  handing back a
/// half-extracted tree with the attacker's symlink still in it. The refusal was
/// correct and the call after it undid the whole thing.
#[test]
fn a_refused_extraction_leaves_nothing_that_reads_as_extracted() {
    let s = Scratch::new("refusedstate");
    let (store, mj) = one_layer_store(
        &s,
        &[
            E::Symlink("evil", "/etc"),
            E::File("evil/passwd", b"pwned", 0o644),
        ],
    );
    let manifest: podvm_image::oci::Manifest = serde_json::from_str(&mj).unwrap();
    let digest = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

    let first = crate::extract(&store, &manifest, digest, &mut std::io::sink());
    assert!(matches!(first, Err(Error::Refused { .. })), "{first:?}");

    // ⛔ The three things that were all wrong before.
    assert!(
        !crate::is_extracted(&store, digest),
        "a refused extraction reports as extracted"
    );
    let (rootfs, _) = crate::paths(&store, digest);
    assert!(
        !rootfs.join("evil").exists(),
        "the attacker's symlink survived the refusal"
    );
    // And a second attempt must refuse again rather than serve the wreckage.
    let second = crate::extract(&store, &manifest, digest, &mut std::io::sink());
    assert!(
        matches!(second, Err(Error::Refused { .. })),
        "the second extraction did not refuse: {second:?}"
    );
}

/// The other half: a successful extraction DOES report as extracted, so the
/// marker is not simply always absent.
#[test]
fn a_successful_extraction_reports_as_extracted() {
    let s = Scratch::new("okstate");
    let (store, mj) = one_layer_store(&s, &[E::File("f", b"hello", 0o644)]);
    let manifest: podvm_image::oci::Manifest = serde_json::from_str(&mj).unwrap();
    let digest = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

    assert!(!crate::is_extracted(&store, digest), "before");
    let done = crate::extract(&store, &manifest, digest, &mut std::io::sink()).unwrap();
    assert!(crate::is_extracted(&store, digest), "after");
    assert_eq!(std::fs::read(done.rootfs.join("f")).unwrap(), b"hello");
    // ⭐ The gzip trailer gives the real uncompressed length, so this is a
    // measurement rather than a multiplier.
    assert!(!done.uncompressed_estimated);
    assert_eq!(
        done.uncompressed_bytes,
        layer(&[E::File("f", b"hello", 0o644)]).len() as u64
    );
}

/// ⛔ A directory and a sidecar with NO marker is the shape a `SIGKILL` between
/// two entries leaves. It must not read as extracted either, which is why the
/// marker exists rather than only the cleanup.
#[test]
fn a_tree_left_by_a_kill_does_not_read_as_extracted() {
    let s = Scratch::new("killed");
    let store = podvm_image::Store::open(s.path().join("store")).unwrap();
    let digest = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
    let (rootfs, sidecar) = crate::paths(&store, digest);
    std::fs::create_dir_all(&rootfs).unwrap();
    std::fs::write(&sidecar, b"{}\n").unwrap();
    assert!(
        !crate::is_extracted(&store, digest),
        "a rootfs and a sidecar with no completion marker read as extracted"
    );
}

// ---------------------------------------------------------------- T-0301

/// ⛔ The mode the image asked for is what the file ends up with, even where it
/// is one this process had to widen to write the content.
#[test]
fn a_mode_the_extractor_cannot_write_through_is_still_the_final_mode() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("mode");
    let l = layer(&[E::File("readonly", b"content", 0o444)]);
    run(s.path(), &[l]).unwrap();
    let p = s.path().join("readonly");
    assert_eq!(std::fs::read(&p).unwrap(), b"content");
    assert_eq!(
        std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
        0o444
    );
}

// ---------------------------------------------------------------- T-1315

/// TODO/extract.md T-1315. A blob whose bytes changed after pull is refused
/// with a digest mismatch naming the blob, and the tampered bytes are never
/// written: the manifest names the original digest and the stored blob no
/// longer matches it.
#[test]
fn a_blob_edited_after_pull_is_refused_and_never_written() {
    use std::io::Write;
    let s = Scratch::new("tamper");
    let store = podvm_image::Store::open(s.path().join("store")).unwrap();
    let bytes = layer(&[E::File("greeting", b"hello honest world", 0o644)]);
    let want = podvm_image::digest::Digest::of(&bytes);
    let (staged, mut f) = store.stage("tamper").unwrap();
    f.write_all(&bytes).unwrap();
    drop(f);
    store.commit(&staged, &want).unwrap();
    // One content byte flipped in the stored blob, as the issue's repro does
    // at blob offset 514: `hello` becomes `heLlo`.
    let blob = store.blob_path(&want);
    let mut stored = std::fs::read(&blob).unwrap();
    let at = stored
        .windows(b"hello".len())
        .position(|w| w == b"hello")
        .expect("the committed tar carries the greeting");
    stored[at + 2] = b'L';
    std::fs::write(&blob, &stored).unwrap();
    let manifest = podvm_image::oci::Manifest {
        schema_version: 2,
        media_type: "application/vnd.oci.image.manifest.v1+json".to_string(),
        config: podvm_image::oci::Descriptor {
            media_type: "application/vnd.oci.image.config.v1+json".to_string(),
            digest: want.to_string(),
            size: 0,
            platform: None,
            annotations: std::collections::HashMap::new(),
        },
        layers: vec![podvm_image::oci::Descriptor {
            media_type: "application/vnd.oci.image.layer.v1.tar".to_string(),
            digest: want.to_string(),
            size: bytes.len() as u64,
            platform: None,
            annotations: std::collections::HashMap::new(),
        }],
    };
    let digest = format!("sha256:{}", "7".repeat(64));
    let mut out = Vec::new();
    let Err(e) = crate::extract(&store, &manifest, &digest, &mut out) else {
        panic!("a tampered blob extracted cleanly");
    };
    let text = format!("{e}");
    assert!(text.contains("mismatch"), "{text}");
    assert!(text.contains(&want.to_string()), "{text}");
    let (rootfs, _) = crate::paths(&store, &digest);
    assert!(
        std::fs::read(rootfs.join("greeting")).is_err(),
        "the tampered bytes reached the rootfs"
    );
}
