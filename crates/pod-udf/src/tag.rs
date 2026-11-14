//! The descriptor tag (ECMA-167 3/7.2) and the little-endian fields of a
//! descriptor.

use crate::error::{Error, TagFault};

/// The tag identifiers that the reader reads (ECMA-167 3/7.2.1, 4/7.2.1).
pub(crate) mod id {
    pub(crate) const AVDP: u16 = 2;
    pub(crate) const VDP: u16 = 3;
    pub(crate) const PD: u16 = 5;
    pub(crate) const LVD: u16 = 6;
    pub(crate) const TD: u16 = 8;
    pub(crate) const FSD: u16 = 256;
    pub(crate) const FID: u16 = 257;
    pub(crate) const AED: u16 = 258;
    pub(crate) const FE: u16 = 261;
    pub(crate) const EFE: u16 = 266;
}

/// The bytes of a tag.
pub(crate) const TAG: usize = 16;

/// The CRC of a descriptor: CRC-ITU-T, polynomial 0x1021, initial value 0
/// (ECMA-167 1/7.2.6).
pub(crate) fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// The sum of the 16 bytes of a tag, but byte 4 (the checksum itself).
pub(crate) fn checksum(tag: &[u8; TAG]) -> u8 {
    tag.iter().enumerate().filter(|(i, _)| *i != 4).fold(0u8, |sum, (_, b)| sum.wrapping_add(*b))
}

/// Check the tag at the head of `buf`: its checksum, its version, its CRC
/// over the bytes of `buf`, and its location when `location` is given.
/// Gives the tag identifier.
pub(crate) fn check(buf: &[u8], location: Option<u32>) -> Result<u16, TagFault> {
    let Some(tag) = buf.first_chunk::<TAG>() else {
        return Err(TagFault::Short);
    };
    if checksum(tag) != tag[4] {
        return Err(TagFault::Checksum);
    }
    let version = u16::from_le_bytes([tag[2], tag[3]]);
    if version != 2 && version != 3 {
        return Err(TagFault::Version(version));
    }
    let crc = u16::from_le_bytes([tag[8], tag[9]]);
    let crc_len = u16::from_le_bytes([tag[10], tag[11]]);
    let body = buf.get(TAG..TAG + usize::from(crc_len)).ok_or(TagFault::CrcLength(crc_len))?;
    if crc16(body) != crc {
        return Err(TagFault::Crc);
    }
    let got = u32::from_le_bytes([tag[12], tag[13], tag[14], tag[15]]);
    if let Some(want) = location
        && want != got
    {
        return Err(TagFault::Location { want, got });
    }
    Ok(u16::from_le_bytes([tag[0], tag[1]]))
}

/// Check the tag of a descriptor that must be one of `want`: the error
/// names the descriptor and its byte offset.
pub(crate) fn expect(
    buf: &[u8],
    offset: u64,
    what: &'static str,
    want: &[u16],
    location: Option<u32>,
) -> Result<u16, Error> {
    let fault = |fault| Error::Descriptor { offset, what, fault };
    let got = check(buf, location).map_err(fault)?;
    if want.contains(&got) { Ok(got) } else { Err(fault(TagFault::Identifier { want: want[0], got })) }
}

/// A view of the fields of one descriptor. A field past its end is
/// [`Error::Corrupt`] at the descriptor.
#[derive(Clone, Copy)]
pub(crate) struct Fields<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) offset: u64,
}

impl<'a> Fields<'a> {
    pub(crate) fn new(bytes: &'a [u8], offset: u64) -> Self {
        Self { bytes, offset }
    }

    pub(crate) fn short(&self) -> Error {
        Error::Corrupt { offset: self.offset, what: "a field past the end of its descriptor" }
    }

    pub(crate) fn slice(&self, at: usize, len: usize) -> Result<&'a [u8], Error> {
        at.checked_add(len).and_then(|end| self.bytes.get(at..end)).ok_or_else(|| self.short())
    }

    pub(crate) fn u8(&self, at: usize) -> Result<u8, Error> {
        Ok(self.slice(at, 1)?[0])
    }

    pub(crate) fn u16(&self, at: usize) -> Result<u16, Error> {
        let b = self.slice(at, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn u32(&self, at: usize) -> Result<u32, Error> {
        let b = self.slice(at, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u64(&self, at: usize) -> Result<u64, Error> {
        let b = self.slice(at, 8)?;
        let mut le = [0u8; 8];
        le.copy_from_slice(b);
        Ok(u64::from_le_bytes(le))
    }
}

/// A `long_ad` (ECMA-167 4/14.14.2): an extent of a partition that the
/// partition reference number names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LongAd {
    /// The extent length in bytes, with its type in the two high bits.
    pub(crate) len: u32,
    pub(crate) lbn: u32,
    pub(crate) partition: u16,
}

impl LongAd {
    pub(crate) fn read(fields: &Fields<'_>, at: usize) -> Result<Self, Error> {
        Ok(Self { len: fields.u32(at)?, lbn: fields.u32(at + 4)?, partition: fields.u16(at + 8)? })
    }
}
