//! The rootless apply of the layers of an OCI image: the layer order, the
//! whiteouts, the containment of each path, and an ownership sidecar in
//! place of `chown`.
//!
//! The wall that this crate goes around: `chown` and `lchown` to an id that
//! the user namespace does not map return `EINVAL`, and GNU tar,
//! containers/storage, Apptainer and pacman stop there.
//!
//! Seven rules:
//!
//! 1. The crate reads each archive **entry by entry, in this process**. It
//!    does not start a `tar` and does not call the `unpack()` of a library
//!    ([`apply`]).
//! 2. The crate **does not restore ownership**. The ids of the image go into
//!    a sidecar, one row for each path ([`sidecar`]).
//! 3. A whiteout is matched on the **basename**, not with a path glob
//!    ([`whiteout`]).
//! 4. An entry that resolves outside the destination is **refused**, also
//!    through a symlink that the same layer made ([`safety`]).
//! 5. An absolute symlink target is **relative to the root file system**: it
//!    is not a refusal ([`safety::rebase_symlink_target`]).
//! 6. The owner bits are **widened between layers**, or the next layer cannot
//!    write ([`apply`]).
//! 7. The whiteouts of a layer apply **before** the entries of that layer,
//!    and hard links and symlinks stay links ([`remove`]).
//!
//! The sidecar changes no permission check of the kernel. A file that the
//! sidecar records as `gid 42` has the id of the extracting process on disk,
//! and the kernel checks each access with that id. Do not show the sidecar as
//! a restored ownership.
//!
//! The caller gives each setting ([`Settings`]): the layers with their blobs
//! ([`Layer`]), the place of the root file system and of its sidecar
//! ([`Place`]), the name of its directory setting for the space message, and
//! the sink of the events. The crate reads no variable and prints nothing.
//! Each failure is a [`pod_core::Failure`] at `Stage::Host` ([`error`]).
//!
//! The crate is Linux only: on another target it is empty.
#![cfg(target_os = "linux")]
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod apply;
/// The acceptance: crafted layers through the real extraction. Correct
/// decisions with wrong connections between them still write `/etc/passwd`.
#[cfg(test)]
mod drive;
pub mod error;
pub mod layer;
mod place;
pub mod remove;
pub mod safety;
pub mod sidecar;
pub mod whiteout;

use std::path::{Path, PathBuf};

use pod_core::{Class, Event, Failure, Report, Stage};
use pod_hostprobe::space;

pub use error::{Mismatch, Refused, Result, mismatch, refused};
pub use place::{DONE_MARKER, Place, ROOTFS_NAME, SIDECAR_NAME, remove_tree};

/// The space that an extraction asks for beyond the measured payload, so
/// that the extraction does not fill the destination to its last block and
/// leave the store of the caller with no space for its index and its lock.
pub const EXTRACT_HEADROOM_BYTES: u64 = 16 * 1024 * 1024;

/// The kind of the event for each layer that the crate applied.
pub const LAYER_EVENT: &str = "extract.layer";

/// The values that the caller gives. The caller fills it one time.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The settings of `pod-hostprobe`. The space message names its
    /// `directory_setting`.
    pub host: pod_hostprobe::Settings,
    /// The space to ask for beyond the payload.
    pub headroom_bytes: u64,
    /// The sink of the events: one [`LAYER_EVENT`] for each layer.
    pub report: Report,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            host: pod_hostprobe::Settings::default(),
            headroom_bytes: EXTRACT_HEADROOM_BYTES,
            report: Report::default(),
        }
    }
}

/// One layer of an image, in the order of the manifest: the blob on disk and
/// what the manifest says about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    /// The digest that the manifest gives, `sha256:<hex>`. The crate hashes
    /// the blob while it reads it and refuses a blob that does not agree.
    pub digest: String,
    /// The media type of the manifest. It gives the compression; the content
    /// of the blob does not.
    pub media_type: String,
    /// The size of the blob that the manifest gives.
    pub size: u64,
    /// The blob in the store of the caller.
    pub blob: PathBuf,
}

/// What an extraction made.
#[derive(Debug, Clone)]
pub struct Extracted {
    pub rootfs: PathBuf,
    pub sidecar: PathBuf,
    pub layers: usize,
    pub entries: u64,
    pub removed: u64,
    /// The sum of the uncompressed layers (docker's `SIZE`).
    pub uncompressed_bytes: u64,
    /// True when the uncompressed size of a layer is an estimate. A caller
    /// that shows `uncompressed_bytes` must then say that it is an estimate.
    pub uncompressed_estimated: bool,
    /// The entries of a type that the crate does not make, with their kinds.
    pub skipped: u64,
    pub skipped_kinds: Vec<String>,
    pub sidecar_rows: u64,
    pub ownership_dropped: u64,
    /// The first entry that lost an id. The report of the caller names it.
    pub first_dropped: Option<sidecar::FirstDropped>,
}

