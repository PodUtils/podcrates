//! TODO/image.md T-1320. Images move without a registry: save, load, import.
//!
//! One interchange shape: a tarball holding an OCI image layout
//! (`oci-layout`, `index.json`, `blobs/sha256/<hex>`). `save` writes the
//! store's own bytes under those names; `load` verifies every blob
//! against its descriptor on the way in, the same verification `pull`
//! performs, and registers the record; `import` builds a runnable record
//! from a plain rootfs tar by synthesizing the config and manifest the
//! store's shape requires.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::digest::Digest;
use crate::error::{Error, Result};
use crate::oci;
use crate::reference::Reference;
use crate::store::{Record, StagedFile, Store};

/// What `save` wrote, for the caller's report line.
pub struct Saved {
    pub record: Record,
    pub blobs: usize,
    pub bytes: u64,
}

const OCI_LAYOUT: &str = r#"{"imageLayoutVersion":"1.0.0"}"#;
const REF_ANNOTATION: &str = "org.opencontainers.image.ref.name";
const MEDIA_UNTAR: &str = "application/vnd.oci.image.layer.v1.tar";

/// The tarball entries `load` accepts, and nothing else. An allowlist
/// rather than a traversal check: a foreign tarball with extra paths is
/// refused by name instead of partially read.
fn allowed_entry(name: &str) -> bool {
    if name == "oci-layout" || name == "index.json" {
        return true;
    }
    let Some(hex) = name.strip_prefix("blobs/sha256/") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `podvm save`: the record's blobs as an OCI-layout tarball on `out`.
/// Blobs come from `read_blob`, so they are verified on the way out the
/// same way every other read is. ⚠ Blob bytes travel through memory: the
/// store's read API is memory-based, and save reuses it rather than
/// growing a second read path around verification.
pub fn save(store: &Store, want: &str, out: &mut dyn Write) -> Result<Saved> {
    let record = store.find_one(want)?;
    let manifest_digest = Digest::parse(&record.manifest_digest)?;
    let manifest_bytes = store.read_blob(&manifest_digest)?;
    let manifest: oci::Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| Error::Oci(format!("the stored manifest does not parse: {e}")))?;
    let mut blobs: Vec<(String, Vec<u8>)> = vec![(manifest_digest.to_string(), manifest_bytes)];
    let manifest_len = blobs[0].1.len();
    let config_digest = manifest.config.parsed_digest()?;
    blobs.push((config_digest.to_string(), store.read_blob(&config_digest)?));
    for layer in &manifest.layers {
        let digest = layer.parsed_digest()?;
        blobs.push((digest.to_string(), store.read_blob(&digest)?));
    }
    blobs.sort_by(|a, b| a.0.cmp(&b.0));
    blobs.dedup_by(|a, b| a.0 == b.0);

    let refname = match &record.tag {
        Some(tag) => format!("{}:{tag}", record.repository),
        None => record.repository.clone(),
    };
    let index = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": oci::MEDIA_OCI_INDEX,
        "manifests": [{
            "mediaType": manifest.media_type,
            "digest": manifest_digest.to_string(),
            "size": manifest_len,
            "annotations": { REF_ANNOTATION: refname },
        }],
    });
    let index_bytes = serde_json::to_vec(&index)
        .map_err(|e| Error::Oci(format!("the save index does not render: {e}")))?;

    let mut archive = tar::Builder::new(out);
    append(&mut archive, "oci-layout", OCI_LAYOUT.as_bytes())?;
    append(&mut archive, "index.json", &index_bytes)?;
    let mut bytes = 0u64;
    for (digest, blob) in &blobs {
        let hex = digest.trim_start_matches("sha256:");
        append(&mut archive, &format!("blobs/sha256/{hex}"), blob)?;
        bytes += blob.len() as u64;
    }
    let count = blobs.len();
    archive
        .into_inner()
        .map_err(|e| Error::Oci(format!("the save tarball does not finish: {e}")))?;
    Ok(Saved {
        record,
        blobs: count,
        bytes,
    })
}

fn append(archive: &mut tar::Builder<&mut dyn Write>, path: &str, data: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(&mut header, path, data)
        .map_err(|e| Error::Oci(format!("the save tarball does not take {path}: {e}")))?;
    Ok(())
}

/// A blob streamed out of the tarball into a staging file, hashed on the
/// way. The name it arrived under is untrusted; only the computed digest
/// decides where it goes.
struct StagedBlob {
    digest: Digest,
    path: PathBuf,
    // ⭐ The in-store staging handle, held until commit or cleanup. Its
    // exclusive lock is what tells `sweep_staging` this file is being
    // written rather than abandoned (invariant I5).
    _guard: StagedFile,
}

