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
//! **Hard links** (module `links`): each inode goes out once, at its first
//! name in the stream, and each other name is a link entry after it. A
//! link of a newer layer comes before its target in the pass, so it waits
//! for the target; a file that a newer layer hides goes out under the name
//! of the link that names it. A hidden file that a link of its own layer
//! names after it went by: its layer is read a second time after the last
//! layer.
//!
//! **The records.** The extended attributes and the ACLs that `extract`
//! sets go out as `SCHILY.xattr.` records ([`crate::xattr::read`]: the same
//! bounds, an ACL in its binary form), which GNU tar with the arguments of
//! incusd sets. Each other PAX record is dropped, as `extract` drops it.
//!
//! **The place and the entries of the caller** ([`flatten_with`],
//! module `extra`): [`Options::prefix`] goes before each path of the image
//! (`rootfs/` of the unified tarball of incus), and the caller writes its
//! own entries before and after the tree.

use std::io::{Read, Write};

use pod_core::{Class, Event, Failure, Report, Stage};

use crate::error::{self, Result};
use crate::{Layer, layer, pax, safety, xattr};

pub use extra::Entries;

#[path = "flatten/entry.rs"]
mod entry;
#[path = "flatten/extra.rs"]
mod extra;
#[path = "flatten/links.rs"]
mod links;
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
    /// A path before each path of the image (`rootfs/` for the unified
    /// tarball of incus), or empty. It gets the lexical check of a path of
    /// a layer; a prefix that does not pass fails before the first byte.
    pub prefix: String,
    /// The mode of the directory entry of each component of the prefix,
    /// which goes out before the tree (owner 0, time 0).
    pub prefix_mode: u32,
    /// The end blocks of the tar after the last entry. `false` for a
    /// caller that appends more entries and writes the end blocks itself.
    pub end: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            report: Report::default(),
            paths_max: PATHS_MAX,
            prefix: String::new(),
            prefix_mode: 0o755,
            end: true,
        }
    }
}

/// The value of `emitted` of a directory that the layers make with no
/// entry, after it went out (no layer has this number).
const IMPLIED: u16 = u16::MAX;

/// What a flatten wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flattened {
    pub layers: usize,
    /// The members of the layers that the crate read, as `extract` counts
    /// them: the root member and the markers included.
    pub entries: u64,
    /// The entries of the stream, the hard links included.
    pub written: u64,
    /// The hard-link entries of the stream: each name of an inode after the
    /// first.
    pub links: u64,
    /// The layers read a second time: each holds a hidden file that a hard
    /// link of its layer names after it, and whose data no name took.
    pub rereads: u64,
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
    /// The extended attributes of the entries of the stream: `set` is the
    /// count of `SCHILY.xattr.` records written, `unset` the records that
    /// `extract` does not set either (past a bound, a mark of overlayfs, a
    /// record that does not decode), with the first of them named.
    pub xattrs: xattr::Count,
    /// The entries that are not of the image: the directories of the
    /// prefix and the entries of the caller.
    pub extra: u64,
    /// The bytes of the stream, the end blocks included.
    pub bytes: u64,
    /// The paths of the trie at the end, with the layers of the markers.
    pub paths: usize,
}

/// Write the final tree of `layers` (in the order of the manifest) to
/// `out` as one tar stream. See the module.
pub fn flatten<W: Write>(layers: &[Layer], out: &mut W, options: &Options) -> Result<Flattened> {
    flatten_with(layers, out, options, |_| Ok(()), |_| Ok(()))
}

/// [`flatten`] with the entries of the caller: `before` writes entries
/// before the tree (the `metadata.yaml` of incus), `after` writes entries
/// after it (`rootfs/.podvm/podvm`), each through [`Entries`]. The order of
/// the stream: the entries of `before`, the directories of
/// [`Options::prefix`], the tree below the prefix, the entries of `after`,
/// the end blocks once ([`Options::end`]). A failure of a closure fails
/// the stream as each other failure.
pub fn flatten_with<W, B, A>(layers: &[Layer], out: &mut W, options: &Options, before: B, after: A) -> Result<Flattened>
where
    W: Write,
    B: FnOnce(&mut Entries<'_, '_, W>) -> Result<()>,
    A: FnOnce(&mut Entries<'_, '_, W>) -> Result<()>,
{
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
    let mut caller = extra::Caller::default();
    if !options.prefix.is_empty() {
        let parts = safety::components(&options.prefix)
            .map_err(|r| error::usage(format!("the prefix {:?} is not a path: {}", options.prefix, r.why())))?;
        caller.prefix = parts.iter().map(|p| p.to_string()).collect();
    }
    let prefix = caller.prefix.iter().map(|p| format!("{p}/")).collect();
    let mut w = write::Out::new(out, prefix);
    let mut st = entry::State::new(options.paths_max, layers);
    let run = || -> Result<()> {
        before(&mut Entries { out: &mut w, caller: &mut caller, image: None })?;
        caller.prefix_dirs(&mut w, options.prefix_mode)?;
        layers_newest_first(&plan, &mut st, &mut w, &options.report)?;
        st.finish(&mut w)?;
        reread(&plan, &mut st, &mut w)?;
        st.implied(&mut w)?;
        caller.check_before(&st.trie)?;
        after(&mut Entries { out: &mut w, caller: &mut caller, image: Some(&st.trie) })?;
        if options.end { w.end() } else { w.flush() }
    };
    let done = run();
    let mut total = st.total;
    (total.layers, total.bytes, total.paths) = (layers.len(), w.bytes(), st.trie.len());
    total.links = st.links.written;
    total.written += st.links.written;
    total.extra = caller.count;
    match done {
        Ok(()) => Ok(total),
        Err(e) => {
            if w.bytes() > 0 {
                w.broken();
                total.bytes = w.bytes();
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
        st.end_layer(w, number)?;
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

/// The second read of each layer that holds a hidden file or symlink
/// that a hard link of its layer names after it (module `links`).
fn reread<W: Write>(
    plan: &[(&Layer, layer::Compression)],
    st: &mut entry::State,
    w: &mut write::Out<'_, W>,
) -> Result<()> {
    for (number, members) in st.links.rereads() {
        let (l, c) = plan[usize::from(number) - 1];
        let (mut stream, hash) = layer::open_hashed(&l.blob, c)?;
        let side = pax::Side::default();
        let mut ar = tar::Archive::new(pax::Framed::new(&mut stream, side.clone()));
        for (i, e) in ar.entries().map_err(error::io)?.enumerate() {
            let mut e = e.map_err(|x| pax::failed(&side, &l.digest, x))?;
            let member = side.next();
            if let Some(names) = members.get(&(i as u64)) {
                st.reread(w, &l.digest, &mut e, &member, names)?;
            }
        }
        drop(ar);
        drop(stream);
        crate::verified(&hash, &l.digest, &l.blob)?;
        st.total.rereads += 1;
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