/// Apply each layer of `layers` in order into `place`. `image` names the
/// image in the messages (for example its manifest digest).
///
/// The space check is here, before the loop, and not in the loop: an image
/// with no layers runs the loop zero times, and a check in the loop is then
/// never reached.
///
/// Each failure removes the partial tree, and the completion marker is
/// written last ([`DONE_MARKER`]): a refused or interrupted extraction never
/// reads as extracted.
pub fn extract(settings: &Settings, image: &str, layers: &[Layer], place: &Place) -> Result<Extracted> {
    // The size is measured for a gzip layer, whose trailer has the
    // uncompressed length, and estimated for the others. The flag of the
    // estimate goes with the number.
    let mut need_bytes = 0u64;
    let mut estimated = false;
    let mut plan = Vec::new();
    for l in layers {
        let c = layer::Compression::of(&l.media_type)?;
        if !l.digest.starts_with("sha256:") {
            return Err(Failure::new(
                Stage::Host,
                Class::Unsupported,
                format!("layer {}: only sha256 digests are checked while the layer is read", l.digest),
            ));
        }
        let (n, est) = layer::uncompressed_size(&l.blob, c, l.size);
        need_bytes = need_bytes.saturating_add(n);
        estimated |= est;
        plan.push((l, c));
    }

    // An estimate: one inode for each 8 KiB of payload. The true count is the
    // count of entries of each layer, which is known only after a read of the
    // layers, and this check must come before that read.
    let need_inodes = (need_bytes / 8192).max(64);

    std::fs::create_dir_all(&place.rootfs).map_err(error::io)?;
    let rootfs = place.rootfs.to_string_lossy();
    space::require(
        &settings.host,
        &rootfs,
        &format!("extracting {image}"),
        need_bytes,
        need_inodes,
        settings.headroom_bytes,
    )?;

    let root = safety::Dir::open(&rootfs)?;
    let mut side = sidecar::Sidecar::create(&place.sidecar).map_err(error::io)?;
    let ids = apply::Ids::current();

    let mut total = Extracted {
        rootfs: place.rootfs.clone(),
        sidecar: place.sidecar.clone(),
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

    // Each failure below removes the partial tree. A refusal that leaves the
    // entries of the attacker on disk refused nothing.
    let done = extract_layers(&settings.report, &root, &plan, ids, &mut side, &mut total)
        .and_then(|()| done_ok(side, &mut total, place));
    match done {
        Ok(()) => Ok(total),
        Err(e) => {
            drop(root);
            // The caller needs the first failure, not a second one about the
            // removal. The marker is absent, so the tree never reads as
            // extracted, also when this removal fails.
            let _ = place.remove();
            Err(e)
        }
    }
}

fn done_ok(side: sidecar::Sidecar, total: &mut Extracted, place: &Place) -> Result<()> {
    total.ownership_dropped = side.dropped();
    total.first_dropped = side.first_dropped();
    let (rows, _) = side.finish().map_err(error::io)?;
    total.sidecar_rows = rows;
    // Last: each step above must be done before this file exists.
    std::fs::write(&place.marker, b"1\n").map_err(error::io)?;
    Ok(())
}

fn extract_layers(
    report: &Report,
    root: &safety::Dir,
    plan: &[(&Layer, layer::Compression)],
    ids: apply::Ids,
    side: &mut sidecar::Sidecar,
    total: &mut Extracted,
) -> Result<()> {
    for (l, c) in plan {
        // The whiteouts of this layer apply to the tree of the layers before
        // it, before the entries of this layer: a layer can then remove a
        // path and make it again.
        //
        // Each pass hashes the raw bytes of the blob while they go into the
        // decompressor. The first pass is the gate: the digest is compared
        // before a whiteout or an entry touches the tree, so a blob that
        // changed after the pull is refused before its bytes are written.
        // The second pass compares again, so a change between the two reads
        // is also found (the caller removes the partial tree).
        let (mut first, hash1) = layer::open_hashed(&l.blob, *c)?;
        let ops = remove::collect(&l.digest, &mut first)?;
        drop(first);
        verified(&hash1, &l.digest, &l.blob)?;
        total.removed += remove::apply(root, &ops)?;

        let (mut second, hash2) = layer::open_hashed(&l.blob, *c)?;
        let st = apply::apply_layer(root, &l.digest, &mut second, ids, side)?;
        drop(second);
        verified(&hash2, &l.digest, &l.blob)?;
        total.entries += st.entries;
        total.skipped += st.skipped;
        for k in st.skipped_kinds {
            if !total.skipped_kinds.contains(&k) {
                total.skipped_kinds.push(k);
            }
        }
        report.event(Event::info(
            LAYER_EVENT,
            format!(
                "{}: Extracted ({} entries, {} whiteouts)",
                short(&l.digest),
                st.entries,
                st.whiteouts + st.opaques
            ),
        ));
    }
    Ok(())
}

fn short(digest: &str) -> String {
    let hex = digest.split(':').next_back().unwrap_or(digest);
    hex.chars().take(12).collect()
}

/// The manifest gives the digest, and the stored blob can have changed after
/// the pull: the hash of the stream is compared before the layer is trusted.
/// A mismatch names the blob, the digest of the manifest and the computed
/// one.
fn verified(hash: &layer::HashedStream, want: &str, blob: &Path) -> Result<()> {
    // The extraction stops at the end of the archive: the rest of the blob
    // goes into the hash first (PodUtils/podvm#281).
    hash.finish()?;
    let got = hash.digest();
    if got == want {
        return Ok(());
    }
    let m = Mismatch { what: format!("stored blob {}", blob.display()), want: want.to_string(), got };
    Err(Failure::caused(Stage::Host, Class::Protocol, m.to_string(), m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sidecar_sits_beside_the_rootfs_not_inside_it() {
        let p = Place::under(Path::new("/tmp/pod-extract-test-paths/sha256_abc123"));
        // The sidecar is at `<rootfs>/../.meta.jsonl`, out of reach of the
        // payload.
        assert_eq!(p.rootfs.parent().unwrap().join(SIDECAR_NAME), p.sidecar);
        assert_eq!(p.rootfs.parent().unwrap().join(DONE_MARKER), p.marker);
        assert!(!p.sidecar.starts_with(&p.rootfs));
    }

    #[test]
    fn short_is_twelve_hex_digits_like_docker_prints() {
        assert_eq!(short("sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e"), "d9e853e87e55");
    }
}
