//! The open of one layer blob as a tar stream, with each compression.
//!
//! **The media type decides, not the magic bytes of the file and not its
//! name.** An OCI manifest gives the compression. A reader that looks at the
//! content can be steered by the content that it reads.
//!
//! A zstd layer is legal OCI and a registry can serve one, so `ruzstd` reads
//! it. Its size in a binary is about one page more than the gzip pair.

use std::cell::RefCell;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::rc::Rc;

use sha2::{Digest as _, Sha256};

use crate::error::{self, Result};

/// What a layer's media type says it is compressed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Zstd,
}

impl Compression {
    /// The OCI and the docker vendor media types: a registry serves the type
    /// with which the image was built.
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
        Err(error::unsupported(format!(
            "layer media type {media_type:?} names a compression this crate cannot \
             read. It reads tar, tar+gzip and tar+zstd; a layer in anything else \
             is refused here rather than extracted as though it were tar"
        )))
    }
}

/// A blob stream that hashes the raw bytes while they are read.
///
/// The manifest gives the digest, and the stored blob can have changed after
/// the pull: each layer is hashed while it goes into the decompressor, and
/// the hash is compared before an entry is applied. No more I/O: the
/// extraction reads the bytes, and the handle gives the digest of them after.
/// The extraction stops at the end of the archive, so
/// [`HashedStream::finish`] reads the rest of the raw blob into the hash
/// before the digest counts (PodUtils/podvm#281).
pub struct HashedStream {
    state: Rc<RefCell<Sha256>>,
    raw: Rc<RefCell<Raw>>,
}

/// The raw blob: the file, each byte hashed as it comes, behind one buffer.
type Raw = BufReader<Hashing<std::fs::File>>;

/// A reader that hashes each byte that comes through it.
struct Hashing<R> {
    inner: R,
    state: Rc<RefCell<Sha256>>,
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        // Hash the bytes that came, not the whole buffer: a short read
        // that hashed the buffer would verify bytes that nobody saw.
        self.state.borrow_mut().update(&buf[..n]);
        Ok(n)
    }
}

/// The raw blob as the decompressor reads it; [`HashedStream`] holds it too.
struct Shared(Rc<RefCell<Raw>>);

impl Read for Shared {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().read(buf)
    }
}

/// Open `path` for decompression per `c`, hashing every raw byte on the way.
/// The returned handle reports the digest of the whole blob after
/// [`HashedStream::finish`].
pub fn open_hashed(path: &std::path::Path, c: Compression) -> Result<(Box<dyn Read>, HashedStream)> {
    let state = Rc::new(RefCell::new(Sha256::new()));
    let f = std::fs::File::open(path).map_err(error::io)?;
    let raw =
        Rc::new(RefCell::new(BufReader::with_capacity(64 * 1024, Hashing { inner: f, state: Rc::clone(&state) })));
    let r = Shared(Rc::clone(&raw));
    let stream: Box<dyn Read> = match c {
        Compression::None => Box::new(r),
        Compression::Gzip => Box::new(flate2::read::GzDecoder::new(r)),
        Compression::Zstd => Box::new(
            ruzstd::decoding::StreamingDecoder::new(r)
                .map_err(|e| error::unsupported(format!("this zstd layer cannot be read: {e}")))?,
        ),
    };
    Ok((stream, HashedStream { state, raw }))
}

impl HashedStream {
    /// Read the rest of the raw blob into the hash: the bytes after the end
    /// of the archive that no read of the extraction took (the padding of
    /// tar to its record of 10 240 bytes, bytes after a compressed frame).
    /// The bytes in the buffer were hashed when they came from the file.
    pub fn finish(&self) -> Result<()> {
        std::io::copy(&mut *self.raw.borrow_mut(), &mut std::io::sink()).map_err(error::io)?;
        Ok(())
    }

    /// The `sha256:<hex>` of each byte that the stream gave until now. It has a
    /// meaning only after [`HashedStream::finish`].
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

/// The size of the layer after decompression, for the space check.
///
/// **Exact for a gzip layer, and an estimate for the others.** The trailer of
/// gzip has `ISIZE`, the uncompressed length modulo 2^32, in its last four
/// bytes. Two system calls read it, and it is a true number.
///
/// **Modulo 2^32:** a layer of 5 GiB gives 1 GiB. When `ISIZE` is *less*
/// than the compressed size, the trailer wrapped and the value is of no use:
/// the estimate is used, with its flag. A layer that wrapped to a possible
/// value cannot be found from the trailer, so a gzip value is exact only for
/// a layer under 4 GiB.
///
/// The flag of the estimate comes back with the number. A caller that shows
/// an estimate as a measurement shows a false value.
pub fn uncompressed_size(path: &std::path::Path, c: Compression, compressed: u64) -> (u64, bool) {
    match c {
        Compression::None => (compressed, false),
        Compression::Gzip => match gzip_isize(path) {
            // A wrapped trailer is less than the compressed bytes, and no
            // real layer is.
            Some(n) if n >= compressed => (n, false),
            _ => (estimate(compressed), true),
        },
        // The frame header of zstd has the content size only when the writer
        // wrote it, and a registry layer often has none: an estimate.
        Compression::Zstd => (estimate(compressed), true),
    }
}

/// An estimate, and each caller that shows it says so. The ratio is not a
/// measurement: it is a large multiplier, so that the space check refuses an
/// extraction that would fit before it starts one that will not fit.
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
        assert_eq!(Compression::of("application/vnd.oci.image.layer.v1.tar+gzip").unwrap(), Compression::Gzip);
        assert_eq!(Compression::of("application/vnd.docker.image.rootfs.diff.tar.gzip").unwrap(), Compression::Gzip);
        assert_eq!(Compression::of("application/vnd.oci.image.layer.v1.tar+zstd").unwrap(), Compression::Zstd);
        assert_eq!(Compression::of("application/vnd.oci.image.layer.v1.tar").unwrap(), Compression::None);
    }

    /// Refused, not guessed. A layer that the crate cannot decompress does not
    /// go to a tar reader.
    #[test]
    fn an_unknown_compression_is_refused_and_names_itself() {
        let e = Compression::of("application/vnd.oci.image.layer.v1.tar+brotli").unwrap_err();
        let s = e.to_string();
        assert!(s.contains("brotli"), "{s}");
        assert!(s.contains("refused"), "{s}");
    }

    /// A media type can have parameters. `; charset=` is not part of the
    /// compression and must not make a gzip layer a refusal.
    #[test]
    fn parameters_do_not_change_the_compression() {
        assert_eq!(Compression::of("application/vnd.oci.image.layer.v1.tar+gzip; foo=bar").unwrap(), Compression::Gzip);
    }

    /// The size of an uncompressed layer is exact and is not multiplied.
    #[test]
    fn a_plain_tar_layer_is_its_own_size_and_is_not_an_estimate() {
        let (n, est) = uncompressed_size(std::path::Path::new("/nonexistent"), Compression::None, 4096);
        assert_eq!(n, 4096);
        assert!(!est);
    }

    /// A missing or unreadable blob gives the estimate, not zero. Zero would
    /// pass each space check.
    #[test]
    fn an_unreadable_gzip_blob_estimates_rather_than_returning_zero() {
        let (n, est) = uncompressed_size(std::path::Path::new("/nonexistent"), Compression::Gzip, 1000);
        assert!(n >= 1000, "{n}");
        assert!(est);
    }
}
