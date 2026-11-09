use super::*;

fn scratch(name: &str) -> Store {
    let d = std::env::temp_dir().join(format!("podvm-layout-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    Store::open(d).unwrap()
}

fn rootfs_tar() -> Vec<u8> {
    let mut archive = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    let bytes = b"hi";
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(&mut header, "etc/hello", &bytes[..])
        .unwrap();
    archive.into_inner().unwrap()
}

/// T-1338: `load` stages inside the store, because the commit is a
/// rename and a rename across filesystems is EXDEV. A staging path
/// outside the store root is the defect, wherever /tmp lives.
#[test]
fn load_stages_inside_the_store() {
    let s = scratch("staging");
    let bytes = b"blob-bytes";
    let blob = stage_entry(&s, std::path::Path::new("x"), &mut &bytes[..]).unwrap();
    assert!(
        blob.path.starts_with(s.root()),
        "staging escaped the store: {}",
        blob.path.display()
    );
    let _ = std::fs::remove_dir_all(s.root());
}

/// T-1320: `import` builds a runnable record from a rootfs tar, with
/// the digest recorded and the layer readable back.
#[test]
fn import_builds_a_record_from_a_rootfs_tar() {
    let s = scratch("import");
    let tar_bytes = rootfs_tar();
    let tarball = std::env::temp_dir().join(format!("podvm-import-{}.tar", std::process::id()));
    std::fs::write(&tarball, &tar_bytes).unwrap();
    let record = import(&s, &tarball, Some("imp:v9")).unwrap();
    assert_eq!(record.repository, "docker.io/library/imp");
    assert_eq!(record.tag.as_deref(), Some("v9"));
    let manifest_digest = Digest::parse(&record.manifest_digest).unwrap();
    let manifest_bytes = s.read_blob(&manifest_digest).unwrap();
    let manifest: oci::Manifest = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(manifest.layers.len(), 1);
    let layer_digest = manifest.layers[0].parsed_digest().unwrap();
    assert_eq!(s.read_blob(&layer_digest).unwrap(), tar_bytes);
    let found = s.find_one("imp:v9").unwrap();
    assert_eq!(found.manifest_digest, record.manifest_digest);
    let _ = std::fs::remove_dir_all(s.root());
    let _ = std::fs::remove_file(&tarball);
}

/// T-1320: a compressed file is refused by name, because the record's
/// media type promises a plain tar.
#[test]
fn import_refuses_a_compressed_file_by_name() {
    let s = scratch("import-gz");
    let mut tar_bytes = rootfs_tar();
    tar_bytes[0] = 0x1f;
    tar_bytes[1] = 0x8b;
    let tarball = std::env::temp_dir().join(format!("podvm-import-gz-{}.tar", std::process::id()));
    std::fs::write(&tarball, &tar_bytes).unwrap();
    let e = import(&s, &tarball, None).unwrap_err();
    assert!(format!("{e}").contains("compressed"), "{e}");
    let _ = std::fs::remove_dir_all(s.root());
    let _ = std::fs::remove_file(&tarball);
}
