//! Opening one layer blob as a tar stream, whatever it is compressed with.
//!
//! ⛔ **The media type decides, never the file's magic and never its name.** An
//! OCI manifest states the compression, and a reader that sniffs instead is one
//! that can be steered by the content it is reading.
//!
//! ⚠ `ruzstd` is pinned because a zstd layer is legal OCI and a registry may
//! serve one. `TODO/deps.md` T-0907 measured it: zstd costs **one page** over
//! the gzip pair, so a named refusal for a zstd layer would have saved nothing
//! worth having. `experiments/results/bloat-archive-zstd.txt` against
//! `bloat-archive.txt` is the reading.

use std::io::{BufReader, Read, Seek, SeekFrom};

use sha2::{Digest as _, Sha256};

use crate::error::{Error, Result};

/// What a layer's media type says it is compressed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Zstd,
}

impl Compression {
    /// ⚠ Both the OCI and the docker vendor media types, because a registry
    /// serves whichever the image was built with and podvm reads both.
    pub fn of(media_type: &str) -> Result<Compression> {
        // The suffix after the last `+` is the compression, where there is one.
        let base = media_type.split(';').next().unwrap_or(media_type).trim();
        if base.ends_with("+gzip") || base.ends_with(".tar.gzip") {
            return Ok(Compression::Gzip);
        }
        if base.ends_with("+zstd") {
            return Ok(Compression::Zstd);
        }
        if base.ends_with(".tar") || base.ends_with("+tar") {
            return Ok(Compression::None);
        }
        Err(Error::Compression(format!(
            "layer media type {media_type:?} names a compression podvm cannot \
             read. It reads tar, tar+gzip and tar+zstd; a layer in anything else \
             is refused here rather than extracted as though it were tar"
        )))
    }
}

/// A blob stream that hashes the raw bytes while they are read.
///
/// TODO/extract.md T-1315: the manifest names the digest and the stored blob
/// may have changed since pull, so each layer is hashed while it streams
/// into the decompressor and compared before any entry is applied. One
/// pass, no extra I/O: the bytes are read for extraction anyway, and the
/// handle below reads the digest back out of them afterwards.
pub struct HashedStream {
    state: std::rc::Rc<std::cell::RefCell<Sha256>>,
}

/// Open `path` for decompression per `c`, hashing every raw byte on the way.
/// The returned handle reports the digest of what the stream actually
/// delivered once it has been read to the end.
pub fn open_hashed(
    path: &std::path::Path,
    c: Compression,
) -> Result<(Box<dyn Read>, HashedStream)> {
    struct Hashing<R> {
        inner: R,
        state: std::rc::Rc<std::cell::RefCell<Sha256>>,
    }
    impl<R: Read> Read for Hashing<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            // ⛔ Hash what arrived, not what was offered. A short read that
            // hashed the whole buffer would verify bytes nobody saw, which
            // is the same defect `podvm-image`'s Verifier documents.
            self.state.borrow_mut().update(&buf[..n]);
            Ok(n)
        }
    }
    let state = std::rc::Rc::new(std::cell::RefCell::new(Sha256::new()));
    let f = std::fs::File::open(path)?;
    let r = BufReader::with_capacity(
        64 * 1024,
        Hashing {
            inner: f,
            state: std::rc::Rc::clone(&state),
        },
    );
    let stream: Box<dyn Read> = match c {
        Compression::None => Box::new(r),
        Compression::Gzip => Box::new(flate2::read::GzDecoder::new(r)),
        Compression::Zstd => Box::new(
            ruzstd::decoding::StreamingDecoder::new(r)
                .map_err(|e| Error::Compression(format!("this zstd layer cannot be read: {e}")))?,
        ),
    };
    Ok((stream, HashedStream { state }))
}

