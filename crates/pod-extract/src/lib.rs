//! `TOOL.md` section 6.3: layers, whiteouts, ownership sidecar, path safety.
//! Milestone M2, [`TODO/milestones.md`](../../../TODO/milestones.md) T-1103 and
//! [`TODO/extract.md`](../../../TODO/extract.md) T-0301 to T-0307.
//!
//! ⭐ **The single highest-risk component. Four separate tools in the corpus
//! stop here, all at the same wall, and the wall is `chown` to an id the user
//! namespace does not map.**
//!
//! Seven rules decide whether this crate is right rather than merely finished,
//! and each is one entry:
//!
//! 1. extraction is **in-process and at entry level**, never through system
//!    `tar` and never through a library's `unpack()` ([`apply`], T-0301);
//! 2. ownership is **never restored by default**; the image's intent goes in a
//!    sidecar keyed by path ([`sidecar`], T-0302);
//! 3. a whiteout is matched on the **basename**, never with a path glob
//!    ([`whiteout`], T-0303);
//! 4. an entry that resolves outside the destination is **refused**, including
//!    through a symlink the same layer created ([`safety`], T-0304);
//! 5. an absolute symlink target is **rootfs-relative**, not a refusal
//!    ([`safety::rebase_symlink_target`], T-0305);
//! 6. owner bits are **widened between layers**, or the second layer fails
//!    ([`apply`], T-0306);
//! 7. whiteouts are applied **before** their own layer's entries, and hard
//!    links and symlinks are preserved ([`remove`], T-0307).
//!
//! ⛔ **The sidecar changes no kernel permission check.** A file recorded as
//! `gid 42` is owned on disk by the id that extracted it, and every access
//! check the kernel makes uses that one. Presenting the sidecar as though it
//! restored ownership would be podvm telling the exact class of lie it exists
//! to refuse.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod apply;
/// ⭐ The acceptance, driven against crafted layers rather than asserted in
/// isolation. A set of correct decisions wired together wrongly still extracts
/// `/etc/passwd`.
#[cfg(test)]
mod drive;
pub mod error;
pub mod layer;
pub mod remove;
pub mod safety;
pub mod sidecar;
pub mod whiteout;

use std::io::Write;
use std::path::{Path, PathBuf};

use podvm_image::{oci, space, Store};

pub use error::{Error, Result};

/// ⚠ Headroom beyond the measured payload, so an extraction does not fill the
/// destination to its last block and leave the store with nothing for its own
/// index and lock.
pub const EXTRACT_HEADROOM_BYTES: u64 = 16 * 1024 * 1024;

/// What an extraction produced.
#[derive(Debug, Clone)]
pub struct Extracted {
    pub rootfs: PathBuf,
    pub sidecar: PathBuf,
    pub layers: usize,
    pub entries: u64,
    pub removed: u64,
    /// ⭐ The figure T-0202's open question wants: docker's `SIZE` is the sum of
    /// the **uncompressed** layers, and M1 could only print the compressed
    /// bytes it held.
    pub uncompressed_bytes: u64,
    /// ⚠ True where any layer's uncompressed size was estimated rather than
    /// read. A caller that prints `uncompressed_bytes` must say so when this is
    /// set: `AGENTS.md`'s third absolute is that an estimate is labelled
    /// as one, in the same sentence, every time.
    pub uncompressed_estimated: bool,
    /// Entries whose type podvm does not materialise, counted and named.
    pub skipped: u64,
    pub skipped_kinds: Vec<String>,
    pub sidecar_rows: u64,
    pub ownership_dropped: u64,
    /// The first entry that lost an id, if any. The report names it.
    pub first_dropped: Option<sidecar::FirstDropped>,
}

/// Where an image's rootfs and its sidecar live, given the store root.
///
/// ⛔ The sidecar sits BESIDE the rootfs, not inside it. A file inside the
/// rootfs is a file the payload can read, write and be confused by, and
/// T-0302's `Prove` reads it at `<rootfs>/../.meta.jsonl` for that reason.
pub fn paths(store: &Store, digest: &str) -> (PathBuf, PathBuf) {
    let base = store.root().join("rootfs").join(digest.replace(':', "_"));
    (base.join("rootfs"), base.join(".meta.jsonl"))
}

