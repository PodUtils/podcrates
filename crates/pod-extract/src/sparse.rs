//! A GNU sparse file of the PAX form: the map of its regions of data.
//!
//! **Why.** GNU tar `-S --format=posix` stores only the regions of data of a
//! sparse file, with a map that gives the offset of each region. The `tar`
//! reader does not read the map: it gives the regions with no holes, and
//! from version 0.1 under the name `GNUSparseFile.N/NAME`.
//!
//! **The three versions** (the GNU tar manual, "PAX Format, Versions 0.0 and
//! 0.1" and "PAX Format, Version 1.0"):
//!
//! * 0.0: `GNU.sparse.size` (the real size), `GNU.sparse.numblocks`, then a
//!   `GNU.sparse.offset` and a `GNU.sparse.numbytes` for each region, in
//!   this order.
//! * 0.1: `GNU.sparse.size`, `GNU.sparse.numblocks`, `GNU.sparse.map`
//!   (`OFFSET,SIZE,...`) and `GNU.sparse.name`, the real name.
//! * 1.0: `GNU.sparse.major=1`, `GNU.sparse.minor=0`, `GNU.sparse.name` and
//!   `GNU.sparse.realsize`. The map is at the start of the data: decimal
//!   numbers, one on each line (the count of regions, then the offset and
//!   the size of each region), padded with NUL bytes to a block.
//!
//! **Bounds.** The map is input of an untrusted archive. The crate reads
//! [`REGIONS_MAX`] regions at most, and a map in the data of [`MAP_MAX`]
//! bytes at most. Each region starts at or after the end of the region
//! before it and ends inside the real size, and the regions hold each byte
//! of the data of the member after the map. A map that does not hold
//! refuses the layer.
//!
//! **The write.** Each region goes to its offset through the descriptor of
//! the file, and the real size is set last (`ftruncate`): each hole stays a
//! hole ([`crate::apply`]).

use std::io::Read;

use crate::error::{self, Result};
use crate::safety::Refusal;

/// The regions of one sparse file that the crate reads. A map of
/// [`MAP_MAX`] bytes holds this count at most: each region is 4 bytes of
/// text or more (`0\n0\n`, `0,0,`).
pub const REGIONS_MAX: usize = 256 * 1024;
/// The map at the start of the data (version 1.0): the bound of the PAX
/// header that holds the map of the other versions ([`crate::pax::PAX_MAX`]).
pub const MAP_MAX: u64 = crate::pax::PAX_MAX;
/// The longest number of a map: `u64::MAX` has 20 digits.
const DIGITS_MAX: usize = 20;
const BLOCK: usize = 512;

/// One region of data: its offset in the file and its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub offset: u64,
    pub len: u64,
}

/// Where the map of a sparse file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Map {
    /// In the PAX records (versions 0.0 and 0.1), not checked yet.
    Records(Vec<Region>),
    /// At the start of the data of the member (version 1.0).
    Data,
}

/// The sparse records of one member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sparse {
    /// The size of the file with its holes.
    pub real_size: u64,
    pub map: Map,
}

/// A sparse file whose map the crate read and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub real_size: u64,
    pub regions: Vec<Region>,
}

/// The sparse records of a member, and its real name (`GNU.sparse.name`).
pub(crate) type Found = (Sparse, Option<Vec<u8>>);

/// The `GNU.sparse.` records of one PAX header, in their order.
#[derive(Debug, Default)]
pub(crate) struct Records {
    seen: bool,
    major: Option<Vec<u8>>,
    minor: Option<Vec<u8>>,
    size: Option<u64>,
    realsize: Option<u64>,
    numblocks: Option<u64>,
    map: Option<Vec<u8>>,
    /// The `GNU.sparse.offset` and `GNU.sparse.numbytes` values of 0.0.
    pairs: Vec<u64>,
    name: Option<Vec<u8>>,
}

