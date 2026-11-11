//! A reader of a squashfs 4.0 image, for the read path only: the
//! superblock, the compressor options, the inode and directory tables, the
//! fragment, id and export tables, and the data blocks (compressed, stored
//! or sparse).
//!
//! The decoder is xz, with or without a BCJ filter (`xz4rust`, Rust with no
//! unsafe code). A stored block needs no decoder, so an image of another
//! compressor reads as far as its blocks are stored; a compressed block of
//! it is [`Fault::Compressor`].
//!
//! The reader trusts nothing in the image. Each offset and each size is
//! checked against the end of the image, each block decodes to the length
//! that its table gives, and a file, a listing and the cache of metadata
//! have a ceiling. A malformed image is a [`Fault`] in the chain of a
//! [`pod_core::Failure`], never a panic or an allocation that the image
//! sizes. The reader takes a `Read + Seek` and reads only the blocks that a
//! call needs.

mod inode;
mod meta;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
pub(crate) mod writer;

use std::io::{Read, Seek, SeekFrom};

use pod_core::{Class, Failure, Stage};

use crate::Result;

pub use inode::{Dir, Entry, File, Inode, Kind};

/// The magic of a squashfs image, `hsqs` in little-endian order.
pub const MAGIC: u32 = 0x7371_7368;
/// The length of the superblock.
pub const SUPERBLOCK_LEN: usize = 96;
/// The most bytes that one metadata block holds.
pub const METADATA_SIZE: usize = 8192;
/// The ceiling of a file that [`Image::read`] reads into memory.
pub const MAX_FILE: u64 = 64 << 20;
/// The ceiling of a directory listing.
pub const MAX_LISTING: u64 = 16 << 20;
/// The ceiling of the target of a symbolic link (`PATH_MAX`).
pub const MAX_LINK: u64 = 4096;
/// The value of a table offset that names no table.
pub const NO_TABLE: u64 = u64::MAX;
/// The flag of the superblock: the compressor options follow it.
pub const FLAG_COMPRESSOR_OPTIONS: u16 = 0x0400;
/// The id of the xz compressor.
pub const XZ: u16 = 4;

/// The compressor of an image, as its superblock and options give it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compressor {
    /// xz: the size of the dictionary, and the BCJ filters that mksquashfs
    /// tried (a bit mask; 1 is x86). Each stream names its own filters.
    Xz { dictionary: u32, filters: u32 },
    /// Another compressor of squashfs (1 gzip, 2 lzma, 3 lzo, 5 lz4, 6
    /// zstd): only its stored blocks read.
    Other(u16),
}

/// The superblock of an image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Superblock {
    pub inodes: u32,
    pub mtime: u32,
    pub block_size: u32,
    pub fragments: u32,
    pub compressor: Compressor,
    pub flags: u16,
    pub ids: u16,
    /// The reference of the root directory's inode.
    pub root: u64,
    /// The end of the image: no read goes past it.
    pub bytes_used: u64,
    pub id_table: u64,
    pub xattr_table: u64,
    pub inode_table: u64,
    pub directory_table: u64,
    pub fragment_table: u64,
    pub export_table: u64,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from(u32_at(b, at)) | (u64::from(u32_at(b, at + 4)) << 32)
}

impl Superblock {
    /// Parse the 96 bytes of a superblock. The compressor options are not in
    /// them: [`Image::open`] reads them.
    pub fn parse(b: &[u8; SUPERBLOCK_LEN]) -> std::result::Result<Superblock, Fault> {
        let magic = u32_at(b, 0);
        if magic != MAGIC {
            return Err(Fault::Magic(magic));
        }
        let (major, minor) = (u16_at(b, 28), u16_at(b, 30));
        if (major, minor) != (4, 0) {
            return Err(Fault::Version { major, minor });
        }
        let (block_size, log) = (u32_at(b, 12), u16_at(b, 22));
        if !(12..=20).contains(&log) || block_size != 1 << log {
            return Err(Fault::BlockSize { size: block_size, log });
        }
        let compressor = match u16_at(b, 20) {
            XZ => Compressor::Xz { dictionary: block_size.max(METADATA_SIZE as u32), filters: 0 },
            id @ (1..=3 | 5 | 6) => Compressor::Other(id),
            id => return Err(Fault::Compressor(id)),
        };
        Ok(Superblock {
            inodes: u32_at(b, 4),
            mtime: u32_at(b, 8),
            block_size,
            fragments: u32_at(b, 16),
            compressor,
            flags: u16_at(b, 24),
            ids: u16_at(b, 26),
            root: u64_at(b, 32),
            bytes_used: u64_at(b, 40),
            id_table: u64_at(b, 48),
            xattr_table: u64_at(b, 56),
            inode_table: u64_at(b, 64),
            directory_table: u64_at(b, 72),
            fragment_table: u64_at(b, 80),
            export_table: u64_at(b, 88),
        })
    }
}

