//! An offline bundle: a signed manifest and the files it names, and a
//! delta from a named base bundle.
//!
//! ```text
//! bundle/
//!   manifest         the words below, signed by the key in them
//!   manifest.sig     the signature, in hex
//!   data/<path>      each file of the manifest
//! ```
//!
//! The manifest is self-describing. Its first line names the form and its
//! version, the `id` is the SHA-256 of its file lines, and each file has
//! its SHA-256 and its size. A delta is a second directory with the same
//! manifest of the new bundle, a `delta` text that names the base `id` and
//! the adds, changes and deletes, and a `data` tree for the files that the
//! delta carries. [`apply_delta`] refuses a bundle whose `id` is not the
//! base of the delta, before it changes one file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pod_identity::{Identity, PublicKey};

use crate::Error;
use crate::hex;

const BUNDLE: &str = "pod-offline bundle 1";
const DELTA: &str = "pod-offline delta 1";

/// One file of a manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileEntry {
    pub hash: [u8; 32],
    pub size: u64,
}

/// The manifest of a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub key: PublicKey,
    pub id: String,
    pub files: BTreeMap<String, FileEntry>,
}

impl Manifest {
    /// The id of a set of files: the SHA-256 of each `path hash size` line
    /// in the order of the paths.
    pub fn compute_id(files: &BTreeMap<String, FileEntry>) -> String {
        let mut words = Vec::new();
        for (path, entry) in files {
            words.extend_from_slice(format!("{path} {} {}\n", hex::encode(&entry.hash), entry.size).as_bytes());
        }
        hex::encode(&hex::sha256(&words))
    }

    /// The text of the manifest, which the signature covers.
    pub fn text(&self) -> String {
        let mut text = format!("{BUNDLE}\nname: {}\nkey: {}\nid: {}\n", self.name, self.key.hex(), self.id);
        for (path, entry) in &self.files {
            text.push_str(&format!("file {} {} {path}\n", hex::encode(&entry.hash), entry.size));
        }
        text
    }
}

/// One operation of a delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Change,
    Delete,
}

impl Op {
    fn letter(self) -> char {
        match self {
            Op::Add => 'a',
            Op::Change => 'c',
            Op::Delete => 'd',
        }
    }
}

/// The words of a delta, and the new manifest that it brings.
#[derive(Debug, Clone)]
pub struct Delta {
    pub base: String,
    pub manifest: Manifest,
    pub ops: BTreeMap<String, Op>,
}

impl Delta {
    /// The text of the delta, which the signature covers.
    pub fn text(&self) -> String {
        let mut text = format!("{DELTA}\nbase: {}\n", self.base);
        for (path, op) in &self.ops {
            text.push_str(&format!("{} {path}\n", op.letter()));
        }
        text
    }
}

/// Write a bundle of `files` into `dir`, signed by `identity`. A path is
/// relative, with no `..`, and no two files have it.
pub fn write_bundle(
    dir: impl AsRef<Path>,
    identity: &Identity,
    name: &str,
    files: &[(String, Vec<u8>)],
) -> Result<Manifest, Error> {
    let dir = dir.as_ref();
    check_name(name)?;
    let mut entries = BTreeMap::new();
    for (path, body) in files {
        safe_path(path)?;
        if entries.contains_key(path) {
            return Err(Error::Manifest { why: format!("the path {path} comes twice") });
        }
        write_file(&data_path(dir, path), body)?;
        entries.insert(path.clone(), FileEntry { hash: hex::sha256(body), size: body.len() as u64 });
    }
    let manifest =
        Manifest { name: name.to_string(), key: identity.public(), id: Manifest::compute_id(&entries), files: entries };
    write_manifest(dir, identity, &manifest)?;
    Ok(manifest)
}

/// Read and verify the manifest of a bundle, and each file that it names.
pub fn read_bundle(dir: impl AsRef<Path>) -> Result<Manifest, Error> {
    let dir = dir.as_ref();
    let manifest = read_manifest(dir)?;
    verify_files(dir, &manifest)?;
    Ok(manifest)
}

