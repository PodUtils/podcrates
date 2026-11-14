//! The information control block of a file (ECMA-167 4/14.6, 4/14.9,
//! 4/14.17): the file entry or the extended file entry, its allocation
//! descriptors with the allocation extent descriptors that continue them,
//! and the extents of the file in the image.

use crate::error::Error;
use crate::read::ReadAt;
use crate::tag::{self, Fields, LongAd, id};
use crate::udf::{Extent, Extents, Udf};
use crate::volume;

/// The file types of the ICB tag (ECMA-167 4/14.6.6, UDF 2.3.5.2).
pub(crate) mod kind {
    pub(crate) const DIRECTORY: u8 = 4;
    pub(crate) const FILE: u8 = 5;
    pub(crate) const REAL_TIME: u8 = 249;
    pub(crate) const METADATA: u8 = 250;
    pub(crate) const METADATA_MIRROR: u8 = 251;
}

/// The ICB strategy that UDF records (ECMA-167 4/A.5).
const STRATEGY: u16 = 4;

/// An allocation descriptor: its type (0 recorded, 1 allocated and not
/// recorded, 2 neither), its bytes, and its first block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ad {
    pub(crate) kind: u8,
    pub(crate) len: u32,
    pub(crate) lbn: u32,
    pub(crate) partition: u16,
}

/// Where the bytes of a file are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Data {
    /// In the file entry itself, from this byte of the image.
    Embedded(u64),
    Ads(Vec<Ad>),
}

/// A file entry, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Icb {
    /// The byte offset of the file entry in the image.
    pub(crate) offset: u64,
    pub(crate) file_type: u8,
    pub(crate) size: u64,
    pub(crate) data: Data,
}

impl<R: ReadAt> Udf<R> {
    /// The block `lbn` of the partition reference `partition`: its byte
    /// offset and its bytes. `offset` is the descriptor that holds the
    /// address, for an error.
    pub(crate) fn block(&mut self, partition: u16, lbn: u32, offset: u64) -> Result<(u64, Vec<u8>), Error> {
        let mut ranges = Vec::new();
        self.map(partition, lbn, u64::from(self.block), offset, &mut ranges)?;
        match ranges[..] {
            [(at, _)] => Ok((at, volume::read(&mut self.reader, at, self.block as usize)?)),
            _ => Err(Error::Corrupt { offset, what: "a block across two extents of the metadata file" }),
        }
    }

    /// The byte ranges of `len` bytes from block `lbn` of a partition
    /// reference.
    pub(crate) fn map(
        &self,
        partition: u16,
        lbn: u32,
        len: u64,
        offset: u64,
        out: &mut Vec<(u64, u64)>,
    ) -> Result<(), Error> {
        let map = self
            .maps
            .get(usize::from(partition))
            .ok_or(Error::Corrupt { offset, what: "a partition reference number with no partition map" })?;
        map.ranges(self.block, lbn, len, offset, out)
    }

    /// Read the file entry at `at`.
    pub(crate) fn icb(&mut self, at: LongAd, from: u64) -> Result<Icb, Error> {
        let (offset, buf) = self.block(at.partition, at.lbn, from)?;
        let ident = tag::expect(&buf, offset, "file entry", &[id::FE, id::EFE], Some(at.lbn))?;
        let f = Fields::new(&buf, offset);
        let strategy = f.u16(20)?;
        if strategy != STRATEGY {
            return Err(Error::Unsupported { what: "the ICB strategy type", value: strategy.to_string() });
        }
        let file_type = f.u8(27)?;
        let flags = f.u16(34)?;
        let size = f.u64(56)?;
        let (lengths, base) = if ident == id::FE { (168, 176) } else { (208, 216) };
        let (ea, ad) = (f.u32(lengths)? as usize, f.u32(lengths + 4)? as usize);
        let start = base + ea;
        let ads = f.slice(start, ad).map_err(|_| Error::Corrupt {
            offset,
            what: "the allocation descriptors go past the end of the file entry",
        })?;
        let data = match flags & 7 {
            0 => Data::Ads(self.ads(ads, true, at.partition, offset)?),
            1 => Data::Ads(self.ads(ads, false, at.partition, offset)?),
            3 if size <= ad as u64 => Data::Embedded(offset + start as u64),
            3 => return Err(Error::Corrupt { offset, what: "embedded data shorter than the file" }),
            2 => return Err(Error::Unsupported { what: "the allocation descriptor type", value: "ext_ad".into() }),
            _ => return Err(Error::Corrupt { offset, what: "an allocation descriptor type of 4 to 7" }),
        };
        Ok(Icb { offset, file_type, size, data })
    }