impl Records {
    /// Take one record. `key` is the key after `GNU.sparse.`.
    pub(crate) fn push(&mut self, key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
        self.seen = true;
        match key {
            b"major" => self.major = Some(value.to_vec()),
            b"minor" => self.minor = Some(value.to_vec()),
            // An empty name keeps the name of the header (Go's `archive/tar`).
            b"name" => self.name = (!value.is_empty()).then(|| value.to_vec()),
            b"size" => self.size = Some(number(key, value)?),
            b"realsize" => self.realsize = Some(number(key, value)?),
            b"numblocks" => self.numblocks = Some(number(key, value)?),
            b"map" => self.map = Some(value.to_vec()),
            b"offset" | b"numbytes" => {
                let want: &[u8] = if self.pairs.len().is_multiple_of(2) { b"offset" } else { b"numbytes" };
                if key != want {
                    return Err(format!(
                        "the GNU sparse record GNU.sparse.{} is not after a GNU.sparse.{}",
                        shown(key),
                        if want == b"offset" { "numbytes" } else { "offset" }
                    ));
                }
                if self.pairs.len() >= 2 * REGIONS_MAX {
                    return Err(too_many());
                }
                self.pairs.push(number(key, value)?);
            }
            _ => return Err(format!("the PAX record GNU.sparse.{} is not a record of a GNU sparse map", shown(key))),
        }
        Ok(())
    }

    /// The sparse records of the member and its real name, or `None` when
    /// the header has no `GNU.sparse.` record.
    pub(crate) fn finish(self) -> std::result::Result<Option<Found>, String> {
        if !self.seen {
            return Ok(None);
        }
        let v1 = match (self.major.as_deref(), self.minor.as_deref()) {
            (Some(b"1"), Some(b"0")) => true,
            (Some(b"0"), Some(b"0" | b"1")) | (None, None) => false,
            (major, minor) => {
                let v = |x: Option<&[u8]>| x.map_or("(none)".to_string(), shown);
                return Err(format!(
                    "the GNU sparse version {}.{} is not a version that this crate reads (0.0, 0.1, 1.0)",
                    v(major),
                    v(minor)
                ));
            }
        };
        // Version 1.0 gives `realsize`, the others `size`.
        let real_size = if v1 { self.realsize.or(self.size) } else { self.size.or(self.realsize) }
            .ok_or("a GNU sparse map with no real size (GNU.sparse.size or GNU.sparse.realsize)")?;
        if real_size > i64::MAX as u64 {
            return Err(format!("the real size {real_size} of a GNU sparse file is past the size of a Linux file"));
        }
        let map = match (v1, self.map, self.pairs.is_empty()) {
            (true, None, true) => Map::Data,
            (true, _, _) => return Err("a GNU sparse map of version 1.0 with a map in its PAX records".into()),
            (false, Some(_), false) => return Err("a GNU sparse map in GNU.sparse.map and in GNU.sparse.offset".into()),
            (false, map, _) => {
                let numblocks = self.numblocks.ok_or("a GNU sparse map with no GNU.sparse.numblocks")?;
                let nums = match map {
                    Some(m) => list(&m)?,
                    None => self.pairs,
                };
                if !nums.len().is_multiple_of(2) || nums.len() as u64 / 2 != numblocks {
                    return Err(format!(
                        "the GNU sparse map has {} numbers, and GNU.sparse.numblocks gives {numblocks} regions",
                        nums.len()
                    ));
                }
                Map::Records(regions(&nums))
            }
        };
        Ok(Some((Sparse { real_size, map }, self.name)))
    }
}

/// The checked layout of a file entry with sparse records: the map of its
/// records, or the map at the start of its data (version 1.0), read from
/// `entry`. `None` with no sparse records. A map that does not hold, or
/// sparse records on an entry that is not a regular file, refuse the layer.
pub(crate) fn layout<R: Read>(
    sparse: Option<&Sparse>,
    entry: &mut tar::Entry<'_, R>,
    layer: &str,
    path: &str,
) -> Result<Option<Layout>> {
    let Some(s) = sparse else {
        return Ok(None);
    };
    let refuse = |why: String| error::refuse(layer, path, Refusal::Member(why));
    if !entry.header().entry_type().is_file() {
        return Err(refuse("the entry has a GNU sparse map and is not a regular file".into()));
    }
    let size = entry.size();
    let (regions, map_len) = match &s.map {
        Map::Records(r) => (r.clone(), 0),
        Map::Data => match read_map(entry, size).map_err(error::io)? {
            Ok(x) => x,
            Err(why) => return Err(refuse(why)),
        },
    };
    check(&regions, s.real_size).map_err(refuse)?;
    // `check` keeps each end inside the real size: the sum does not overflow.
    let held: u64 = regions.iter().map(|r| r.len).sum();
    if held != size - map_len {
        let why = format!("the regions of the GNU sparse map hold {held} bytes, and the data has {}", size - map_len);
        return Err(refuse(why));
    }
    Ok(Some(Layout { real_size: s.real_size, regions }))
}

