//! A directory: its file identifier descriptors (ECMA-167 4/14.4) and their
//! names in OSTA compressed unicode (UDF 2.1.1).

use crate::error::Error;
use crate::icb::Icb;
use crate::read::ReadAt;
use crate::tag::{self, Fields, LongAd, id};
use crate::udf::{Extent, Udf};
use crate::volume;

/// The fixed part of a file identifier descriptor.
const FID_HEAD: usize = 38;
/// The file characteristics (ECMA-167 4/14.4.3).
const IS_DIRECTORY: u8 = 0x02;
const IS_DELETED: u8 = 0x04;
const IS_PARENT: u8 = 0x08;

/// An entry of a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The name, decoded from OSTA compressed unicode.
    pub name: String,
    /// The entry says that it is a directory.
    pub directory: bool,
    pub(crate) icb: LongAd,
}

/// The entries of a directory: no parent entry and no deleted entry.
/// `offset` is the first byte of the directory in the image, for an error.
pub(crate) fn parse(buf: &[u8], offset: u64, limit: usize) -> Result<Vec<Entry>, Error> {
    let mut entries = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        let at = offset + pos as u64;
        let rest = &buf[pos..];
        if rest.len() < FID_HEAD {
            return Err(Error::Corrupt {
                offset: at,
                what: "a file identifier descriptor past the end of its directory",
            });
        }
        tag::expect(rest, at, "file identifier descriptor", &[id::FID], None)?;
        let f = Fields::new(rest, at);
        let characteristics = f.u8(18)?;
        let name_len = usize::from(f.u8(19)?);
        let icb = LongAd::read(&f, 20)?;
        let use_len = usize::from(f.u16(36)?);
        let name = f.slice(FID_HEAD + use_len, name_len)?;
        pos += (FID_HEAD + use_len + name_len).next_multiple_of(4);
        if characteristics & (IS_DELETED | IS_PARENT) != 0 {
            continue;
        }
        if entries.len() >= limit {
            return Err(Error::Limit { what: "entries of a directory", limit: limit as u64 });
        }
        let name =
            decode(name).ok_or(Error::Corrupt { offset: at, what: "a name that is not OSTA compressed unicode" })?;
        entries.push(Entry { name, directory: characteristics & IS_DIRECTORY != 0, icb });
    }
    Ok(entries)
}

/// A name in OSTA compressed unicode: a compression ID of 8 (one byte for
/// each character) or 16 (UTF-16, big end first); UDF 2.50 adds 254 and
/// 255, read the same way. An unpaired surrogate becomes U+FFFD.
pub(crate) fn decode(bytes: &[u8]) -> Option<String> {
    match bytes.split_first() {
        None => Some(String::new()),
        Some((8 | 254, rest)) => Some(rest.iter().map(|b| char::from(*b)).collect()),
        Some((16 | 255, rest)) if rest.len() % 2 == 0 => {
            let units = rest.as_chunks::<2>().0.iter().map(|u| u16::from_be_bytes(*u));
            Some(char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect())
        }
        _ => None,
    }
}

/// The entry of `name`: the same name first, else the first name that is
/// the same with no regard to case (as Windows reads the media).
pub(crate) fn lookup<'a>(entries: &'a [Entry], name: &str) -> Option<&'a Entry> {
    entries.iter().find(|e| e.name == name).or_else(|| {
        let name = name.to_lowercase();
        entries.iter().find(|e| e.name.to_lowercase() == name)
    })
}

impl<R: ReadAt> Udf<R> {
    /// The entries of the directory `icb`.
    pub(crate) fn entries(&mut self, icb: &Icb) -> Result<Vec<Entry>, Error> {
        let limit = self.limits.directory_bytes;
        if icb.size > limit {
            return Err(Error::Limit { what: "bytes of a directory", limit });
        }
        let extents = self.extents(icb)?;
        let mut buf = Vec::with_capacity(icb.size as usize);
        for part in &extents.parts {
            match *part {
                Extent::Data { offset, len } => buf.extend(volume::read(&mut self.reader, offset, len as usize)?),
                Extent::Zero { len } => buf.resize(buf.len() + len as usize, 0),
            }
        }
        let first = match extents.parts.first() {
            Some(Extent::Data { offset, .. }) => *offset,
            _ => icb.offset,
        };
        parse(&buf, first, self.limits.entries)
    }
}