/// Check each file of `manifest` under `dir` against its hash and size.
pub fn verify_files(dir: impl AsRef<Path>, manifest: &Manifest) -> Result<(), Error> {
    let dir = dir.as_ref();
    for (path, entry) in &manifest.files {
        let bytes = std::fs::read(data_path(dir, path)).map_err(|e| Error::file(data_path(dir, path), e))?;
        if bytes.len() as u64 != entry.size {
            let why = format!("the size is {}, and the manifest says {}", bytes.len(), entry.size);
            return Err(Error::Manifest { why: format!("{path}: {why}") });
        }
        let hash = hex::sha256(&bytes);
        if hash != entry.hash {
            return Err(Error::HashMismatch {
                path: data_path(dir, path),
                expected: hex::encode(&entry.hash),
                found: hex::encode(&hash),
            });
        }
    }
    Ok(())
}

/// Make the delta from `previous` to `name` with `files`, and sign it and
/// the new manifest in `dir`. The files that the delta carries are the
/// added and the changed ones.
pub fn write_delta(
    dir: impl AsRef<Path>,
    identity: &Identity,
    previous: &Manifest,
    name: &str,
    files: &[(String, Vec<u8>)],
) -> Result<Delta, Error> {
    let dir = dir.as_ref();
    check_name(name)?;
    let mut next = BTreeMap::new();
    let mut contents = BTreeMap::new();
    for (path, body) in files {
        safe_path(path)?;
        if next.contains_key(path) {
            return Err(Error::Manifest { why: format!("the path {path} comes twice") });
        }
        next.insert(path.clone(), FileEntry { hash: hex::sha256(body), size: body.len() as u64 });
        contents.insert(path.clone(), body);
    }
    let manifest =
        Manifest { name: name.to_string(), key: identity.public(), id: Manifest::compute_id(&next), files: next };
    let mut ops = BTreeMap::new();
    for (path, entry) in &manifest.files {
        match previous.files.get(path) {
            None => {
                ops.insert(path.clone(), Op::Add);
            }
            Some(old) if old != entry => {
                ops.insert(path.clone(), Op::Change);
            }
            Some(_) => {}
        }
    }
    for path in previous.files.keys() {
        if !manifest.files.contains_key(path) {
            ops.insert(path.clone(), Op::Delete);
        }
    }
    let delta = Delta { base: previous.id.clone(), manifest, ops };
    for (path, op) in &delta.ops {
        if *op != Op::Delete {
            let body = contents.get(path).ok_or_else(|| Error::Manifest { why: format!("no body for {path}") })?;
            write_file(&data_path(dir, path), body)?;
        }
    }
    let text = delta.text();
    write_file(&dir.join("delta"), text.as_bytes())?;
    write_file(&dir.join("delta.sig"), signature(identity, text.as_bytes()).as_bytes())?;
    write_manifest(dir, identity, &delta.manifest)?;
    Ok(delta)
}

/// Apply the delta in `delta_dir` to the bundle in `base_dir`. The base
/// must be the bundle that the delta names. The manifest of the new bundle
/// is written last, so a half-applied delta keeps the old manifest.
pub fn apply_delta(base_dir: impl AsRef<Path>, delta_dir: impl AsRef<Path>) -> Result<Manifest, Error> {
    let base_dir = base_dir.as_ref();
    let delta_dir = delta_dir.as_ref();
    let previous = read_bundle(base_dir)?;
    let delta = read_delta(delta_dir)?;
    if delta.base != previous.id {
        return Err(Error::WrongBase { expected: delta.base, found: previous.id });
    }
    check_ops(&previous, &delta)?;
    for (path, op) in &delta.ops {
        match op {
            Op::Delete => {
                let file = data_path(base_dir, path);
                match std::fs::remove_file(&file) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(Error::file(file, e)),
                }
            }
            Op::Add | Op::Change => {
                let body = std::fs::read(data_path(delta_dir, path))
                    .map_err(|e| Error::file(data_path(delta_dir, path), e))?;
                let entry = delta.manifest.files.get(path).ok_or_else(|| Error::Manifest {
                    why: format!("the delta changes {path}, and its manifest does not name it"),
                })?;
                if hex::sha256(&body) != entry.hash || body.len() as u64 != entry.size {
                    return Err(Error::HashMismatch {
                        path: data_path(delta_dir, path),
                        expected: hex::encode(&entry.hash),
                        found: hex::encode(&hex::sha256(&body)),
                    });
                }
                write_file(&data_path(base_dir, path), &body)?;
            }
        }
    }
    verify_files(base_dir, &delta.manifest)?;
    // The signature of the new manifest is the one that the delta carries:
    // this end holds no key to sign it again.
    let manifest = read_text(&delta_dir.join("manifest"))?;
    let signature = read_text(&delta_dir.join("manifest.sig"))?;
    write_file(&base_dir.join("manifest"), manifest.as_bytes())?;
    write_file(&base_dir.join("manifest.sig"), signature.as_bytes())?;
    Ok(delta.manifest)
}