/// Each region starts at or after the end of the region before it and ends
/// inside the real size (Go's `archive/tar` `validateSparseEntries`).
fn check(regions: &[Region], real_size: u64) -> std::result::Result<(), String> {
    let mut end = 0;
    for r in regions {
        let Some(e) = r.offset.checked_add(r.len).filter(|e| *e <= real_size) else {
            return Err(format!(
                "the GNU sparse region at {} of {} bytes ends past the real size {real_size}",
                r.offset, r.len
            ));
        };
        if r.offset < end {
            return Err(format!("the GNU sparse region at {} is not in order after the end {end}", r.offset));
        }
        end = e;
    }
    Ok(())
}

/// The map at the start of the data of version 1.0, and its length in
/// bytes (whole blocks). The outer error is a read that failed.
type MapRead = std::result::Result<(Vec<Region>, u64), String>;

fn read_map(data: &mut dyn Read, size: u64) -> std::io::Result<MapRead> {
    let (mut nums, mut want): (Vec<u64>, Option<usize>) = (Vec::new(), None);
    let (mut n, mut digits, mut read) = (0u64, 0usize, 0u64);
    let mut block = [0u8; BLOCK];
    loop {
        if read + BLOCK as u64 > size.min(MAP_MAX) {
            return Ok(Err(format!(
                "the GNU sparse map at the start of the data is longer than {} bytes",
                size.min(MAP_MAX)
            )));
        }
        data.read_exact(&mut block)?;
        read += BLOCK as u64;
        for b in block {
            match b {
                b'0'..=b'9' if digits < DIGITS_MAX => {
                    digits += 1;
                    let Some(x) = n.checked_mul(10).and_then(|x| x.checked_add(u64::from(b - b'0'))) else {
                        return Ok(Err("a number of the GNU sparse map is past u64".into()));
                    };
                    n = x;
                }
                b'0'..=b'9' => {
                    return Ok(Err(format!("a number of the GNU sparse map has more than {DIGITS_MAX} digits")));
                }
                b'\n' if digits > 0 => {
                    nums.push(n);
                    (n, digits) = (0, 0);
                    if want.is_none() {
                        if nums[0] > REGIONS_MAX as u64 {
                            return Ok(Err(too_many()));
                        }
                        want = Some(1 + 2 * nums[0] as usize);
                    }
                    if Some(nums.len()) == want {
                        // The rest of the block is padding.
                        return Ok(Ok((regions(&nums[1..]), read)));
                    }
                }
                _ => {
                    return Ok(Err(format!("the byte 0x{b:02x} in the GNU sparse map is not a digit or a newline")));
                }
            }
        }
    }
}

/// The regions of a list of numbers: an offset and a length for each.
fn regions(nums: &[u64]) -> Vec<Region> {
    nums.as_chunks::<2>().0.iter().map(|[offset, len]| Region { offset: *offset, len: *len }).collect()
}

/// The numbers of `GNU.sparse.map`, separated by commas. An empty map has
/// no number.
fn list(map: &[u8]) -> std::result::Result<Vec<u64>, String> {
    if map.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for v in map.split(|b| *b == b',') {
        if out.len() >= 2 * REGIONS_MAX {
            return Err(too_many());
        }
        out.push(number(b"map", v)?);
    }
    Ok(out)
}

fn number(key: &[u8], value: &[u8]) -> std::result::Result<u64, String> {
    let n = (!value.is_empty() && value.iter().all(u8::is_ascii_digit))
        .then(|| std::str::from_utf8(value).ok()?.parse::<u64>().ok())
        .flatten();
    n.ok_or_else(|| format!("the GNU sparse record GNU.sparse.{} {:?} is not a number", shown(key), shown(value)))
}

fn too_many() -> String {
    format!("a GNU sparse map of more than {REGIONS_MAX} regions")
}

/// The start of a key or a value for a message.
fn shown(b: &[u8]) -> String {
    String::from_utf8_lossy(&b[..b.len().min(40)]).into_owned()
}

#[cfg(test)]
mod tests;