/// Why an image does not read: the cause of the [`Failure`] of a call.
/// [`fault`] finds it in a failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The first four bytes are not [`MAGIC`].
    Magic(u32),
    /// A version other than 4.0.
    Version { major: u16, minor: u16 },
    /// A block size that is not a power of two from 4 KiB to 1 MiB, or that
    /// its log does not give.
    BlockSize { size: u32, log: u16 },
    /// A compressor that has no decoder here, by its id.
    Compressor(u16),
    /// Compressor options that do not read: why.
    Options(&'static str),
    /// An xz dictionary that is not 2^n or 3 × 2^n, or that is larger than
    /// the block size.
    Dictionary(u32),
    /// A read that goes past the end of the image: what, and where.
    Past { what: &'static str, at: u64 },
    /// A metadata block whose header gives 0 bytes or more than 8 KiB.
    MetadataSize { at: u64, size: u16 },
    /// An offset past the data of its metadata block.
    Offset { at: u64, offset: usize },
    /// A block whose size on disk is larger than the block size.
    BlockLength { at: u64, size: u32 },
    /// An xz stream that does not decode: where, and why.
    Stream { at: u64, why: String },
    /// A block that decodes to another length than its table gives.
    Length { at: u64, want: usize, got: usize },
    /// An inode reference outside the inode table.
    Reference(u64),
    /// An inode type that squashfs 4.0 does not have.
    InodeType(u16),
    /// A directory listing that does not parse: why.
    Listing(&'static str),
    /// An index past the end of its table.
    Index { table: &'static str, index: u64 },
    /// A fragment whose bytes are not in its block.
    Fragment { index: u32 },
    /// A file, a listing or a link over its ceiling.
    TooLarge { what: &'static str, size: u64, limit: u64 },
    /// A path that the image does not hold.
    Missing(String),
    /// A path that is not a directory, where a directory is necessary.
    NotADirectory(String),
    /// A path that is not a regular file.
    NotAFile(String),
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::Magic(m) => write!(f, "no squashfs magic (0x{m:08x})"),
            Fault::Version { major, minor } => write!(f, "squashfs {major}.{minor}, not 4.0"),
            Fault::BlockSize { size, log } => write!(f, "a block size of {size} with the log {log}"),
            Fault::Compressor(id) => write!(f, "compressor {id} has no decoder here"),
            Fault::Options(why) => write!(f, "the compressor options: {why}"),
            Fault::Dictionary(d) => write!(f, "an xz dictionary of {d} bytes"),
            Fault::Past { what, at } => write!(f, "{what} at {at} goes past the end of the image"),
            Fault::MetadataSize { at, size } => write!(f, "the metadata block at {at} gives {size} bytes"),
            Fault::Offset { at, offset } => write!(f, "offset {offset} is past the metadata block at {at}"),
            Fault::BlockLength { at, size } => write!(f, "the block at {at} has {size} bytes on disk"),
            Fault::Stream { at, why } => write!(f, "the xz stream at {at}: {why}"),
            Fault::Length { at, want, got } => write!(f, "the block at {at} gives {got} bytes, not {want}"),
            Fault::Reference(r) => write!(f, "the inode reference 0x{r:x} is outside the inode table"),
            Fault::InodeType(t) => write!(f, "inode type {t}"),
            Fault::Listing(why) => write!(f, "a directory listing: {why}"),
            Fault::Index { table, index } => write!(f, "index {index} is past the {table} table"),
            Fault::Fragment { index } => write!(f, "fragment {index} does not hold the tail of its file"),
            Fault::TooLarge { what, size, limit } => write!(f, "{what} of {size} bytes, over the {limit}-byte ceiling"),
            Fault::Missing(path) => write!(f, "{path}: not in the image"),
            Fault::NotADirectory(path) => write!(f, "{path}: not a directory"),
            Fault::NotAFile(path) => write!(f, "{path}: not a regular file"),
        }
    }
}

impl std::error::Error for Fault {}

impl Fault {
    /// The failure of this fault: [`Class::NotFound`] for a missing path,
    /// [`Class::Unsupported`] for a version or a compressor,
    /// [`Class::Policy`] for a ceiling, [`Class::Protocol`] for the rest.
    pub fn failure(self) -> Failure {
        let class = match self {
            Fault::Missing(_) => Class::NotFound,
            Fault::Version { .. } | Fault::Compressor(_) => Class::Unsupported,
            Fault::TooLarge { .. } => Class::Policy,
            _ => Class::Protocol,
        };
        Failure::caused(Stage::Host, class, format!("squashfs: {self}"), self)
    }
}

impl From<Fault> for Failure {
    fn from(f: Fault) -> Failure {
        f.failure()
    }
}

/// The [`Fault`] in the chain of `f`, when the image did not read.
pub fn fault(f: &Failure) -> Option<&Fault> {
    crate::cause_of(f)
}

/// An open image: the source, its superblock, and the caches of one reader.
pub struct Image<R> {
    src: R,
    sb: Superblock,
    meta: meta::Cache,
    xz: Option<Box<xz4rust::XzDecoder<'static>>>,
    /// The last fragment block: its place and its bytes. The tails of the
    /// files of a directory are often in one block.
    fragment: Option<(u64, Vec<u8>)>,
}

impl<R> std::fmt::Debug for Image<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image").field("superblock", &self.sb).finish_non_exhaustive()
    }
}