/// Each operation of the delta must follow from the two manifests, and the
/// two must name the same set of files at the end.
fn check_ops(previous: &Manifest, delta: &Delta) -> Result<(), Error> {
    for (path, op) in &delta.ops {
        let was = previous.files.contains_key(path);
        let now = delta.manifest.files.contains_key(path);
        let ok = match op {
            Op::Add => !was && now,
            Op::Change => was && now,
            Op::Delete => was && !now,
        };
        if !ok {
            return Err(Error::Manifest { why: format!("the operation {op:?} of {path} does not fit the base") });
        }
    }
    for path in delta.manifest.files.keys() {
        if !previous.files.contains_key(path) && !delta.ops.contains_key(path) {
            return Err(Error::Manifest { why: format!("the delta does not add {path}") });
        }
    }
    for path in previous.files.keys() {
        if !delta.manifest.files.contains_key(path) && delta.ops.get(path) != Some(&Op::Delete) {
            return Err(Error::Manifest { why: format!("the delta does not delete {path}") });
        }
    }
    Ok(())
}

fn read_delta(dir: &Path) -> Result<Delta, Error> {
    let text = read_text(&dir.join("delta"))?;
    let sig = read_signature(&dir.join("delta.sig"))?;
    let mut lines = text.lines();
    if lines.next() != Some(DELTA) {
        return Err(Error::Manifest { why: format!("the delta does not start with {DELTA:?}") });
    }
    let base = lines
        .next()
        .and_then(|line| line.strip_prefix("base: "))
        .ok_or_else(|| Error::Manifest { why: "the delta names no base".into() })?
        .to_string();
    let mut ops = BTreeMap::new();
    for line in lines {
        let (letter, path) = line.split_once(' ').ok_or_else(|| Error::Manifest { why: "a bad delta line".into() })?;
        let op = match letter {
            "a" => Op::Add,
            "c" => Op::Change,
            "d" => Op::Delete,
            _ => return Err(Error::Manifest { why: format!("the operation {letter:?} is not a, c or d") }),
        };
        safe_path(path)?;
        if ops.insert(path.to_string(), op).is_some() {
            return Err(Error::Manifest { why: format!("the path {path} comes twice") });
        }
    }
    let manifest = read_manifest(dir)?;
    if !manifest.key.verifies(text.as_bytes(), &sig) {
        return Err(Error::Proof);
    }
    Ok(Delta { base, manifest, ops })
}

