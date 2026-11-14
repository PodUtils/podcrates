//! The volume structure (ECMA-167 part 3): the anchor at block 256, the
//! volume descriptor sequence, the partition descriptors, the logical
//! volume descriptor and its partition maps, and the map of a logical
//! block to bytes of the image.

use std::io;

use crate::Limits;
use crate::error::Error;
use crate::read::ReadAt;
use crate::tag::{self, Fields, LongAd, id};

/// The block sizes that the reader tries, in order: CD and DVD media have
/// 2048, a hard disk image 512 or 4096.
const BLOCK_SIZES: [u32; 3] = [2048, 512, 4096];
/// The block of the anchor volume descriptor pointer (ECMA-167 3/8.4.2.1).
const ANCHOR: u32 = 256;
/// The most partition maps of a logical volume.
const MAX_MAPS: u32 = 64;
/// The partition type identifier of a metadata partition map (UDF 2.2.10).
const METADATA_MAP: &[u8] = b"*UDF Metadata Partition";

/// Read `len` bytes of the image at `offset`.
pub(crate) fn read<R: ReadAt>(reader: &mut R, offset: u64, len: usize) -> Result<Vec<u8>, Error> {
    let mut buf = vec![0u8; len];
    reader.read_at(offset, &mut buf).map_err(|source| Error::Read { offset, len, source })?;
    Ok(buf)
}

/// An extent of blocks of the image (`extent_ad`, ECMA-167 3/7.1).
#[derive(Clone, Copy, Debug)]
struct Run {
    len: u32,
    block: u32,
}

/// The anchor: the block size and the two volume descriptor sequences.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub(crate) block: u32,
    main: Run,
    reserve: Run,
}

/// Find the anchor at block 256 for each block size. An image too short
/// for a size has no anchor of that size.
pub(crate) fn anchor<R: ReadAt>(reader: &mut R) -> Result<Anchor, Error> {
    for block in BLOCK_SIZES {
        let offset = u64::from(ANCHOR) * u64::from(block);
        let buf = match read(reader, offset, block as usize) {
            Ok(buf) => buf,
            Err(Error::Read { source, .. }) if source.kind() == io::ErrorKind::UnexpectedEof => continue,
            Err(e) => return Err(e),
        };
        if tag::check(&buf, Some(ANCHOR)) == Ok(id::AVDP) {
            let f = Fields::new(&buf, offset);
            let main = Run { len: f.u32(16)?, block: f.u32(20)? };
            let reserve = Run { len: f.u32(24)?, block: f.u32(28)? };
            return Ok(Anchor { block, main, reserve });
        }
    }
    Err(Error::NotUdf)
}

/// A partition descriptor (ECMA-167 3/10.5): its number and its blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Partition {
    pub(crate) number: u16,
    pub(crate) start: u32,
    pub(crate) len: u32,
}

/// A partition map of the logical volume (ECMA-167 3/10.7, UDF 2.2.8 to
/// 2.2.10), before the reader resolves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RawMap {
    Physical { number: u16 },
    Metadata { number: u16, file: u32, mirror: u32 },
}

/// The logical volume descriptor (ECMA-167 3/10.6).
#[derive(Clone, Debug)]
pub(crate) struct Logical {
    pub(crate) block: u32,
    /// The UDF revision of the domain identifier, as BCD (0x0102, 0x0260).
    pub(crate) revision: u16,
    /// The file set descriptor.
    pub(crate) fsd: LongAd,
    pub(crate) maps: Vec<RawMap>,
}

/// The prevailing descriptors of a sequence.
#[derive(Clone, Debug)]
pub(crate) struct Descriptors {
    pub(crate) partitions: Vec<Partition>,
    pub(crate) logical: Logical,
}

/// The descriptors of the main sequence; when it fails or lacks one, those
/// of the reserve sequence; else the failure of the main sequence.
pub(crate) fn descriptors<R: ReadAt>(reader: &mut R, anchor: &Anchor, limits: &Limits) -> Result<Descriptors, Error> {
    match sequence(reader, anchor.block, anchor.main, limits) {
        Ok(found) => Ok(found),
        Err(main) => sequence(reader, anchor.block, anchor.reserve, limits).map_err(|_| main),
    }
}

