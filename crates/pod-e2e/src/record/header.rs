//! The header of a record: 24 bytes, big-endian, which go as the associated
//! data of the AEAD. A changed byte of the header, the length of the
//! padding among them, fails the record's check.

use super::Fault;

/// The bytes of a header.
pub const HEADER: usize = 24;
/// The version of the records that this build writes, and the only one that
/// it reads.
pub const VERSION: u8 = 1;
/// The most bytes of a record's body (its payload and its padding): a
/// multiple of the last bucket, so that each payload up to it pads in it.
pub const MAX_LENGTH: usize = 16 << 20;
/// The sizes of the padded bodies; a body above the last pads to its next
/// multiple.
pub const BUCKETS: [usize; 5] = [0, 128, 512, 1024, 2048];
/// The bit of the flags that the layer sets: the parity of the key's epoch.
pub const PHASE: u16 = 1;

/// The fields of a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u8,
    pub kind: u8,
    pub flags: u16,
    pub stream: u32,
    pub counter: u64,
    /// The bytes of the body: the payload and the padding.
    pub length: u32,
    /// The bytes of the padding, at the end of the body.
    pub pad: u16,
    pub reserved: u16,
}

impl Header {
    pub fn encode(&self) -> [u8; HEADER] {
        let mut out = [0u8; HEADER];
        out[0] = self.version;
        out[1] = self.kind;
        out[2..4].copy_from_slice(&self.flags.to_be_bytes());
        out[4..8].copy_from_slice(&self.stream.to_be_bytes());
        out[8..16].copy_from_slice(&self.counter.to_be_bytes());
        out[16..20].copy_from_slice(&self.length.to_be_bytes());
        out[20..22].copy_from_slice(&self.pad.to_be_bytes());
        out[22..24].copy_from_slice(&self.reserved.to_be_bytes());
        out
    }

    /// The header at the start of `bytes`, checked: this version, no
    /// reserved bits, a length within [`MAX_LENGTH`], and a padding within
    /// the length.
    pub fn parse(bytes: &[u8]) -> Result<Header, Fault> {
        let b: &[u8; HEADER] = bytes
            .get(..HEADER)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| Fault::Malformed(format!("a record of {} bytes, shorter than its header", bytes.len())))?;
        let header = Header {
            version: b[0],
            kind: b[1],
            flags: u16::from_be_bytes([b[2], b[3]]),
            stream: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
            counter: u64::from_be_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]),
            length: u32::from_be_bytes([b[16], b[17], b[18], b[19]]),
            pad: u16::from_be_bytes([b[20], b[21]]),
            reserved: u16::from_be_bytes([b[22], b[23]]),
        };
        if header.version != VERSION {
            return Err(Fault::Malformed(format!("a record of version {}, where this end reads {VERSION}", b[0])));
        }
        if header.reserved != 0 {
            return Err(Fault::Malformed("a record with its reserved bytes set".into()));
        }
        if header.length as usize > MAX_LENGTH {
            return Err(Fault::Malformed(format!("a record of {} bytes, above {MAX_LENGTH}", header.length)));
        }
        if u32::from(header.pad) > header.length {
            return Err(Fault::Malformed(format!("a padding of {} in a body of {}", header.pad, header.length)));
        }
        Ok(header)
    }
}

/// The size of a body of `len` bytes of payload, padded: the first bucket
/// that holds it, or above the last, its next multiple.
pub fn bucket(len: usize) -> usize {
    let last = BUCKETS[BUCKETS.len() - 1];
    BUCKETS.into_iter().find(|b| len <= *b).unwrap_or_else(|| len.div_ceil(last) * last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_comes_back_whole_in_big_endian() {
        let h = Header {
            version: VERSION,
            kind: 9,
            flags: 0x0102,
            stream: 0x0304_0506,
            counter: 0x0708_090a_0b0c_0d0e,
            length: 300,
            pad: 212,
            reserved: 0,
        };
        let bytes = h.encode();
        assert_eq!(&bytes[..8], &[1, 9, 1, 2, 3, 4, 5, 6]);
        assert_eq!(&bytes[8..16], &[7, 8, 9, 10, 11, 12, 13, 14]);
        assert_eq!(Header::parse(&bytes).unwrap(), h);
        let refused = |mut b: [u8; HEADER], at: usize, v: u8| {
            b[at] = v;
            Header::parse(&b).is_err()
        };
        assert!(refused(bytes, 0, 2), "another version");
        assert!(refused(bytes, 23, 1), "a reserved bit");
        assert!(refused(bytes, 16, 0x10), "a length above the limit");
        assert!(refused(bytes, 20, 0x10), "a padding longer than the body");
        assert!(Header::parse(&bytes[..23]).is_err());
    }

    #[test]
    fn a_payload_pads_to_the_next_bucket() {
        let cases = [(0, 0), (1, 128), (128, 128), (129, 512), (600, 1024), (1024, 1024), (1025, 2048)];
        for (len, want) in cases {
            assert_eq!(bucket(len), want, "{len}");
        }
        assert_eq!((bucket(2048), bucket(2049), bucket(5000)), (2048, 4096, 6144));
        assert_eq!(bucket(MAX_LENGTH), MAX_LENGTH, "the largest body pads in its limit");
    }
}
