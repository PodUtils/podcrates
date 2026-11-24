//! The final tree of the layers of an image as one tar stream, with no tree
//! on the disk.
//!
//! [`flatten`] reads the same [`Layer`]s as [`crate::extract`] and writes
//! to a [`Write`] the entries that the tree of `extract` holds: the paths,
//! kinds, modes, owners of the image, contents and symlink targets. A
//! caller that streams the tree of an image (to incusd, for example) then
//! needs no copy of the tree on its disk. The module makes no system call
//! and opens no tree.
//!
//! **One pass, from the newest layer.** Each blob is read once. The layers
//! go from the newest to the oldest, and the entries of a layer in their
//! order; each entry that no newer layer hides goes out as it is read, with
//! its data. A directory entry can then come after its children. GNU tar,
//! the unpacker of incusd (`tar -xf -` with `--numeric-owner`, as root),
//! takes that order with the mode and the owner of each directory, and so
//! does `extract` (a directory that is there gets its mode at the end).
//!
//! **The rules of `extract`, with no second copy.** The paths get
//! [`crate::safety::components`], the symlink targets
//! [`crate::safety::rebase_symlink_target`], the markers [`crate::whiteout::classify`],
//! and a GNU sparse file [`crate::sparse`]'s checks (its holes go out as
//! zero bytes). The tree is a trie of the paths that the layers name
//! (`trie`). An entry of layer `L` at the path `P` is hidden when a newer
//! layer removes `P` or a directory above it (a whiteout), empties a
//! directory above it (an opaque marker), has a non-directory above it, or
//! has an entry at `P`: the last writer wins, and of two directories the
//! newest gives the mode and the owner. A marker of a layer hides the paths
//! of the older layers only, so a layer can remove a path and make it
//! again. Two entries of one path in one layer go out in their order, as a
//! reader of the layer takes them; a file or a link over a directory that
//! the same layer filled in the stream refuses the layer
//! ([`crate::safety::Refusal::Stream`]), as a reader of a stream cannot replace a
//! full directory.
//!
//! **An entry through a symlink of an older layer** is refused as `extract`
//! refuses it (the whole layer, [`crate::safety::Refusal::Escapes`]). The pass
//! finds it when it reads the older layer, after the newer layer went out.
//! Each path keeps a mark: "a newer layer wrote below here". A
//! non-directory of an older layer at the end of that layer refuses the
//! newer layer, unless a layer between them removed the path.
//!
//! **A failure after the first byte** writes one block that is not a tar
//! header where the end blocks go: a reader of the stream then fails, and
//! does not take a short tree as a whole one. GNU tar takes a stream with
//! no end blocks as whole. The caller discards the upload. The digest of a
//! blob is known at its end, after its entries went out: a blob that does
//! not agree fails the stream with the cause [`crate::Mismatch`].
//!
//! **Not in the stream:** the extended attributes and the ACLs of an
//! entry, and the entries of the caller. A hard link whose target is not a
//! file of the same layer that went out before it fails the stream
//! (`Unsupported`, the link named). Each other PAX record is dropped, as
//! `extract` drops it.

use std::io::{Read, Write};

use pod_core::{Class, Event, Failure, Report, Stage};

use crate::error::{self, Result};
use crate::{Layer, layer, pax};

#[path = "flatten/entry.rs"]
mod entry;
#[path = "flatten/trie.rs"]
mod trie;
#[path = "flatten/write.rs"]
mod write;

/// The kind of the event for each layer that went into the stream.
pub const LAYER_EVENT: &str = "flatten.layer";

/// The default bound of the paths that [`flatten`] keeps in memory (about
/// 52 bytes and the bytes of the name of each path; module `trie`).
pub const PATHS_MAX: usize = 8 * 1024 * 1024;

/// The values that the caller gives.
#[derive(Clone, Debug)]
pub struct Options {
    /// The sink of the events: one [`LAYER_EVENT`] for each layer.
    pub report: Report,
    /// The most paths of the image, with the layers of each whiteout and
    /// opaque marker. The path after it fails the stream (`Unsupported`).
    pub paths_max: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options { report: Report::default(), paths_max: PATHS_MAX }
    }
}

