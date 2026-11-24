//! The fuzz target of `flatten` of `pod-extract` (T-1254, T-1193): the
//! layers of an untrusted image into one tar stream, with no tree on the
//! disk.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use pod_extract::Layer;
use pod_extract::flatten::{Flattened, Options, flatten};
use pod_extract::layer::{Compression, open_hashed};
use sha2::{Digest, Sha256};

/// The media type of each compression that the first byte picks.
const MEDIA: [&str; 3] = [
    "application/vnd.oci.image.layer.v1.tar",
    "application/vnd.oci.image.layer.v1.tar+gzip",
    "application/vnd.oci.image.layer.v1.tar+zstd",
];

/// The bound of the paths of a run: a small trie keeps a run fast, and an
/// input past it fails with `Unsupported`.
const PATHS: usize = 256;

/// The most bytes of the stream that a run keeps. A write past it fails,
/// and the stream fails with it; the checks of the stream then do not run.
/// Below the allocation limit of `scripts/fuzz.sh` (64 MiB).
const SINK_MAX: usize = 4 << 20;

/// A fresh name for the directory of the blobs of one run.
static RUN: AtomicU64 = AtomicU64::new(0);

/// The stream of a run, cut at [`SINK_MAX`].
#[derive(Default)]
struct Sink {
    bytes: Vec<u8>,
    full: bool,
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len() + buf.len() > SINK_MAX {
            self.full = true;
            return Err(std::io::Error::other("the sink of the fuzz target is full"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The layers of an image (`pod_extract::flatten::flatten`, T-1193): the
/// first byte gives the count of layers (1 to 4, the byte modulo 4) and the
/// compression of each (the digits of the byte divided by 4, in base 3:
/// tar, gzip, zstd), oldest first. Each layer but the last is a length of
/// two bytes (little endian) and its blob; the last layer is the rest.
/// Each blob is written with its true digest. The checks: no panic; the
/// stream has the bytes that the count gives; on success the `tar` reader
/// reads the stream to its end, with one entry for each entry that the
/// count gives, no path absolute or with `..`, and each path once unless
/// one layer names it twice (two entries of one path in one layer go out
/// in their order); on a failure after the first byte the stream ends with
/// the block of 0xff and the `tar` reader fails on it.
pub fn flatten_layers(data: &[u8]) {
    run(data);
}

/// One run of `flatten_layers`, with its checks: what `flatten` gave and
/// the stream. `None` for an empty input.
fn run(data: &[u8]) -> Option<(pod_extract::Result<Flattened>, Sink)> {
    let (&pick, mut rest) = data.split_first()?;
    let count = usize::from(pick % 4) + 1;
    let mut blobs = Vec::new();
    let mut digits = usize::from(pick / 4);
    for i in 0..count {
        let bytes = if i + 1 == count {
            std::mem::take(&mut rest)
        } else {
            let len = match rest {
                [a, b, tail @ ..] => {
                    rest = tail;
                    usize::from(u16::from_le_bytes([*a, *b]))
                }
                _ => std::mem::take(&mut rest).len(),
            };
            let (blob, tail) = rest.split_at(len.min(rest.len()));
            rest = tail;
            blob
        };
        blobs.push((bytes, digits % 3));
        digits /= 3;
    }
    let n = RUN.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("podcrates-fuzz-flatten-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the directory of a run");
    let layers = write_blobs(&dir, &blobs);
    let mut sink = Sink::default();
    let options = Options { paths_max: PATHS, ..Options::default() };
    let done = flatten(&layers, &mut sink, &options);
    if !sink.full {
        check(&layers, &sink.bytes, &done);
    }
    let _ = std::fs::remove_dir_all(&dir);
    Some((done, sink))
}

/// Each blob as a file under `dir`, with its digest and its media type.
fn write_blobs(dir: &Path, blobs: &[(&[u8], usize)]) -> Vec<Layer> {
    let mut out = Vec::new();
    for (i, (bytes, c)) in blobs.iter().enumerate() {
        let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        let blob = dir.join(format!("blob-{i}"));
        std::fs::write(&blob, bytes).expect("the blob of a run");
        out.push(Layer {
            digest: format!("sha256:{hex}"),
            media_type: MEDIA[*c].into(),
            size: bytes.len() as u64,
            blob,
        });
    }
    out
}

/// The checks of one stream; the sink kept each byte of it.
fn check(layers: &[Layer], stream: &[u8], done: &pod_extract::Result<Flattened>) {
    let total = match done {
        Ok(total) => total,
        Err(f) => {
            if !stream.is_empty() {
                assert!(stream.ends_with(&[0xff; 512]), "a failure with no block of 0xff: {}", f.chain());
                let mut ar = tar::Archive::new(Cursor::new(stream));
                let read = ar
                    .entries()
                    .and_then(|mut es| es.try_for_each(|e| std::io::copy(&mut e?, &mut std::io::sink()).map(|_| ())));
                assert!(read.is_err(), "the tar reader takes a broken stream of {} bytes: {}", stream.len(), f.chain());
            }
            return;
        }
    };
    assert_eq!(total.bytes, stream.len() as u64, "the count of the bytes");
    let mut ar = tar::Archive::new(Cursor::new(stream));
    let mut seen = BTreeMap::<String, u32>::new();
    let mut listed = 0u64;
    for e in ar.entries().expect("the stream reads") {
        let mut e = e.expect("an entry of the stream reads");
        let path = String::from_utf8_lossy(&e.path_bytes()).into_owned();
        let mut names = vec![path.clone()];
        if e.header().entry_type().is_hard_link() {
            let link = e.link_name_bytes().expect("a hard link has a target");
            names.push(String::from_utf8_lossy(&link).into_owned());
        }
        for name in &names {
            assert!(!name.is_empty() && !name.starts_with('/'), "the path {name:?}");
            assert!(name.split('/').all(|c| c != ".."), "the path {name:?}");
        }
        std::io::copy(&mut e, &mut std::io::sink()).expect("the data of an entry reads");
        *seen.entry(path.trim_end_matches('/').to_string()).or_default() += 1;
        listed += 1;
    }
    assert_eq!(listed, total.written + total.implied + total.extra, "the entries of the stream: {total:?}");
    let twice: Vec<&String> = seen.iter().filter(|(_, n)| **n > 1).map(|(p, _)| p).collect();
    if !twice.is_empty() {
        let Some(named) = named_twice(layers) else { return };
        for path in twice {
            assert!(named.contains(path), "{path} is in the stream two times");
        }
    }
}

/// Each path that one layer names two times, as the `tar` reader gives it
/// with no `./` and no `/` at its end. `None` when the reader fails on a
/// layer: its view of the layer is then not that of the crate.
fn named_twice(layers: &[Layer]) -> Option<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for l in layers {
        let c = Compression::of(&l.media_type).ok()?;
        let (stream, _) = open_hashed(&l.blob, c).ok()?;
        let mut seen = BTreeSet::new();
        for e in tar::Archive::new(stream).entries().ok()? {
            let e = e.ok()?;
            let raw = String::from_utf8_lossy(&e.path_bytes()).into_owned();
            let parts = pod_extract::safety::components(&raw).ok()?;
            let path = parts.join("/");
            if !seen.insert(path.clone()) {
                out.insert(path);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("seeds/flatten_layers").join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// The seeds reach past the first check: each image of one to three
    /// layers in each compression flattens, an entry through a symlink of
    /// an older layer is refused, and a blob that fails after the newer
    /// layer went out breaks the stream.
    #[test]
    fn the_seeds_of_flatten_layers_read() {
        let paths = |name: &str| {
            let (done, sink) = run(&seed(name)).expect("a seed");
            done.unwrap_or_else(|f| panic!("{name}: {}", f.chain()));
            let mut ar = tar::Archive::new(Cursor::new(sink.bytes));
            let entries = ar.entries().expect("the stream");
            entries
                .map(|e| String::from_utf8_lossy(&e.expect("an entry").path_bytes()).into_owned())
                .collect::<Vec<_>>()
        };
        assert!(!paths("whiteouts").iter().any(|p| p.contains("shadow") || p.starts_with("opt") || p == "top"));
        assert!(paths("opaque").iter().all(|p| !p.ends_with("old")));
        assert_eq!(paths("flip"), ["p", "q"]);
        assert_eq!(paths("twice-in-one-layer").iter().filter(|p| *p == "f").count(), 2);
        for (name, want) in [
            ("links-across-layers", "bin/ls"),
            ("long-names", "link"),
            ("each-compression", "etc/motd"),
            ("each-compression", "etc/hostname"),
        ] {
            assert!(paths(name).iter().any(|p| p == want), "{name}: no {want}");
        }
        let (done, _) = run(&seed("escape-by-older-symlink")).expect("a seed");
        let refused = pod_extract::refused(&done.expect_err("an escape")).cloned().expect("a refusal");
        assert!(matches!(refused.refusal, pod_extract::safety::Refusal::Escapes(_)), "{refused:?}");
        let (done, sink) = run(&seed("broken-older")).expect("a seed");
        assert!(done.is_err() && sink.bytes.len() > 512 && sink.bytes.ends_with(&[0xff; 512]));
    }
}