/// `podvm load`: an OCI-layout tarball into the store. Every blob is
/// hashed on the way in and matched against a descriptor before it is
/// committed, so a tarball whose bytes changed in transit is refused the
/// way a bad pull is. Staging files are removed on every path out.
pub fn load(store: &Store, src: &Path) -> Result<Record> {
    let file = std::fs::File::open(src).map_err(|e| Error::io(src.display().to_string(), e))?;
    let mut archive = tar::Archive::new(file);
    let mut index_bytes: Option<Vec<u8>> = None;
    let mut staged: Vec<StagedBlob> = Vec::new();
    let outcome = load_inner(store, src, &mut archive, &mut index_bytes, &mut staged);
    for blob in &staged {
        let _ = std::fs::remove_file(&blob.path);
    }
    outcome
}

fn load_inner(
    store: &Store,
    src: &Path,
    archive: &mut tar::Archive<std::fs::File>,
    index_bytes: &mut Option<Vec<u8>>,
    staged: &mut Vec<StagedBlob>,
) -> Result<Record> {
    let entries = archive
        .entries()
        .map_err(|e| Error::Oci(format!("{} is not a tarball: {e}", src.display())))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| Error::Oci(format!("{} does not unpack: {e}", src.display())))?;
        let path = entry.path().map_err(|e| {
            Error::Oci(format!(
                "{} has an unreadable entry name: {e}",
                src.display()
            ))
        })?;
        let name = path.to_string_lossy().replace('\\', "/");
        let name = name.trim_start_matches("./").to_string();
        if !allowed_entry(&name) {
            return Err(Error::Oci(format!(
                "{} carries {name:?}, which is not part of a single-image OCI layout. \
                 podvm loads one image per tarball",
                src.display()
            )));
        }
        if name == "oci-layout" {
            continue;
        }
        if name == "index.json" {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| Error::Oci(format!("{} does not read: {e}", src.display())))?;
            *index_bytes = Some(bytes);
            continue;
        }
        staged.push(stage_entry(store, src, &mut entry)?);
    }
    let Some(index_bytes) = index_bytes else {
        return Err(Error::Oci(format!(
            "{} has no index.json, so it names no image",
            src.display()
        )));
    };
    let index: oci::Index = serde_json::from_slice(index_bytes).map_err(|e| {
        Error::Oci(format!(
            "{} has an index that does not parse: {e}",
            src.display()
        ))
    })?;
    if index.manifests.len() != 1 {
        return Err(Error::Oci(format!(
            "{} names {} manifests; podvm loads one image per tarball",
            src.display(),
            index.manifests.len()
        )));
    }
    let descriptor = &index.manifests[0];
    let manifest_digest = descriptor.parsed_digest()?;
    commit_staged(store, staged, &manifest_digest, src)?;
    let manifest_bytes = store.read_blob(&manifest_digest)?;
    let manifest: oci::Manifest = serde_json::from_slice(&manifest_bytes).map_err(|e| {
        Error::Oci(format!(
            "{} has a manifest that does not parse: {e}",
            src.display()
        ))
    })?;
    let config_digest = manifest.config.parsed_digest()?;
    commit_staged(store, staged, &config_digest, src)?;
    let config_bytes = store.read_blob(&config_digest)?;
    let config: oci::Config = serde_json::from_slice(&config_bytes).map_err(|e| {
        Error::Oci(format!(
            "{} has a config that does not parse: {e}",
            src.display()
        ))
    })?;
    for layer in &manifest.layers {
        commit_staged(store, staged, &layer.parsed_digest()?, src)?;
    }
    if !staged.is_empty() {
        return Err(Error::Oci(format!(
            "{} carries {} blob(s) the manifest does not name; refusing rather than \
             storing bytes no record points at",
            src.display(),
            staged.len()
        )));
    }
    let refname = descriptor
        .annotations
        .get(REF_ANNOTATION)
        .cloned()
        .unwrap_or_else(|| manifest_digest.to_string());
    let reference = Reference::parse(&refname)?;
    let platform = if config.os.is_empty() || config.architecture.is_empty() {
        // ⚠ Degenerate configs exist; the host platform is the honest
        // fallback, and the record says exactly that.
        crate::platform::Platform::host().to_string()
    } else {
        format!("{}/{}", config.os, config.architecture)
    };
    let record = crate::store::record_of(
        &reference,
        &manifest_digest,
        &descriptor.media_type,
        &manifest_digest,
        &manifest,
        &config,
        &platform,
    )?;
    store.put_record(record.clone())?;
    Ok(record)
}

/// Stream one tarball entry to a staging file, hashing on the way.
///
/// ⭐ The staging file lives inside the store (`Store::stage`), because
/// the commit is a `rename(2)` and a rename across filesystems is
/// `EXDEV`: staging into `temp_dir()` broke `load` wherever /tmp and
/// the store sit on different filesystems (found by the T-1338 perf
/// harness on the lane: tmpfs /tmp against an overlayfs store).
/// Blobs stream to files, so the tarball is never fully in memory.
fn stage_entry(store: &Store, src: &Path, entry: &mut dyn Read) -> Result<StagedBlob> {
    use sha2::Digest as _;
    let (path, mut out) = store.stage("load")?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = entry
            .read(&mut buf)
            .map_err(|e| Error::Oci(format!("{} does not read: {e}", src.display())))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])
            .map_err(|e| Error::io(path.display().to_string(), e))?;
    }
    let hex = crate::digest::hex_of(&hasher.finalize());
    Ok(StagedBlob {
        digest: Digest::parse(&format!("sha256:{hex}"))?,
        path,
        _guard: out,
    })
}