fn sequence<R: ReadAt>(reader: &mut R, block: u32, run: Run, limits: &Limits) -> Result<Descriptors, Error> {
    let mut partitions: Vec<(u32, Partition)> = Vec::new();
    let mut logical: Option<(u32, Logical)> = None;
    let (mut at, mut left) = (run.block, run.len / block);
    let mut seen = 0usize;
    while left > 0 {
        seen += 1;
        if seen > limits.descriptors {
            return Err(Error::Limit { what: "volume descriptors", limit: limits.descriptors as u64 });
        }
        let offset = u64::from(at) * u64::from(block);
        let buf = read(reader, offset, block as usize)?;
        let ident = tag::check(&buf, Some(at)).map_err(|fault| Error::Descriptor {
            offset,
            what: "volume descriptor",
            fault,
        })?;
        let f = Fields::new(&buf, offset);
        match ident {
            id::TD => break,
            id::VDP => {
                (left, at) = (f.u32(20)? / block, f.u32(24)?);
                continue;
            }
            id::PD => {
                let (number, vdsn) = (f.u16(22)?, f.u32(16)?);
                let found = Partition { number, start: f.u32(188)?, len: f.u32(192)? };
                partitions.retain(|(n, p)| p.number != number || *n > vdsn);
                if !partitions.iter().any(|(_, p)| p.number == number) {
                    partitions.push((vdsn, found));
                }
            }
            id::LVD => {
                let vdsn = f.u32(16)?;
                if logical.as_ref().is_none_or(|(n, _)| vdsn >= *n) {
                    logical = Some((vdsn, logical_volume(&f)?));
                }
            }
            _ => {}
        }
        at =
            at.checked_add(1).ok_or(Error::Corrupt { offset, what: "a volume descriptor sequence past block 2^32" })?;
        left -= 1;
    }
    let (_, logical) = logical.ok_or(Error::Missing { what: "logical volume descriptor" })?;
    if partitions.is_empty() {
        return Err(Error::Missing { what: "partition descriptor" });
    }
    Ok(Descriptors { partitions: partitions.into_iter().map(|(_, p)| p).collect(), logical })
}

fn logical_volume(f: &Fields<'_>) -> Result<Logical, Error> {
    let block = f.u32(212)?;
    let revision = f.u16(240)?;
    let fsd = LongAd::read(f, 248)?;
    let table = f.u32(264)? as usize;
    let count = f.u32(268)?;
    if count > MAX_MAPS {
        return Err(Error::Limit { what: "partition maps", limit: MAX_MAPS.into() });
    }
    let table = f.slice(440, table)?;
    let mut maps = Vec::new();
    let mut at = 0usize;
    for _ in 0..count {
        let m = Fields::new(table, f.offset + 440);
        let (kind, len) = (m.u8(at)?, usize::from(m.u8(at + 1)?));
        if len < 2 {
            return Err(Error::Corrupt { offset: f.offset, what: "a partition map shorter than its head" });
        }
        let map = m.slice(at, len)?;
        let m = Fields::new(map, f.offset + 440 + at as u64);
        maps.push(match kind {
            1 => RawMap::Physical { number: m.u16(4)? },
            2 => {
                // The identifier of a regid: 23 bytes after its flags,
                // padded with zeros.
                let ident = m.slice(5, 23)?;
                let ident = &ident[..ident.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1)];
                if ident != METADATA_MAP {
                    return Err(Error::Unsupported {
                        what: "a partition map",
                        value: format!("{:?}", String::from_utf8_lossy(ident)),
                    });
                }
                RawMap::Metadata { number: m.u16(38)?, file: m.u32(40)?, mirror: m.u32(44)? }
            }
            other => return Err(Error::Unsupported { what: "a partition map of type", value: other.to_string() }),
        });
        at += len;
    }
    Ok(Logical { block, revision, fsd, maps })
}

/// A part of the metadata file: `len` bytes from byte `file` of the file
/// are at block `lbn` of the physical partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MetaExtent {
    pub(crate) file: u64,
    pub(crate) lbn: u32,
    pub(crate) len: u64,
}

/// A resolved partition map: the blocks of a partition reference number.
#[derive(Clone, Debug)]
pub(crate) enum Map {
    Physical(Partition),
    /// A metadata partition: its blocks are the bytes of the metadata file,
    /// in the physical partition.
    Metadata(Partition, Vec<MetaExtent>),
}

/// Add a byte range; it extends the last range when it follows it.
pub(crate) fn push(out: &mut Vec<(u64, u64)>, offset: u64, len: u64) {
    match out.last_mut() {
        Some((at, n)) if *at + *n == offset => *n += len,
        _ if len > 0 => out.push((offset, len)),
        _ => {}
    }
}

impl Map {
    /// The byte ranges of the image of `len` bytes from block `lbn`.
    /// `offset` is the descriptor that holds the address, for an error.
    pub(crate) fn ranges(
        &self,
        block: u32,
        lbn: u32,
        len: u64,
        offset: u64,
        out: &mut Vec<(u64, u64)>,
    ) -> Result<(), Error> {
        let block = u64::from(block);
        let past = || Error::Corrupt { offset, what: "an extent past the end of its partition" };
        match self {
            Map::Physical(p) => {
                if u64::from(lbn) + len.div_ceil(block) > u64::from(p.len) {
                    return Err(past());
                }
                push(out, (u64::from(p.start) + u64::from(lbn)) * block, len);
            }
            Map::Metadata(p, extents) => {
                let (mut pos, end) = (u64::from(lbn) * block, u64::from(lbn) * block + len);
                while pos < end {
                    let i = extents.partition_point(|e| e.file + e.len <= pos);
                    let e = extents.get(i).filter(|e| e.file <= pos).ok_or_else(past)?;
                    let take = (e.file + e.len).min(end) - pos;
                    let at = (u64::from(p.start) + u64::from(e.lbn)) * block + (pos - e.file);
                    push(out, at, take);
                    pos += take;
                }
            }
        }
        Ok(())
    }
}
