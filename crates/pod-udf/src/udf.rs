//! The reader: open a volume, walk a path, give the extents of a file.

use std::ops::Range;

use crate::Limits;
use crate::dir::{self, Entry};
use crate::error::Error;
use crate::icb::{Data, Icb, kind};
use crate::read::ReadAt;
use crate::tag::{self, Fields, LongAd, id};
use crate::volume::{self, Map, MetaExtent, Partition, RawMap};

/// A part of a file in the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extent {
    /// `len` bytes of the file at byte `offset` of the image.
    Data { offset: u64, len: u64 },
    /// `len` bytes of the file that the image does not record: zeros.
    Zero { len: u64 },
}

impl Extent {
    /// The bytes of the file in this part.
    pub fn len(&self) -> u64 {
        match *self {
            Extent::Data { len, .. } | Extent::Zero { len } => len,
        }
    }

    /// True for a part of no byte.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The parts of a file in the image, in the order of the file, and its
/// size. The lengths of the parts add up to the size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extents {
    pub size: u64,
    pub parts: Vec<Extent>,
}

impl Extents {
    /// The one byte range of the image that holds the whole file, when the
    /// file is one recorded part.
    pub fn range(&self) -> Option<Range<u64>> {
        match self.parts[..] {
            [Extent::Data { offset, len }] => Some(offset..offset + len),
            _ => None,
        }
    }
}

/// An open UDF volume over the caller's reader.
#[derive(Debug)]
pub struct Udf<R> {
    pub(crate) reader: R,
    pub(crate) limits: Limits,
    pub(crate) block: u32,
    pub(crate) maps: Vec<Map>,
    revision: u16,
    root: LongAd,
}

/// Find `path` in the image of `reader`, with the default limits.
pub fn find<R: ReadAt>(reader: R, path: &str) -> Result<Extents, Error> {
    Udf::open(reader)?.find(path)
}

impl<R: ReadAt> Udf<R> {
    /// Open the volume with the default [`Limits`].
    pub fn open(reader: R) -> Result<Self, Error> {
        Self::open_with(reader, Limits::default())
    }

    /// Read the anchor, the volume descriptors, the partition maps (with the
    /// metadata file of a metadata partition) and the file set descriptor.
    pub fn open_with(mut reader: R, limits: Limits) -> Result<Self, Error> {
        let anchor = volume::anchor(&mut reader)?;
        let found = volume::descriptors(&mut reader, &anchor, &limits)?;
        let logical = found.logical;
        if logical.block != anchor.block {
            return Err(Error::Unsupported {
                what: "a logical block size of",
                value: format!("{} bytes with blocks of {} bytes", logical.block, anchor.block),
            });
        }
        let mut maps = Vec::new();
        for raw in &logical.maps {
            let number = match *raw {
                RawMap::Physical { number } | RawMap::Metadata { number, .. } => number,
            };
            let partition = *found
                .partitions
                .iter()
                .find(|p| p.number == number)
                .ok_or(Error::Missing { what: "partition descriptor of a partition map" })?;
            maps.push(match *raw {
                RawMap::Physical { .. } => Map::Physical(partition),
                RawMap::Metadata { file, mirror, .. } => {
                    let extents = metadata(&mut reader, anchor.block, partition, file, &limits).or_else(|main| {
                        metadata(&mut reader, anchor.block, partition, mirror, &limits).map_err(|_| main)
                    })?;
                    Map::Metadata(partition, extents)
                }
            });
        }
        let mut udf = Udf { reader, limits, block: anchor.block, maps, revision: logical.revision, root: logical.fsd };
        let fsd = logical.fsd;
        let (offset, buf) = udf.block(fsd.partition, fsd.lbn, 0)?;
        tag::expect(&buf, offset, "file set descriptor", &[id::FSD], Some(fsd.lbn))?;
        udf.root = LongAd::read(&Fields::new(&buf, offset), 400)?;
        Ok(udf)
    }

    /// The block size of the volume in bytes.
    pub fn block_size(&self) -> u32 {
        self.block
    }

    /// The UDF revision of the domain of the logical volume, as BCD:
    /// 0x0102 for UDF 1.02, 0x0250 for UDF 2.50. A writer that sets no
    /// domain gives 0.
    pub fn revision(&self) -> u16 {
        self.revision
    }

    /// The limits of this volume.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Give back the reader.
    pub fn into_inner(self) -> R {
        self.reader
    }