impl HashedStream {
    /// The `sha256:<hex>` of everything the stream delivered so far. Only
    /// meaningful once the stream has been read to the end, which both
    /// extraction passes do: whiteout collection and entry application each
    /// walk every entry.
    pub fn digest(&self) -> String {
        let h = self.state.borrow().clone().finalize();
        let mut s = String::with_capacity(71);
        s.push_str("sha256:");
        for b in h {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }
}

/// How large the layer is once decompressed, for the space precheck.
///
/// ⭐ **Exact for a gzip layer, and labelled an estimate otherwise.** gzip's
/// trailer carries `ISIZE`, the uncompressed length modulo 2^32, in the last
/// four bytes. Reading it is two syscalls and gives a real number rather than a
/// guess.
///
/// ⚠ **Modulo 2^32 is the catch, and it is handled rather than ignored.** A
/// layer of 5 GiB reports 1 GiB. Where `ISIZE` comes out *below* the compressed
/// size, the trailer has certainly wrapped and the value is useless, so the
/// estimate is used instead and says so. A layer that wrapped to something
/// plausible cannot be detected from the trailer alone, which is why the
/// caller's message says a gzip figure is exact only for layers under 4 GiB.
///
/// ⛔ Returns the estimate flag with the number. A caller that printed this as
/// measured when it was multiplied would be reporting a fabricated figure, and
/// `AGENTS.md`'s third absolute is that an estimate is labelled as one in
/// the same sentence, every time.
pub fn uncompressed_size(path: &std::path::Path, c: Compression, compressed: u64) -> (u64, bool) {
    match c {
        Compression::None => (compressed, false),
        Compression::Gzip => match gzip_isize(path) {
            // A wrapped trailer reads smaller than the compressed bytes, which
            // no real layer does.
            Some(n) if n >= compressed => (n, false),
            _ => (estimate(compressed), true),
        },
        // ⚠ zstd's frame header carries the content size only when the writer
        // chose to record it, and a registry layer often has not. Estimated,
        // and said so.
        Compression::Zstd => (estimate(compressed), true),
    }
}

/// ⚠ AN ESTIMATE, and every caller that prints it says so. The ratio is not a
/// measurement of anything: it is a deliberately generous multiplier so the
/// precheck errs towards refusing a pull that would have fitted rather than
/// starting one that will not.
fn estimate(compressed: u64) -> u64 {
    compressed.saturating_mul(4)
}

fn gzip_isize(path: &std::path::Path) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.seek(SeekFrom::End(0)).ok()?;
    if len < 4 {
        return None;
    }
    f.seek(SeekFrom::End(-4)).ok()?;
    let mut b = [0u8; 4];
    f.read_exact(&mut b).ok()?;
    Some(u32::from_le_bytes(b) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_media_types_a_registry_actually_serves() {
        assert_eq!(
            Compression::of("application/vnd.oci.image.layer.v1.tar+gzip").unwrap(),
            Compression::Gzip
        );
        assert_eq!(
            Compression::of("application/vnd.docker.image.rootfs.diff.tar.gzip").unwrap(),
            Compression::Gzip
        );
        assert_eq!(
            Compression::of("application/vnd.oci.image.layer.v1.tar+zstd").unwrap(),
            Compression::Zstd
        );
        assert_eq!(
            Compression::of("application/vnd.oci.image.layer.v1.tar").unwrap(),
            Compression::None
        );
    }

    /// ⛔ Refused, not guessed at. A layer podvm cannot decompress is not one
    /// it may hand to a tar reader and hope.
    #[test]
    fn an_unknown_compression_is_refused_and_names_itself() {
        let e = Compression::of("application/vnd.oci.image.layer.v1.tar+brotli").unwrap_err();
        let s = e.to_string();
        assert!(s.contains("brotli"), "{s}");
        assert!(s.contains("refused"), "{s}");
    }

    /// ⚠ A media type may carry parameters. `; charset=` is not part of the
    /// compression and must not turn a gzip layer into a refusal.
    #[test]
    fn parameters_do_not_change_the_compression() {
        assert_eq!(
            Compression::of("application/vnd.oci.image.layer.v1.tar+gzip; foo=bar").unwrap(),
            Compression::Gzip
        );
    }

    /// ⭐ An uncompressed layer's size is known exactly and is never multiplied.
    #[test]
    fn a_plain_tar_layer_is_its_own_size_and_is_not_an_estimate() {
        let (n, est) = uncompressed_size(
            std::path::Path::new("/nonexistent"),
            Compression::None,
            4096,
        );
        assert_eq!(n, 4096);
        assert!(!est);
    }

    /// ⚠ A missing or unreadable blob falls back to the estimate rather than to
    /// zero. Zero would pass every space check.
    #[test]
    fn an_unreadable_gzip_blob_estimates_rather_than_returning_zero() {
        let (n, est) = uncompressed_size(
            std::path::Path::new("/nonexistent"),
            Compression::Gzip,
            1000,
        );
        assert!(n >= 1000, "{n}");
        assert!(est);
    }
}