/// Extract every layer of `manifest` into the store, in order.
///
/// ⛔ **The space precheck is here, before the loop, and that placement is the
/// point.** [`TODO/image.md`](../../../TODO/image.md) T-0203 says "before any
/// download **and again before any extraction**", and this is the second call
/// site that made it `partial`. It is deliberately NOT inside the per-layer
/// loop: an image with no layers, or one whose layers are all already present,
/// runs that loop zero times, and a guard inside it would never be reached by
/// the caller that has nothing. A guard is checked where a caller with NOTHING
/// can reach it.
pub fn extract(
    store: &Store,
    manifest: &oci::Manifest,
    digest: &str,
    out: &mut dyn Write,
) -> Result<Extracted> {
    let (rootfs, sidecar_path) = paths(store, digest);

    // ------------------------------------------------------------- the size
    //
    // Measured where the layer is gzip, whose trailer carries the uncompressed
    // length, and estimated otherwise. ⛔ Which of the two it is travels with
    // the number rather than being lost here.
    let mut need_bytes = 0u64;
    let mut estimated = false;
    let mut plan = Vec::new();
    for d in &manifest.layers {
        let c = layer::Compression::of(&d.media_type)?;
        let parsed = d.parsed_digest().map_err(Error::Image)?;
        let blob = store.blob_path(&parsed);
        let (n, est) = layer::uncompressed_size(&blob, c, d.size);
        need_bytes = need_bytes.saturating_add(n);
        estimated |= est;
        plan.push((parsed.to_string(), blob, c));
    }

    // ⚠ An inode per 8 KiB of payload, and it is AN ESTIMATE. The real count is
    // the number of entries across every layer, which is not known until the
    // layers have been read, and reading them is the thing this check exists to
    // happen before.
    let need_inodes = (need_bytes / 8192).max(64);

    std::fs::create_dir_all(&rootfs)?;
    // ⛔ T-0203's SECOND CALL SITE. `space::require` had exactly one caller
    // until this line, in `crates/podvm-image/src/pull.rs`, which is the only
    // reason that entry was `partial`.
    space::require(
        &rootfs.to_string_lossy(),
        &format!("extracting {digest}"),
        need_bytes,
        need_inodes,
        EXTRACT_HEADROOM_BYTES,
    )
    .map_err(Error::Image)?;

    let root = safety::Dir::open(&rootfs.to_string_lossy())?;
    let mut sidecar = sidecar::Sidecar::create(&sidecar_path)?;
    let ids = apply::Ids::current();

    let mut total = Extracted {
        rootfs: rootfs.clone(),
        sidecar: sidecar_path.clone(),
        layers: plan.len(),
        entries: 0,
        removed: 0,
        uncompressed_bytes: need_bytes,
        uncompressed_estimated: estimated,
        skipped: 0,
        skipped_kinds: Vec::new(),
        sidecar_rows: 0,
        ownership_dropped: 0,
        first_dropped: None,
    };

    // ⛔ EVERY FAILURE PATH BELOW REMOVES THE PARTIAL TREE. A refusal that
    // leaves the attacker's entries on disk has not refused anything: the next
    // caller finds a directory and a sidecar and, before the marker existed,
    // was told the image was extracted. See `DONE_MARKER`.
    let done = extract_layers(&root, &plan, ids, &mut sidecar, &mut total, out);
    let done = match done {
        Ok(()) => done_ok(sidecar, &mut total, &rootfs),
        Err(e) => Err(e),
    };
    match done {
        Ok(()) => Ok(total),
        Err(e) => {
            drop(root);
            // ⚠ Best effort, and deliberately silent: the extraction already
            // failed and the caller needs THAT error, not a second one about
            // the cleanup. What must not happen is the tree surviving as a
            // valid-looking extraction, and the absent marker guarantees that
            // even if this removal fails.
            let _ = remove_extracted(store, digest);
            Err(e)
        }
    }
}

fn done_ok(sidecar: sidecar::Sidecar, total: &mut Extracted, rootfs: &Path) -> Result<()> {
    total.ownership_dropped = sidecar.dropped();
    total.first_dropped = sidecar.first_dropped();
    let (rows, _) = sidecar.finish()?;
    total.sidecar_rows = rows;
    // ⛔ LAST. Everything above has to have happened for this file to exist.
    if let Some(base) = rootfs.parent() {
        std::fs::write(base.join(DONE_MARKER), b"1\n")?;
    }
    Ok(())
}

