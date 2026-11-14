//! A reader of UDF (ECMA-167 with UDF 1.02 to 2.60, as the ISO images of
//! Windows use them, with the metadata partition where one is present). It
//! finds a file by its path and gives the byte ranges of the file in the
//! image, in order, and its size, so that a client fetches only those
//! ranges, for example by HTTP Range requests.
//!
//! The crate does no I/O of its own: the caller gives a [`ReadAt`], which
//! reads bytes at an offset of the image. [`Udf::open`] reads the anchor,
//! the volume descriptors and the file set; [`Udf::find`] walks the path.
//!
//! ```no_run
//! use pod_udf::{Extent, Seeker, Udf};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = std::fs::File::open("windows.iso")?;
//! let mut udf = Udf::open(Seeker(file))?;
//! let extents = udf.find("/sources/boot.wim")?;
//! println!("{} bytes", extents.size);
//! for part in &extents.parts {
//!     match part {
//!         Extent::Data { offset, len } => println!("bytes {offset} to {}", offset + len - 1),
//!         Extent::Zero { len } => println!("{len} bytes of zeros"),
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Each failure is a typed [`Error`]. [`Limits`] bound the depth of a path,
//! the entries of a directory, the bytes of a directory, the allocation
//! descriptors of a file and the volume descriptors, so that no input makes
//! the reader loop or allocate without bound.

mod dir;
mod error;
mod icb;
mod read;
mod tag;
mod udf;
mod volume;

pub use dir::Entry;
pub use error::{Error, TagFault};
pub use read::{ReadAt, Seeker};
pub use udf::{Extent, Extents, Udf, find};

/// The bounds of a walk. Each bound that a walk passes is
/// [`Error::Limit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most components of a path.
    pub depth: usize,
    /// The most file identifier descriptors of one directory.
    pub entries: usize,
    /// The most bytes of one directory.
    pub directory_bytes: u64,
    /// The most allocation descriptors of one file, with the allocation
    /// extent descriptors that hold them.
    pub extents: usize,
    /// The most volume descriptors of one sequence, with each pointer to a
    /// next extent.
    pub descriptors: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { depth: 64, entries: 65_536, directory_bytes: 16 << 20, extents: 65_536, descriptors: 1024 }
    }
}

#[cfg(test)]
mod tests;