    /// The allocation descriptors of `first`, `short_ad` or `long_ad`, and
    /// of each allocation extent descriptor that continues them. A
    /// `short_ad` is in the partition of the descriptor that holds it.
    fn ads(&mut self, first: &[u8], short: bool, partition: u16, offset: u64) -> Result<Vec<Ad>, Error> {
        let size = if short { 8 } else { 16 };
        let limit = self.limits.extents;
        let over = || Error::Limit { what: "allocation descriptors of a file", limit: limit as u64 };
        let (mut list, mut partition, mut offset) = (first.to_vec(), partition, offset);
        let mut out = Vec::new();
        let mut continued = 0usize;
        loop {
            if list.len() % size != 0 {
                return Err(Error::Corrupt { offset, what: "allocation descriptors of a part of a descriptor" });
            }
            let mut next = None;
            for raw in list.chunks_exact(size) {
                let f = Fields::new(raw, offset);
                let head = f.u32(0)?;
                let (kind, len, lbn) = ((head >> 30) as u8, head & 0x3fff_ffff, f.u32(4)?);
                let partition = if short { partition } else { f.u16(8)? };
                if len == 0 {
                    break;
                }
                if kind == 3 {
                    next = Some(LongAd { len, lbn, partition });
                    break;
                }
                if out.len() + continued >= limit {
                    return Err(over());
                }
                out.push(Ad { kind, len, lbn, partition });
            }
            let Some(next) = next else { return Ok(out) };
            continued += 1;
            if out.len() + continued > limit {
                return Err(over());
            }
            let (at, buf) = self.block(next.partition, next.lbn, offset)?;
            tag::expect(&buf, at, "allocation extent descriptor", &[id::AED], Some(next.lbn))?;
            let f = Fields::new(&buf, at);
            let len = f.u32(20)? as usize;
            list = f.slice(24, len)?.to_vec();
            (partition, offset) = (next.partition, at);
        }
    }

    /// The extents of the bytes of a file in the image, in order, up to
    /// its size. Adjacent ranges are one part.
    pub(crate) fn extents(&self, icb: &Icb) -> Result<Extents, Error> {
        let size = icb.size;
        let ads = match &icb.data {
            Data::Embedded(at) => {
                let parts = if size > 0 { vec![Extent::Data { offset: *at, len: size }] } else { Vec::new() };
                return Ok(Extents { size, parts });
            }
            Data::Ads(ads) => ads,
        };
        let mut parts = Vec::new();
        let mut total = 0u64;
        for ad in ads {
            if total >= size {
                break;
            }
            let len = u64::from(ad.len).min(size - total);
            if ad.kind == 0 {
                let mut ranges = Vec::new();
                self.map(ad.partition, ad.lbn, len, icb.offset, &mut ranges)?;
                for (offset, len) in ranges {
                    add(&mut parts, Extent::Data { offset, len });
                }
            } else {
                add(&mut parts, Extent::Zero { len });
            }
            total += len;
        }
        if total < size {
            return Err(Error::Corrupt {
                offset: icb.offset,
                what: "the allocation descriptors end before the size of the file",
            });
        }
        Ok(Extents { size, parts })
    }
}

/// Add a part; it extends the last part when it follows it.
fn add(parts: &mut Vec<Extent>, part: Extent) {
    match (parts.last_mut(), part) {
        (Some(Extent::Data { offset, len }), Extent::Data { offset: at, len: n }) if *offset + *len == at => *len += n,
        (Some(Extent::Zero { len }), Extent::Zero { len: n }) => *len += n,
        _ => parts.push(part),
    }
}