fn read_manifest(dir: &Path) -> Result<Manifest, Error> {
    let text = read_text(&dir.join("manifest"))?;
    let sig = read_signature(&dir.join("manifest.sig"))?;
    let mut lines = text.lines();
    if lines.next() != Some(BUNDLE) {
        return Err(Error::Manifest { why: format!("the manifest does not start with {BUNDLE:?}") });
    }
    let mut name = None;
    let mut key = None;
    let mut id = None;
    let mut files = BTreeMap::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("name: ") {
            name = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("key: ") {
            key = Some(PublicKey::parse(rest).ok_or_else(|| Error::Manifest { why: "a bad key".into() })?);
        } else if let Some(rest) = line.strip_prefix("id: ") {
            id = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("file ") {
            let mut parts = rest.splitn(3, ' ');
            let hash =
                parts.next().and_then(hex::decode).ok_or_else(|| Error::Manifest { why: "a bad hash".into() })?;
            let size = parts
                .next()
                .and_then(|text| text.parse::<u64>().ok())
                .ok_or_else(|| Error::Manifest { why: "a bad size".into() })?;
            let path = parts.next().ok_or_else(|| Error::Manifest { why: "a file line has no path".into() })?;
            safe_path(path)?;
            let hash: [u8; 32] =
                hash.try_into().map_err(|_| Error::Manifest { why: "a hash is not 32 bytes".into() })?;
            if files.insert(path.to_string(), FileEntry { hash, size }).is_some() {
                return Err(Error::Manifest { why: format!("the path {path} comes twice") });
            }
        } else {
            return Err(Error::Manifest { why: format!("the line {line:?} is not of a manifest") });
        }
    }
    let name = name.ok_or_else(|| Error::Manifest { why: "the manifest has no name".into() })?;
    check_name(&name)?;
    let key = key.ok_or_else(|| Error::Manifest { why: "the manifest has no key".into() })?;
    let id = id.ok_or_else(|| Error::Manifest { why: "the manifest has no id".into() })?;
    if Manifest::compute_id(&files) != id {
        return Err(Error::Manifest { why: "the id is not the hash of the file lines".into() });
    }
    if !key.verifies(text.as_bytes(), &sig) {
        return Err(Error::Proof);
    }
    Ok(Manifest { name, key, id, files })
}

fn write_manifest(dir: &Path, identity: &Identity, manifest: &Manifest) -> Result<(), Error> {
    let text = manifest.text();
    write_file(&dir.join("manifest"), text.as_bytes())?;
    write_file(&dir.join("manifest.sig"), signature(identity, text.as_bytes()).as_bytes())?;
    Ok(())
}

fn signature(identity: &Identity, text: &[u8]) -> String {
    hex::encode(&identity.sign(text))
}

/// The name of a bundle is one line with no control byte.
fn check_name(name: &str) -> Result<(), Error> {
    if name.is_empty() {
        return Err(Error::Manifest { why: "the name is empty".into() });
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(Error::Manifest { why: "the name holds a control byte".into() });
    }
    Ok(())
}

fn read_signature(path: &Path) -> Result<[u8; 64], Error> {
    let text = read_text(path)?;
    let raw = hex::decode(text.trim()).ok_or_else(|| Error::Manifest { why: "the signature is not hex".into() })?;
    raw.try_into().map_err(|_| Error::Manifest { why: "the signature is not 64 bytes".into() })
}

fn read_text(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|e| Error::file(path, e))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::file(parent, e))?;
    }
    std::fs::write(path, bytes).map_err(|e| Error::file(path, e))
}

fn data_path(dir: &Path, path: &str) -> PathBuf {
    dir.join("data").join(path)
}

/// A path of a bundle is relative, with no `..` and no empty part.
fn safe_path(path: &str) -> Result<(), Error> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return Err(Error::Manifest { why: format!("the path {path:?} is not relative") });
    }
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(Error::Manifest { why: format!("the path {path:?} has a bad part") });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pod-offline-bundle-unit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_id_is_the_hash_of_the_file_lines() {
        let mut files = BTreeMap::new();
        files.insert("a".to_string(), FileEntry { hash: [1; 32], size: 2 });
        let id = Manifest::compute_id(&files);
        assert_eq!(id.len(), 64);
        files.insert("b".to_string(), FileEntry { hash: [2; 32], size: 3 });
        assert_ne!(Manifest::compute_id(&files), id);
    }

    #[test]
    fn a_path_with_a_parent_part_is_refused() {
        assert!(safe_path("a/b").is_ok());
        for bad in ["", "/a", "../a", "a/../b", "a//b", "a\\b"] {
            assert!(safe_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_delta_names_each_add_a_change_and_a_delete() {
        let dir = seed("delta");
        let one = write_bundle(
            dir.join("one"),
            &Identity::from_seed(&[1; 32]),
            "one",
            &[("a".to_string(), b"a1".to_vec()), ("b".to_string(), b"b1".to_vec())],
        )
        .unwrap();
        let delta = write_delta(
            dir.join("d"),
            &Identity::from_seed(&[1; 32]),
            &one,
            "two",
            &[("a".to_string(), b"a2".to_vec()), ("c".to_string(), b"c1".to_vec())],
        )
        .unwrap();
        assert_eq!(delta.ops.get("a"), Some(&Op::Change));
        assert_eq!(delta.ops.get("b"), Some(&Op::Delete));
        assert_eq!(delta.ops.get("c"), Some(&Op::Add));
        let _ = std::fs::remove_dir_all(dir);
    }
}