impl<R: Read + Seek> Image<R> {
    /// Read the superblock and the compressor options of `src`.
    pub fn open(mut src: R) -> Result<Image<R>> {
        let len = src.seek(SeekFrom::End(0)).map_err(|e| io_failure(0, e))?;
        if len < SUPERBLOCK_LEN as u64 {
            return Err(Fault::Past { what: "the superblock", at: 0 }.into());
        }
        let mut b = [0u8; SUPERBLOCK_LEN];
        src.seek(SeekFrom::Start(0)).and_then(|_| src.read_exact(&mut b)).map_err(|e| io_failure(0, e))?;
        let sb = Superblock::parse(&b)?;
        if sb.bytes_used > len || sb.bytes_used < SUPERBLOCK_LEN as u64 {
            return Err(Fault::Past { what: "the end of the image", at: sb.bytes_used }.into());
        }
        let mut image = Image { src, sb, meta: meta::Cache::default(), xz: None, fragment: None };
        if image.sb.flags & FLAG_COMPRESSOR_OPTIONS != 0 {
            image.options()?;
        }
        Ok(image)
    }

    /// The superblock, with the compressor options.
    pub fn superblock(&self) -> &Superblock {
        &self.sb
    }

    /// The options of xz: the dictionary must have the form that the kernel
    /// takes (2^n or 3 × 2^n), and the reader bounds it by the block size,
    /// as mksquashfs does. The block of the options is stored: the decoder
    /// needs them before it can read a compressed block.
    fn options(&mut self) -> Result<()> {
        let Compressor::Xz { .. } = self.sb.compressor else { return Ok(()) };
        let at = SUPERBLOCK_LEN as u64;
        let mut head = [0u8; 2];
        self.read_at("the compressor options", at, &mut head)?;
        let word = u16::from_le_bytes(head);
        if word & 0x8000 == 0 {
            return Err(Fault::Options("the block is compressed").into());
        }
        if word & 0x7fff < 8 {
            return Err(Fault::Options("fewer than 8 bytes").into());
        }
        let mut b = [0u8; 8];
        self.read_at("the compressor options", at + 2, &mut b)?;
        let (dictionary, filters) = (u32_at(&b, 0), u32_at(&b, 4));
        let n = dictionary.trailing_zeros();
        let shaped = dictionary != 0 && (dictionary == 1 << n || u64::from(dictionary) == 3u64 << n);
        if !shaped || dictionary > self.sb.block_size.max(METADATA_SIZE as u32) {
            return Err(Fault::Dictionary(dictionary).into());
        }
        self.sb.compressor = Compressor::Xz { dictionary, filters };
        Ok(())
    }

    /// Fill `out` from the image at `at`, which must end at or before the
    /// end of the image.
    fn read_at(&mut self, what: &'static str, at: u64, out: &mut [u8]) -> Result<()> {
        let end = at.checked_add(out.len() as u64);
        if end.is_none_or(|end| end > self.sb.bytes_used) {
            return Err(Fault::Past { what, at }.into());
        }
        self.src.seek(SeekFrom::Start(at)).and_then(|_| self.src.read_exact(out)).map_err(|e| io_failure(at, e))
    }

    /// `len` bytes of the image at `at`.
    fn bytes_at(&mut self, what: &'static str, at: u64, len: usize) -> Result<Vec<u8>> {
        let mut out = vec![0u8; len];
        self.read_at(what, at, &mut out)?;
        Ok(out)
    }
}

fn io_failure(at: u64, e: std::io::Error) -> Failure {
    Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("squashfs: a read at {at}: {e}"), e)
}