/// What a flatten wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flattened {
    pub layers: usize,
    /// The members of the layers that the crate read, as `extract` counts
    /// them: the root member and the markers included.
    pub entries: u64,
    /// The entries of the stream.
    pub written: u64,
    /// The directories that the layers make with no entry (a file below
    /// them) and that no newer layer removes, written after the last layer
    /// with mode 0755 and owner 0, as `extract` makes them.
    pub implied: u64,
    /// The entries that a newer layer hides.
    pub hidden: u64,
    /// The whiteouts and the opaque markers.
    pub whiteouts: u64,
    /// The entries of a kind that `extract` does not make, with their kinds.
    pub skipped: u64,
    pub skipped_kinds: Vec<String>,
    /// The bytes of the stream, the end blocks included.
    pub bytes: u64,
    /// The paths of the trie at the end, with the layers of the markers.
    pub paths: usize,
}

/// Write the final tree of `layers` (in the order of the manifest) to
/// `out` as one tar stream. See the module.
pub fn flatten<W: Write>(layers: &[Layer], out: &mut W, options: &Options) -> Result<Flattened> {
    // Each layer is checked before the first byte, as `extract` checks it.
    let mut plan = Vec::new();
    for l in layers {
        let c = layer::Compression::of(&l.media_type)?;
        if !l.digest.starts_with("sha256:") {
            return Err(error::unsupported(format!(
                "layer {}: only sha256 digests are checked while the layer is read",
                l.digest
            )));
        }
        plan.push((l, c));
    }
    if layers.len() >= usize::from(u16::MAX) {
        return Err(error::unsupported(format!("an image of {} layers: flatten reads 65534 at most", layers.len())));
    }
    let mut w = write::Out::new(out);
    let mut st = entry::State::new(options.paths_max, layers);
    let done = layers_newest_first(&plan, &mut st, &mut w, &options.report)
        .and_then(|()| st.implied(&mut w))
        .and_then(|()| w.end());
    let mut total = st.total;
    (total.layers, total.bytes, total.paths) = (layers.len(), w.bytes(), st.trie.len());
    match done {
        Ok(()) => Ok(total),
        Err(e) => {
            if w.bytes() > 0 {
                w.broken();
            }
            Err(e)
        }
    }
}

fn layers_newest_first<W: Write>(
    plan: &[(&Layer, layer::Compression)],
    st: &mut entry::State,
    w: &mut write::Out<'_, W>,
    report: &Report,
) -> Result<()> {
    for (i, (l, c)) in plan.iter().enumerate().rev() {
        // The checked count above keeps the number inside `u16`.
        let number = (i + 1) as u16;
        let before = st.total.clone();
        let (mut stream, hash) = layer::open_hashed(&l.blob, *c)?;
        read_layer(st, w, &l.digest, number, &mut stream)?;
        drop(stream);
        crate::verified(&hash, &l.digest, &l.blob)?;
        st.end_layer(number)?;
        let t = &st.total;
        report.event(Event::info(
            LAYER_EVENT,
            format!(
                "{}: Flattened ({} entries written, {} hidden, {} whiteouts)",
                crate::short(&l.digest),
                t.written - before.written,
                t.hidden - before.hidden,
                t.whiteouts - before.whiteouts
            ),
        ));
    }
    Ok(())
}

fn read_layer<W: Write>(
    st: &mut entry::State,
    w: &mut write::Out<'_, W>,
    digest: &str,
    number: u16,
    stream: &mut dyn Read,
) -> Result<()> {
    let side = pax::Side::default();
    let mut ar = tar::Archive::new(pax::Framed::new(stream, side.clone()));
    for e in ar.entries().map_err(error::io)? {
        let mut e = e.map_err(|x| pax::failed(&side, digest, x))?;
        let member = side.next();
        st.entry(w, digest, number, &mut e, &member)?;
    }
    Ok(())
}

/// The trie is full: the failure names the bound.
fn full(max: usize) -> Failure {
    Failure::new(
        Stage::Host,
        Class::Unsupported,
        format!(
            "the image names more than {max} paths (with its markers), and flatten keeps each path in memory; \
             give a larger Options::paths_max"
        ),
    )
}