    /// The extents of the file at `path` (components split at `/` or `\`).
    /// A name matches the same name first, else a name that is the same
    /// with no regard to case.
    pub fn find(&mut self, path: &str) -> Result<Extents, Error> {
        let (shown, icb) = self.walk(path)?;
        if icb.file_type != kind::FILE && icb.file_type != kind::REAL_TIME {
            return Err(Error::NotAFile { path: shown, file_type: icb.file_type });
        }
        self.extents(&icb)
    }

    /// The entries of the directory at `path`, in the order of the
    /// directory, with no parent entry and no deleted entry.
    pub fn list(&mut self, path: &str) -> Result<Vec<Entry>, Error> {
        let (shown, icb) = self.walk(path)?;
        if icb.file_type != kind::DIRECTORY {
            return Err(Error::NotADirectory { path: shown });
        }
        self.entries(&icb)
    }

    /// The bytes of a file, at most `max`; a larger file is
    /// [`Error::Limit`].
    pub fn read(&mut self, extents: &Extents, max: u64) -> Result<Vec<u8>, Error> {
        if extents.size > max {
            return Err(Error::Limit { what: "bytes of a read", limit: max });
        }
        let mut out = Vec::with_capacity(extents.size as usize);
        for part in &extents.parts {
            let len = usize::try_from(part.len()).map_err(|_| Error::Limit { what: "bytes of a read", limit: max })?;
            match *part {
                Extent::Data { offset, .. } => out.extend(volume::read(&mut self.reader, offset, len)?),
                Extent::Zero { .. } => out.resize(out.len() + len, 0),
            }
        }
        Ok(out)
    }

    /// The file entry at `path`, and the path with the names as the image
    /// spells them.
    fn walk(&mut self, path: &str) -> Result<(String, Icb), Error> {
        let mut names = Vec::new();
        for name in path.split(['/', '\\']).filter(|n| !n.is_empty() && *n != ".") {
            if name == ".." {
                return Err(Error::Path { path: path.into(), reason: "a `..` component" });
            }
            if names.len() == self.limits.depth {
                return Err(Error::Limit { what: "components of a path", limit: self.limits.depth as u64 });
            }
            names.push(name);
        }
        let mut icb = self.icb(self.root, 0)?;
        let mut shown = String::new();
        for name in names {
            if icb.file_type != kind::DIRECTORY {
                return Err(Error::NotADirectory { path: if shown.is_empty() { "/".into() } else { shown } });
            }
            let entries = self.entries(&icb)?;
            let Some(entry) = dir::lookup(&entries, name) else {
                return Err(Error::NotFound { path: format!("{shown}/{name}") });
            };
            shown = format!("{shown}/{}", entry.name);
            icb = self.icb(entry.icb, icb.offset)?;
        }
        if shown.is_empty() {
            shown.push('/');
        }
        Ok((shown, icb))
    }
}

/// The extents of the metadata file at block `lbn` of `partition`: the
/// blocks of a metadata partition (UDF 2.2.13).
fn metadata<R: ReadAt>(
    reader: &mut R,
    block: u32,
    partition: Partition,
    lbn: u32,
    limits: &Limits,
) -> Result<Vec<MetaExtent>, Error> {
    let mut physical = Udf {
        reader,
        limits: *limits,
        block,
        maps: vec![Map::Physical(partition)],
        revision: 0,
        root: LongAd { len: 0, lbn: 0, partition: 0 },
    };
    let icb = physical.icb(LongAd { len: block, lbn, partition: 0 }, 0)?;
    if icb.file_type != kind::METADATA && icb.file_type != kind::METADATA_MIRROR {
        return Err(Error::Corrupt { offset: icb.offset, what: "the metadata file has another file type" });
    }
    let Data::Ads(ads) = &icb.data else {
        return Err(Error::Unsupported { what: "a metadata file with", value: "embedded data".into() });
    };
    let mut extents = Vec::new();
    let mut file = 0u64;
    for ad in ads {
        let len = u64::from(ad.len);
        if ad.kind == 0 {
            if ad.partition != 0 {
                return Err(Error::Unsupported { what: "a metadata file in", value: "another partition".into() });
            }
            let mut ranges = Vec::new();
            physical.map(0, ad.lbn, len, icb.offset, &mut ranges)?;
            extents.push(MetaExtent { file, lbn: ad.lbn, len });
        }
        file += len;
    }
    Ok(extents)
}
