use super::*;

fn tree(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-enter-stage-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src/bin")).unwrap();
    d
}

/// Files, directories and symlinks replicate whole, with link
/// targets unrewritten.
#[test]
fn a_tree_replicates_whole() {
    let d = tree("whole");
    std::fs::write(d.join("src/bin/prog"), b"\x7fELF-tool").unwrap();
    std::fs::write(d.join("src/note.txt"), b"hi").unwrap();
    std::os::unix::fs::symlink("prog", d.join("src/bin/sh")).unwrap();
    std::os::unix::fs::symlink("/abs/target", d.join("src/abs")).unwrap();
    let dst = d.join("dst");
    let n = copy_tree(&d.join("src"), &dst).unwrap();
    assert_eq!(n, 5, "bin, prog, note.txt, sh, abs");
    assert_eq!(std::fs::read(dst.join("bin/prog")).unwrap(), b"\x7fELF-tool");
    assert_eq!(std::fs::read_link(dst.join("bin/sh")).unwrap(), std::path::Path::new("prog"));
    assert_eq!(std::fs::read_link(dst.join("abs")).unwrap(), std::path::Path::new("/abs/target"));
    let _ = std::fs::remove_dir_all(&d);
}

/// A fifo refuses naming the path: the staged tree would not be the
/// image's if special files silently dropped out of it.
#[test]
#[cfg(unix)]
fn a_special_file_is_a_sentence() {
    pod_testkit::need_tool!("mkfifo");
    let d = tree("fifo");
    let fifo = d.join("src/pipe");
    let st = std::process::Command::new("mkfifo").arg(&fifo).status().expect("mkfifo runs on the test machine");
    assert!(st.success(), "the fixture fifo could not be made");
    let e = copy_tree(&d.join("src"), &d.join("dst")).unwrap_err();
    assert_eq!(e.path, fifo, "{e}");
    assert!(e.why.contains("not a file, directory or symlink"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A staging directory that already exists refuses rather than
/// merging: a stale tree must never donate files to a run.
#[test]
fn an_existing_destination_refuses() {
    let d = tree("exists");
    std::fs::create_dir_all(d.join("dst")).unwrap();
    let e = copy_tree(&d.join("src"), &d.join("dst")).unwrap_err();
    assert!(e.why.contains("already exists"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A source that is not a directory refuses.
#[test]
fn a_missing_source_refuses() {
    let d = tree("missing");
    let e = copy_tree(&d.join("nope"), &d.join("dst")).unwrap_err();
    assert!(e.why.contains("not a directory"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The digest's colon spells an underscore, like the extraction
/// paths: one digest, one directory, everywhere.
#[test]
fn the_cache_dir_spells_the_digest() {
    let dir = cache_dir(Path::new("/store"), "sha256:abc123");
    assert_eq!(dir, Path::new("/store/cache/sha256_abc123"));
}

/// The first stage copies and marks; the second reuses the marked
/// tree without copying again.
#[test]
fn the_cache_copies_once_then_reuses() {
    let d = tree("cache");
    std::fs::write(d.join("src/bin/prog"), b"\x7fELF").unwrap();
    let store = d.join("store");
    let first = stage_cache(&Names::DEFAULT, &store, "sha256:one", &d.join("src")).unwrap();
    assert_eq!(std::fs::read(first.join("bin/prog")).unwrap(), b"\x7fELF");
    assert_eq!(std::fs::read_to_string(first.join(Names::DEFAULT.cache_marker)).unwrap(), "sha256:one");
    // A sentinel the second stage must not copy again.
    std::fs::write(d.join("src/bin/late"), b"late").unwrap();
    let second = stage_cache(&Names::DEFAULT, &store, "sha256:one", &d.join("src")).unwrap();
    assert_eq!(first, second);
    assert!(!second.join("bin/late").exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// A marker naming another digest rebuilds rather than entering a
/// tree of unknown provenance.
#[test]
fn a_foreign_marker_rebuilds() {
    let d = tree("foreign");
    std::fs::write(d.join("src/bin/prog"), b"\x7fELF").unwrap();
    let store = d.join("store");
    let first = stage_cache(&Names::DEFAULT, &store, "sha256:one", &d.join("src")).unwrap();
    std::fs::write(first.join(Names::DEFAULT.cache_marker), "sha256:other").unwrap();
    let second = stage_cache(&Names::DEFAULT, &store, "sha256:one", &d.join("src")).unwrap();
    assert_eq!(first, second);
    assert_eq!(std::fs::read_to_string(second.join(Names::DEFAULT.cache_marker)).unwrap(), "sha256:one");
    assert_eq!(std::fs::read(second.join("bin/prog")).unwrap(), b"\x7fELF");
    let _ = std::fs::remove_dir_all(&d);
}

/// A source that is not a directory refuses before the store is
/// touched: no rung stages what nothing resolved, and no rung
/// litters the store refusing it.
#[test]
fn a_tmpfs_over_a_missing_source_refuses_before_touching_the_store() {
    let d = tree("tmpfs-missing");
    let store = d.join("store");
    let e = stage_tmpfs(&store, &d.join("nope")).unwrap_err();
    assert!(e.why.contains("not a directory"), "{e}");
    assert!(!store.exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// The tmpfs rung stages onto a mount where one holds, and refuses
/// naming the mount where one does not. Either arm cleans behind
/// itself: entry carries the payload bytes and releases to nothing,
/// refusal leaves no staging directory.
#[test]
fn tmpfs_stages_onto_a_mount_or_refuses_naming_it() {
    let d = tree("tmpfs");
    std::fs::write(d.join("src/bin/prog"), b"\x7fELF").unwrap();
    let store = d.join("store");
    match stage_tmpfs(&store, &d.join("src")) {
        Ok(dir) => {
            assert_eq!(std::fs::read(dir.join("bin/prog")).unwrap(), b"\x7fELF");
            release_tmpfs(&dir);
            assert!(!dir.exists());
        }
        Err(r) => {
            assert!(r.why.contains("tmpfs mount was refused"), "{r}");
            let staged: Vec<_> = std::fs::read_dir(store.join("tmpfs"))
                .map(|rd| rd.filter_map(|e| e.ok()).collect())
                .unwrap_or_default();
            assert!(staged.is_empty(), "a refused mount left staging behind");
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// Releasing an unmounted directory removes it: the removal half of
/// the release does not depend on the unmount half.
#[test]
fn releasing_an_unmounted_directory_removes_it() {
    let d = tree("release");
    let dir = d.join("plain");
    std::fs::create_dir_all(&dir).unwrap();
    release_tmpfs(&dir);
    assert!(!dir.exists());
    let _ = std::fs::remove_dir_all(&d);
}