fn extract_layers(
    root: &safety::Dir,
    plan: &[(String, PathBuf, layer::Compression)],
    ids: apply::Ids,
    sidecar: &mut sidecar::Sidecar,
    total: &mut Extracted,
    out: &mut dyn Write,
) -> Result<()> {
    for (n, (ld, blob, c)) in plan.iter().enumerate() {
        // ⭐ T-0307: this layer's whiteouts are applied to the ACCUMULATED tree
        // BEFORE this layer's own entries, which is the order that lets a layer
        // both delete a path and recreate it.
        // `references/indigo-dc__udocker/tree/udocker/container/structure.py:279`.
        //
        // ⭐ T-1315: each pass hashes the raw blob bytes while they stream
        // into the decompressor. The first pass is the gate: the digest is
        // compared before any whiteout or entry touches the tree, so a blob
        // edited after pull is refused with the tampered bytes never written.
        // The second pass re-checks, so a change between the two reads is
        // still caught (the error path below removes the partial tree).
        let (mut first, hash1) = layer::open_hashed(blob, *c)?;
        let ops = remove::collect(&mut first)?;
        drop(first);
        verified(&hash1, ld, blob)?;
        total.removed += remove::apply(root, &ops)?;

        let (mut second, hash2) = layer::open_hashed(blob, *c)?;
        let st = apply::apply_layer(root, ld, &mut second, ids, sidecar)?;
        drop(second);
        verified(&hash2, ld, blob)?;
        total.entries += st.entries;
        total.skipped += st.skipped;
        for k in st.skipped_kinds {
            if !total.skipped_kinds.contains(&k) {
                total.skipped_kinds.push(k);
            }
        }
        let _ = writeln!(
            out,
            "{}: Extracted ({} entries, {} whiteouts)",
            short(ld),
            st.entries,
            st.whiteouts + st.opaques
        );
        let _ = n;
    }
    Ok(())
}

fn short(digest: &str) -> String {
    let hex = digest.split(':').next_back().unwrap_or(digest);
    hex.chars().take(12).collect()
}

/// TODO/extract.md T-1315. The manifest names the digest and the stored blob
/// may have changed since pull, so the streamed hash is compared before the
/// layer is trusted. A mismatch names the blob, the manifest's digest and
/// the computed one, through the store's own error shape.
fn verified(hash: &layer::HashedStream, want: &str, blob: &std::path::Path) -> Result<()> {
    let got = hash.digest();
    if got == want {
        return Ok(());
    }
    Err(Error::Image(podvm_image::error::Error::DigestMismatch {
        what: format!("stored blob {}", blob.display()),
        want: want.to_string(),
        got,
    }))
}

/// ⭐ **The completion marker, written last and never before.**
///
/// ⛔ FOUND BY RUNNING `experiments/220-extract-path-safety.sh`, not by a test.
/// Extraction of a hostile layer is REFUSED partway through, by design, and it
/// leaves a directory and a sidecar behind. `is_extracted` originally asked
/// whether those two existed, so the very next `podvm extract` of the same
/// image answered "already done" and printed the path of a **half-extracted
/// tree with the attacker's symlink still in it**, exit 0. The refusal was
/// correct and completely undone by the call after it.
///
/// A marker written after the last byte is the only thing that distinguishes a
/// finished extraction from an interrupted one, because a `SIGKILL` between two
/// entries leaves exactly the same directory and sidecar a success does.
pub const DONE_MARKER: &str = ".extracted";

/// Has this image already been extracted, completely?
pub fn is_extracted(store: &Store, digest: &str) -> bool {
    let (rootfs, _) = paths(store, digest);
    let Some(base) = rootfs.parent() else {
        return false;
    };
    // ⛔ The marker, not the directory. See [`DONE_MARKER`].
    rootfs.is_dir() && base.join(DONE_MARKER).is_file()
}

/// Remove an extracted rootfs and its sidecar.
pub fn remove_extracted(store: &Store, digest: &str) -> Result<bool> {
    let (rootfs, _) = paths(store, digest);
    let Some(base) = rootfs.parent() else {
        return Ok(false);
    };
    if !base.exists() {
        return Ok(false);
    }
    remove_tree(base)?;
    Ok(true)
}

/// ⛔ Removal that never follows a symlink out of the tree it is clearing.
/// `std::fs::remove_dir_all` is not used here because this tree was written
/// from a hostile archive, and the one guarantee that matters is the one this
/// crate spent T-0304 establishing.
fn remove_tree(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return Ok(());
    };
    let d = safety::Dir::open(&parent.to_string_lossy())?;
    remove::remove_at(d.fd(), name)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sidecar_sits_beside_the_rootfs_not_inside_it() {
        let s = Store::open("/tmp/podvm-test-store-paths").unwrap();
        let (rootfs, side) = paths(&s, "sha256:abc123");
        // T-0302's Prove reads `<rootfs>/../.meta.jsonl`.
        assert_eq!(rootfs.parent().unwrap().join(".meta.jsonl"), side);
        // ⛔ A colon is not a portable path character and the digest carries
        // one.
        assert!(!rootfs.to_string_lossy().contains(':'));
    }

    #[test]
    fn short_is_twelve_hex_digits_like_docker_prints() {
        assert_eq!(
            short("sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e"),
            "d9e853e87e55"
        );
    }
}
