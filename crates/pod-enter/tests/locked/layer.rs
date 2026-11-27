//! The cases of an overlay volume and of the devices of the host
//! (PodUtils/podvm#689), on each path of `locked.rs`.
//!
//! An overlay volume at `/ov`: the payload makes a file, changes one and
//! removes one. The lower layer stays byte for byte; the upper directory
//! holds the new file, the changed file and a whiteout; the mount is locked
//! as each other mount of the rung. A host path in the upper directory is
//! refused by name, and the payload does not run.
//!
//! The devices: where a user namespace owns the mount namespace of the rung
//! (the one of the rung, or the one of the caller), the regular files of
//! the image at `/dev/null` and `/dev/urandom` become the host's nodes, and
//! `/dev/ptmx` opens. As host root the image keeps its files.

use std::collections::BTreeMap;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::run::{EINVAL, ENOENT, expect, host_root, plan, start, tree};
use pod_enter::{OverlayLayers, RootDir, VolumeBind, spawn_namespace};

/// Each file and directory under `dir`: its path, its mode, and its bytes.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (u32, Vec<u8>)> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().map(Result::unwrap) {
            let m = std::fs::symlink_metadata(e.path()).unwrap();
            let bytes = if m.is_file() { std::fs::read(e.path()).unwrap() } else { Vec::new() };
            if m.is_dir() {
                todo.push(e.path());
            }
            out.insert(e.path().strip_prefix(dir).unwrap().to_path_buf(), (m.mode(), bytes));
        }
    }
    out
}

/// The lower layer, and an upper and a work directory beside it.
fn layers(dir: &Path) -> (PathBuf, OverlayLayers) {
    let lower = dir.join("lower");
    std::fs::create_dir_all(lower.join("sub")).unwrap();
    for (f, b) in [("keep", "keep"), ("edit", "old"), ("gone", "gone"), ("sub/deep", "deep")] {
        std::fs::write(lower.join(f), b).unwrap();
    }
    let (upper, work) = (dir.join("upper"), dir.join("work"));
    for d in [&upper, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::set_permissions(&upper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let text = |p: &Path| p.to_str().unwrap().to_string();
    (lower, OverlayLayers { upper: text(&upper), work: text(&work) })
}

fn overlay(lower: &Path, layers: OverlayLayers) -> VolumeBind {
    VolumeBind {
        host: lower.to_str().unwrap().to_string(),
        guest: "/ov".to_string(),
        overlay: Some(layers),
        ..VolumeBind::default()
    }
}

/// A write, a change and a removal in an overlay volume.
pub fn overlay_volume(pid: bool) -> Result<(), String> {
    let t = tree(if pid { "ov-pid" } else { "ov" });
    let (lower, l) = layers(&t.dir);
    let upper = PathBuf::from(&l.upper);
    let before = snapshot(&lower);
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let want = [
        ("put:/ov/made", 3),
        ("change:/ov/edit", 7),
        ("unlink:/ov/gone", 0),
        ("stat:/ov/gone", ENOENT),
        ("stat:/ov/keep", 0),
        ("put:/ov/sub/new", 3),
        ("umount:/ov", EINVAL),
    ];
    let ops: Vec<String> = want.iter().map(|(op, _)| op.to_string()).collect();
    let mut p = plan(&t, &ops, pid, false);
    p.volumes.push(overlay(&lower, l.clone()));
    let mut err = Vec::new();
    let r = start(&mut p, |p| spawn_namespace(&root, p, &mut err))?;
    let out = expect(&r, &want);
    let code = r.end()?;
    let err = String::from_utf8_lossy(&err);
    out.map_err(|e| format!("{e}: {err}"))?;
    if code != 0 {
        return Err(format!("exit {code}: {err}"));
    }
    if snapshot(&lower) != before {
        return Err("a write of the payload changed the lower layer".to_string());
    }
    let read = |f: &str| std::fs::read_to_string(upper.join(f)).unwrap_or_default();
    let gone = std::fs::symlink_metadata(upper.join("gone"));
    let whiteout = gone.is_ok_and(|m| m.file_type().is_char_device() && m.rdev() == 0);
    if read("made") != "new" || read("edit") != "changed" || read("sub/new") != "new" || !whiteout {
        return Err(format!("the upper layer: {:?}", snapshot(&upper).keys().collect::<Vec<_>>()));
    }
    if upper.join("keep").exists() {
        return Err("a file that the payload only read was copied up".to_string());
    }
    // A host path in the upper directory: refused by name before the fork.
    let inside = upper.join("inner");
    std::fs::create_dir_all(&inside).map_err(|e| e.to_string())?;
    let mut p = plan(&t, &[], pid, false);
    p.volumes.push(overlay(&inside, l));
    match start(&mut p, |p| spawn_namespace(&root, p, &mut std::io::sink())) {
        Err(e) if e.contains(":/ov:overlay has layers that overlap") => Ok(()),
        Err(e) => Err(format!("a host path in the upper directory: {e}")),
        Ok(r) => {
            let _ = r.end();
            Err("the payload ran with a host path in the upper directory".to_string())
        }
    }
}

/// The regular files of the image at `/dev/null` and `/dev/urandom` become
/// the host's nodes, and `/dev/ptmx` opens, where a user namespace owns the
/// mount namespace.
pub fn devices(pid: bool) -> Result<(), String> {
    let t = tree(if pid { "dev-pid" } else { "dev" });
    for f in ["null", "urandom"] {
        std::fs::write(t.image.join("dev").join(f), "").map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(t.image.join("dev/pts")).map_err(|e| e.to_string())?;
    let root = RootDir::open(t.image.to_str().unwrap()).map_err(|e| e.to_string())?;
    let want: Vec<(&str, i64)> = if !host_root() {
        vec![("chr:/dev/null", 1), ("chr:/dev/urandom", 1), ("rdwr:/dev/ptmx", 0)]
    } else {
        vec![("chr:/dev/null", 0), ("chr:/dev/urandom", 0)]
    };
    let ops: Vec<String> = want.iter().map(|(op, _)| op.to_string()).collect();
    let mut err = Vec::new();
    let r = start(&mut plan(&t, &ops, pid, false), |p| spawn_namespace(&root, p, &mut err))?;
    let out = expect(&r, &want);
    let code = r.end()?;
    out.map_err(|e| format!("{e}: {}", String::from_utf8_lossy(&err)))?;
    if code != 0 {
        return Err(format!("exit {code}: {}", String::from_utf8_lossy(&err)));
    }
    Ok(())
}