/// Commit the staged blob whose computed digest matches `want` into the
/// store. A staged blob nobody names is left for the caller to refuse on.
/// The staging file is removed on every path through here.
fn commit_staged(
    store: &Store,
    staged: &mut Vec<StagedBlob>,
    want: &Digest,
    src: &Path,
) -> Result<()> {
    let Some(pos) = staged.iter().position(|b| b.digest == *want) else {
        return Err(Error::DigestMismatch {
            what: format!("tarball {}", src.display()),
            want: want.to_string(),
            got: "absent".to_string(),
        });
    };
    let blob = staged.remove(pos);
    // ⚠ The staging file already holds exactly the verified bytes, so it
    // commits directly: a second copy would double the I/O for no new
    // verification.
    store.commit(&blob.path, want)?;
    Ok(())
}

/// `podvm import`: a plain rootfs tar into a runnable record. The config
/// and manifest are synthesized around the tar's own digest; a compressed
/// file is refused by name rather than stored under a lie, because the
/// record's media type promises a plain tar.
pub fn import(store: &Store, tar: &Path, reference: Option<&str>) -> Result<Record> {
    let mut head = [0u8; 512];
    {
        let mut f =
            std::fs::File::open(tar).map_err(|e| Error::io(tar.display().to_string(), e))?;
        f.read_exact(&mut head)
            .map_err(|e| Error::Oci(format!("{} does not read: {e}", tar.display())))?;
    }
    if head.starts_with(&[0x1f, 0x8b]) || head.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        return Err(Error::Oci(format!(
            "{} is compressed; import takes a plain tar so the record's media type \
             stays true. Decompress it first",
            tar.display()
        )));
    }
    if &head[257..262] != b"ustar" {
        return Err(Error::Oci(format!(
            "{} is not a tar archive (no ustar magic); nothing runnable can be built from it",
            tar.display()
        )));
    }
    let (staged_path, mut staged_file) = store.stage("import")?;
    let layer_digest = stream_file_hashed(tar, &mut staged_file)?;
    drop(staged_file);
    store.commit(&staged_path, &layer_digest)?;
    let layer_len = std::fs::metadata(store.blob_path(&layer_digest))
        .map(|m| m.len())
        .map_err(|e| Error::io(tar.display().to_string(), e))?;

    let host = crate::platform::Platform::host();
    let created = crate::clock::now();
    let config_value = serde_json::json!({
        "architecture": host.arch,
        "os": "linux",
        "created": created,
        "rootfs": {"type": "layers", "diff_ids": [layer_digest.to_string()]},
        "config": {},
    });
    let config_bytes = serde_json::to_vec(&config_value)
        .map_err(|e| Error::Oci(format!("the import config does not render: {e}")))?;
    let config_digest = Digest::of(&config_bytes);
    let manifest_value = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": oci::MEDIA_OCI_MANIFEST,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": config_digest.to_string(),
            "size": config_bytes.len(),
        },
        "layers": [{
            "mediaType": MEDIA_UNTAR,
            "digest": layer_digest.to_string(),
            "size": layer_len,
        }],
    });
    let manifest_bytes = serde_json::to_vec(&manifest_value)
        .map_err(|e| Error::Oci(format!("the import manifest does not render: {e}")))?;
    let manifest_digest = Digest::of(&manifest_bytes);
    let manifest: oci::Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| Error::Oci(format!("the import manifest does not parse: {e}")))?;
    let config: oci::Config = serde_json::from_slice(&config_bytes)
        .map_err(|e| Error::Oci(format!("the import config does not parse: {e}")))?;
    store.put_bytes(&config_bytes, &config_digest, "import config")?;
    store.put_bytes(&manifest_bytes, &manifest_digest, "import manifest")?;
    // ⚠ The reference is a name, never an authority: `imported` with no tag
    // where the caller named nothing. `Reference::parse` applies docker's
    // `:latest` default centrally, so nothing here invents its own.
    let reference = match reference {
        Some(r) => Reference::parse(r)?,
        None => Reference::parse("imported")?,
    };
    let record = crate::store::record_of(
        &reference,
        &manifest_digest,
        oci::MEDIA_OCI_MANIFEST,
        &manifest_digest,
        &manifest,
        &config,
        &format!("{}/{}", config.os, config.architecture),
    )?;
    store.put_record(record.clone())?;
    Ok(record)
}

/// Stream a file into an open staging handle, returning its digest.
fn stream_file_hashed(path: &Path, out: &mut dyn Write) -> Result<Digest> {
    use sha2::Digest as _;
    let mut f = std::fs::File::open(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| Error::io(path.display().to_string(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])
            .map_err(|e| Error::io(path.display().to_string(), e))?;
    }
    let hex = crate::digest::hex_of(&hasher.finalize());
    Digest::parse(&format!("sha256:{hex}"))
}

#[cfg(test)]
mod tests;
